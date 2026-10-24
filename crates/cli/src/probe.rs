//! `zapret-hub-cli probe <url|domain>`: where a host resolves, which of zapret2's lists and which
//! routing rule take it, and whether it answers now (HTTP, TLS 1.2, TLS 1.3, ping) through
//! whatever filters at the moment.

use std::net::{Ipv4Addr, ToSocketAddrs as _};
use std::path::Path;

use probe::curl::{CheckKind, CheckResult};
use router::rules::{Rule, Table, covers, parse_net};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub host: String,
    pub url: String,
    /// What the system resolver answers, without doubles; empty when it fails (`dns_error`).
    pub dns: Vec<String>,
    pub dns_error: Option<String>,
    /// The files of zapret2's `lists` that hold the host (`list-*`) or one of its IPv4s
    /// (`ipset-*`); `exclude` in a name lets it through untouched.
    pub lists: Vec<String>,
    /// Why the lists were not read: zapret2 is not set or does not open.
    pub lists_error: Option<String>,
    /// The first enabled rule of routes.toml that takes the host or one of its IPv4s.
    pub route: Option<String>,
    pub checks: Vec<CheckResult>,
    pub ping_ms: Option<u32>,
}

/// `discord.com`, `https://Discord.com:8443/x` → the host (lower case, no port, userinfo or
/// trailing dot) and the URL the checks ask (a bare name or `host:port` gets `https://`).
pub fn target(input: &str) -> anyhow::Result<(String, String)> {
    let input = input.trim();
    let lower = input.to_ascii_lowercase();
    let (rest, url) = match ["https://", "http://"]
        .iter()
        .find(|scheme| lower.starts_with(**scheme))
    {
        Some(scheme) => (&input[scheme.len()..], None),
        None => (input, Some(format!("https://{input}"))),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    anyhow::ensure!(
        !host.is_empty() && !host.contains(char::is_whitespace),
        "not a domain or URL: {input:?}"
    );
    Ok((host, url.unwrap_or_else(|| input.to_owned())))
}

/// A hostlist line takes `host`: the name and its subdomains (`*.`, `+.` and a leading `.` say
/// the same), `^name` the name only. Comments, blanks and addresses take nothing.
pub fn name_listed(line: &str, host: &str) -> bool {
    let line = line.trim().to_ascii_lowercase();
    if line.is_empty() || line.starts_with('#') || parse_net(&line).is_some() {
        return false;
    }
    if let Some(exact) = line.strip_prefix('^') {
        return exact.trim_end_matches('.') == host;
    }
    let name = line.trim_end_matches('.');
    let name = ["*.", "+.", "."]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or(name);
    !name.is_empty() && covers(name, host)
}

/// An ipset line (an IPv4 or a CIDR) holds `ip`.
// ponytail: IPv6 lines are skipped; the hub's lists carry IPv4.
pub fn ip_listed(line: &str, ip: Ipv4Addr) -> bool {
    parse_net(line.trim()).is_some_and(|(a, b)| (a..=b).contains(&u32::from(ip)))
}

/// The list files of `dir` that take `host` or one of `ips`, by name.
pub fn lists(dir: &Path, host: &str, ips: &[Ipv4Addr]) -> anyhow::Result<Vec<String>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !name.ends_with(".txt") {
            continue;
        }
        let Ok(text) = zapret::read_text(&path) else {
            continue;
        };
        let hit = if name.starts_with("ipset-") {
            text.lines()
                .any(|line| ips.iter().any(|ip| ip_listed(line, *ip)))
        } else {
            text.lines().any(|line| name_listed(line, host))
        };
        if hit {
            found.push(name);
        }
    }
    found.sort();
    Ok(found)
}

/// The first enabled rule that takes the name, else one of the addresses.
pub fn route(rules: &[Rule], host: &str, ips: &[Ipv4Addr]) -> Option<String> {
    let table = Table::new(rules);
    std::iter::once(None)
        .chain(ips.iter().copied().map(Some))
        .find_map(|ip| table.rule_for(host, ip))
        .map(str::to_owned)
}

/// The whole probe. `zapret2_dir` is the setting; `rules` the routes the hub has.
pub fn run(input: &str, zapret2_dir: Option<&str>, rules: &[Rule]) -> anyhow::Result<Report> {
    let (host, url) = target(input)?;
    let (mut dns, dns_error) = match (host.as_str(), 443).to_socket_addrs() {
        Ok(addrs) => (addrs.map(|a| a.ip().to_string()).collect(), None),
        Err(err) => (Vec::new(), Some(err.to_string())),
    };
    dns.sort();
    dns.dedup();
    let ips: Vec<Ipv4Addr> = dns.iter().filter_map(|ip| ip.parse().ok()).collect();
    let (lists, lists_error) = match zapret2_dir {
        None => (Vec::new(), Some("the zapret2 folder is not set".to_owned())),
        Some(dir) => match zapret2::Engine::open(dir)
            .and_then(|engine| lists(&zapret2::strategies::lists_dir(&engine), &host, &ips))
        {
            Ok(found) => (found, None),
            Err(err) => (Vec::new(), Some(format!("{err:#}"))),
        },
    };
    let route = route(rules, &host, &ips);
    // Network waits: all at once take the longest one.
    let (checks, ping_ms) = std::thread::scope(|scope| {
        let checks: Vec<_> = CheckKind::ALL
            .iter()
            .map(|kind| scope.spawn(|| probe::curl::check(&url, *kind, None)))
            .collect();
        let ping_ms = probe::ping::ping(&host, 1000);
        let checks = checks
            .into_iter()
            .map(|check| check.join().expect("a curl check does not panic"))
            .collect();
        (checks, ping_ms)
    });
    Ok(Report {
        host,
        url,
        dns,
        dns_error,
        lists,
        lists_error,
        route,
        checks,
        ping_ms,
    })
}

/// The report for a person.
pub fn text(report: &Report) -> String {
    let none = || "—".to_owned();
    let mut out = format!("{} ({})\n", report.host, report.url);
    let dns = match &report.dns_error {
        Some(err) => format!("ошибка: {err}"),
        None => report.dns.join(", "),
    };
    out += &format!("DNS: {dns}\n");
    let lists = match &report.lists_error {
        Some(err) => format!("не прочитаны: {err}"),
        None if report.lists.is_empty() => none(),
        None => report.lists.join(", "),
    };
    out += &format!("Списки: {lists}\n");
    out += &format!("Маршрут: {}\n", report.route.clone().unwrap_or_else(none));
    for check in &report.checks {
        let time = check
            .time_ms
            .map_or_else(String::new, |ms| format!(", {ms} мс"));
        out += &format!(
            "{}: {:?} (HTTP {}{time})\n",
            check.kind.label(),
            check.status,
            check.http_code
        );
    }
    let ping = report.ping_ms.map_or_else(none, |ms| format!("{ms} мс"));
    out += &format!("Ping: {ping}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use router::rules::Protocol;

    #[test]
    fn a_domain_or_url_gives_the_host_and_the_url() {
        let t = |s: &str| target(s).unwrap();
        assert_eq!(
            t(" Discord.COM "),
            ("discord.com".into(), "https://Discord.COM".into())
        );
        assert_eq!(
            t("example.com:8443"),
            ("example.com".into(), "https://example.com:8443".into())
        );
        assert_eq!(
            t("HTTP://Example.com./x?y=1"),
            ("example.com".into(), "HTTP://Example.com./x?y=1".into())
        );
        assert_eq!(t("https://u:p@example.com:443/p").0, "example.com");
        assert_eq!(t("https://[::1]:9001/p").0, "::1");
        assert!(target("").is_err());
        assert!(target("https://").is_err());
        assert!(target("a b").is_err());
    }

    #[test]
    fn hostlist_lines_take_the_name_and_its_subdomains() {
        for line in [
            "discord.com",
            "+.discord.com",
            "*.discord.com",
            ".discord.com",
            " DISCORD.com. ",
        ] {
            assert!(name_listed(line, "discord.com"), "{line}");
            assert!(name_listed(line, "gw.discord.com"), "{line}");
        }
        assert!(name_listed("^discord.com", "discord.com"));
        assert!(!name_listed("^discord.com", "gw.discord.com"));
        assert!(!name_listed("discord.com", "discord.com.evil.io"));
        assert!(!name_listed("cord.com", "discord.com"));
        assert!(!name_listed("# discord.com", "discord.com"));
        assert!(!name_listed("", "discord.com"));
        assert!(!name_listed("1.2.3.4", "1.2.3.4"));
    }

    #[test]
    fn ipset_lines_hold_an_address_or_a_subnet() {
        let ip: Ipv4Addr = "203.0.113.9".parse().unwrap();
        assert!(ip_listed("203.0.113.0/24", ip));
        assert!(ip_listed(" 203.0.113.9 ", ip));
        assert!(!ip_listed("203.0.114.0/24", ip));
        assert!(!ip_listed("discord.com", ip));
        assert!(!ip_listed("::1", ip));
    }

    #[test]
    fn lists_name_the_files_that_take_the_host_or_an_address() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
        write("list-general.txt", "# x\ndiscord.com\n");
        write("list-google.txt", "youtube.com\n");
        write("list-exclude-user.txt", "gw.discord.com\n");
        write("ipset-exclude.txt", "203.0.113.0/24\n");
        write("ipset-all.txt", "198.51.100.0/24\n");
        write("notes.md", "discord.com\n");
        let ips = ["203.0.113.9".parse().unwrap()];
        assert_eq!(
            lists(dir.path(), "gw.discord.com", &ips).unwrap(),
            [
                "ipset-exclude.txt",
                "list-exclude-user.txt",
                "list-general.txt"
            ]
        );
        assert!(lists(dir.path(), "example.org", &[]).unwrap().is_empty());
    }

    #[test]
    fn the_route_is_the_first_enabled_rule_by_name_or_address() {
        let rule = |name: &str, enabled: bool, domains: &[&str], ips: &[&str]| Rule {
            name: name.into(),
            enabled,
            protocol: Protocol::Both,
            ports: vec![],
            ips: ips.iter().map(|s| (*s).into()).collect(),
            domains: domains.iter().map(|s| (*s).into()).collect(),
            apps: vec![],
        };
        let rules = [
            rule("Off", false, &["discord.com"], &[]),
            rule("Net", true, &[], &["203.0.113.0/24"]),
            rule("Discord", true, &["discord.com"], &[]),
        ];
        let ip: Ipv4Addr = "203.0.113.9".parse().unwrap();
        assert_eq!(
            route(&rules, "gw.discord.com", &[ip]).as_deref(),
            Some("Discord")
        );
        assert_eq!(route(&rules, "example.org", &[ip]).as_deref(), Some("Net"));
        assert_eq!(route(&rules, "example.org", &[]), None);
    }
}

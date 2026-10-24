//! Whether the current strategy and lists cover a culprit (agent proposal 1, 5): pure matching;
//! the app reads the files and the running state (`src-tauri/src/diag/context.rs`).

use std::net::Ipv4Addr;

use router::rules::Protocol;

use crate::agent::Coverage;
use crate::cover::strategy_covers;

/// Everything `coverage` needs about one culprit group and the setup.
pub struct Input<'a> {
    pub key: &'a str,
    pub route: String,
    pub protocol: Protocol,
    pub ports: &'a [u16],
    pub ips: &'a [Ipv4Addr],
    /// The group's key, when it is a name, and the names its flows carried.
    pub hosts: &'a [String],
    /// The current strategy's final args.
    pub args: Option<&'a str>,
    /// Lines of `ipset-all.txt` / of the hostlists; `None`: no such list.
    pub ipset: Option<&'a [String]>,
    pub hostlist: Option<&'a [String]>,
    pub list_age_days: Option<u32>,
}

fn entries(lines: &[String]) -> impl Iterator<Item = &str> {
    lines
        .iter()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// `a.b.c.d` or `a.b.c.d/n`; an empty list is winws's "any" and matches everything.
pub fn ip_in_list(ip: Ipv4Addr, lines: &[String]) -> bool {
    let mut any = true;
    for entry in entries(lines) {
        any = false;
        let (net, bits) = entry.split_once('/').unwrap_or((entry, "32"));
        let (Ok(net), Ok(bits)) = (net.parse::<Ipv4Addr>(), bits.parse::<u32>()) else {
            continue;
        };
        if bits > 32 {
            continue;
        }
        let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
        if u32::from(ip) & mask == u32::from(net) & mask {
            return true;
        }
    }
    any
}

/// winws hostlists match the name and its subdomains.
pub fn host_in_list(host: &str, lines: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    entries(lines).any(|e| {
        let e = e
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .to_ascii_lowercase();
        host == e || host.ends_with(&format!(".{e}"))
    })
}

pub fn coverage(i: &Input) -> Coverage {
    Coverage {
        key: i.key.to_owned(),
        route: i.route.clone(),
        in_hostlist: i
            .hostlist
            .filter(|_| !i.hosts.is_empty())
            .map(|l| i.hosts.iter().any(|h| host_in_list(h, l))),
        in_ipset: i
            .ipset
            .filter(|_| !i.ips.is_empty())
            .map(|l| i.ips.iter().any(|ip| ip_in_list(*ip, l))),
        game_filter_covers: i
            .args
            .filter(|_| !i.ports.is_empty())
            .map(|a| i.ports.iter().all(|p| strategy_covers(a, i.protocol, *p))),
        list_age_days: i.list_age_days,
    }
}

/// `zapret`, `zapret2`, `router:<rule>`, `system_proxy`, `direct`.
pub fn route(rule: Option<&str>, system_proxy: bool, engine: Option<&str>) -> String {
    match (rule, system_proxy, engine) {
        (Some(rule), ..) => format!("router:{rule}"),
        (None, true, _) => "system_proxy".into(),
        (None, false, Some(engine)) => engine.into(),
        _ => "direct".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn cidr_and_plain_addresses() {
        let l = lines(&["# c", "203.0.113.0/24", "198.51.100.7"]);
        assert!(ip_in_list("203.0.113.9".parse().unwrap(), &l));
        assert!(ip_in_list("198.51.100.7".parse().unwrap(), &l));
        assert!(!ip_in_list("198.51.100.8".parse().unwrap(), &l));
        assert!(ip_in_list(
            "1.2.3.4".parse().unwrap(),
            &lines(&["", "# only"])
        ));
        assert!(ip_in_list(
            "1.2.3.4".parse().unwrap(),
            &lines(&["0.0.0.0/0"])
        ));
    }

    #[test]
    fn hostlist_matches_subdomains_only() {
        let l = lines(&["discord.com", "*.steamstatic.com"]);
        assert!(host_in_list("Gateway.discord.com", &l));
        assert!(host_in_list("steamstatic.com", &l));
        assert!(!host_in_list("notdiscord.com", &l));
    }

    #[test]
    fn coverage_fills_what_is_known() {
        let ips = ["203.0.113.5".parse().unwrap()];
        let hosts = ["x.example.org".to_owned()];
        let ipset = lines(&["203.0.113.0/24"]);
        let host = lines(&["example.com"]);
        let c = coverage(&Input {
            key: "k",
            route: "zapret".into(),
            protocol: Protocol::Udp,
            ports: &[50001],
            ips: &ips,
            hosts: &hosts,
            args: Some("--filter-udp=443 --new --filter-udp=50000-50100"),
            ipset: Some(&ipset),
            hostlist: Some(&host),
            list_age_days: Some(3),
        });
        assert_eq!(
            (
                c.in_ipset,
                c.in_hostlist,
                c.game_filter_covers,
                c.list_age_days
            ),
            (Some(true), Some(false), Some(true), Some(3))
        );
        let c = coverage(&Input {
            key: "k",
            route: "direct".into(),
            protocol: Protocol::Tcp,
            ports: &[],
            ips: &[],
            hosts: &[],
            args: None,
            ipset: None,
            hostlist: None,
            list_age_days: None,
        });
        assert_eq!(
            (c.in_ipset, c.in_hostlist, c.game_filter_covers),
            (None, None, None)
        );
    }

    #[test]
    fn route_names() {
        assert_eq!(route(Some("r"), true, Some("zapret")), "router:r");
        assert_eq!(route(None, true, Some("zapret")), "system_proxy");
        assert_eq!(route(None, false, Some("zapret2")), "zapret2");
        assert_eq!(route(None, false, None), "direct");
    }
}

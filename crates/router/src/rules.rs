//! `routes.toml`: rule format, validation, the voice preset, ipset import and the match table.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Spanned;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    pub protocol: Protocol,
    pub ports: Vec<String>,
    pub ips: Vec<String>,
    pub domains: Vec<String>,
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    DiscordVoice,
    Discord,
    #[serde(rename = "dbd")]
    Dbd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RulesError {
    pub line: Option<u32>,
    pub field: Option<String>,
    pub message: String,
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        if let Some(field) = &self.field {
            write!(f, "{field}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for RulesError {}

pub fn preset(preset: Preset) -> Rule {
    match preset {
        Preset::DiscordVoice => Rule {
            name: "Discord голос".into(),
            enabled: true,
            protocol: Protocol::Udp,
            ports: vec!["19294-19344".into(), "50000-65535".into()],
            ips: vec!["104.29.0.0/16".into(), "35.217.0.0/16".into()],
            domains: vec!["discord.media".into()],
            apps: vec![],
        },
        Preset::Discord => Rule {
            name: "Discord целиком".into(),
            enabled: true,
            protocol: Protocol::Both,
            ports: vec![],
            ips: vec![],
            domains: [
                "discord.com",
                "discord.gg",
                "discordapp.com",
                "discordapp.net",
                "discord.media",
            ]
            .map(String::from)
            .into(),
            apps: vec![],
        },
        Preset::Dbd => Rule {
            name: "Dead by Daylight".into(),
            enabled: true,
            protocol: Protocol::Both,
            ports: vec![],
            ips: vec![],
            domains: vec!["bhvrdbd.com".into()],
            apps: vec!["DeadByDaylight-Win64-Shipping.exe".into()],
        },
    }
}

fn parse_port(s: &str) -> Option<(u16, u16)> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b) = (a.parse::<u16>().ok()?, b.parse::<u16>().ok()?);
    (1 <= a && a <= b).then_some((a, b))
}

/// `a.b.c.d` or `a.b.c.d/n` as an inclusive range; host bits are zeroed.
fn parse_net(s: &str) -> Option<(u32, u32)> {
    let (ip, bits) = match s.split_once('/') {
        Some((ip, n)) => (ip, n.parse::<u32>().ok().filter(|n| *n <= 32)?),
        None => (s, 32),
    };
    let ip = u32::from(ip.parse::<Ipv4Addr>().ok()?);
    let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
    Some((ip & mask, ip | !mask))
}

fn valid_name(name: &str) -> bool {
    let letter =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё');
    let name = name.trim();
    (1..=40).contains(&name.chars().count())
        && name.chars().next().is_some_and(letter)
        && name
            .chars()
            .all(|c| letter(c) || matches!(c, ' ' | '-' | '_' | '.'))
}

/// Why `d` is not an LDH host name with at least two labels, if it is not.
fn domain_error(d: &str) -> Option<String> {
    if d.starts_with("*.") {
        return Some(format!(
            "\"{d}\": write the domain itself, its subdomains are included"
        ));
    }
    if !d.is_ascii() {
        return Some(format!(
            "\"{d}\": write an international domain in its xn-- form"
        ));
    }
    let name = d.strip_suffix('.').unwrap_or(d);
    let labels: Vec<&str> = name.split('.').collect();
    let ldh = |l: &str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    };
    if name.len() > 253 || labels.len() < 2 || !labels.iter().all(|l| ldh(l)) {
        return Some(format!(
            "\"{d}\" is not a domain: letters, digits and - in dot-separated labels, no scheme, port or path"
        ));
    }
    if labels[labels.len() - 1].bytes().all(|c| c.is_ascii_digit()) {
        return Some(format!("\"{d}\" is an IP address: put it in ips"));
    }
    None
}

/// Both already lowercase without a trailing dot: equal, or `name` is a subdomain of `domain`.
pub fn covers(domain: &str, name: &str) -> bool {
    name == domain
        || name
            .strip_suffix(domain)
            .is_some_and(|head| head.ends_with('.'))
}

/// A file name or an absolute path, either ending in .exe.
fn valid_app(a: &str) -> bool {
    let stem = a.len().checked_sub(4).filter(|&i| a.is_char_boundary(i));
    let named = stem.is_some_and(|i| i > 0 && a[i..].eq_ignore_ascii_case(".exe"));
    named
        && !a.contains(['*', '?', '"', '<', '>', '|'])
        && (Path::new(a).is_absolute() || !a.contains(['\\', '/', ':']))
}

/// Lowercase with backslash separators: how programs are compared.
fn norm_app(a: &str) -> String {
    a.to_lowercase().replace('/', "\\")
}

/// Whether the executable exe is the program app (already norm_app): a full path or a file name.
pub fn runs(app: &str, exe: &str) -> bool {
    let exe = norm_app(exe);
    if app.contains('\\') {
        exe == app
    } else {
        exe.rsplit('\\').next() == Some(app)
    }
}

fn err(field: &str, message: impl Into<String>) -> FieldError {
    FieldError {
        field: field.into(),
        message: message.into(),
    }
}

pub fn check(rule: &Rule) -> Vec<FieldError> {
    let mut out = Vec::new();
    if !valid_name(&rule.name) {
        out.push(err(
            "name",
            "1-40 characters: Latin or Cyrillic letters, digits, space, - _ .; starts with a letter or digit",
        ));
    }
    for p in &rule.ports {
        if parse_port(p).is_none() {
            out.push(err(
                "ports",
                format!("\"{p}\" is not a port or a range a-b within 1-65535"),
            ));
        }
    }
    for ip in &rule.ips {
        if parse_net(ip).is_none() {
            out.push(err(
                "ips",
                format!("\"{ip}\" is not an IPv4 address or a.b.c.d/n subnet"),
            ));
        }
    }
    for d in &rule.domains {
        if let Some(message) = domain_error(d) {
            out.push(err("domains", message));
        }
    }
    for a in &rule.apps {
        if !valid_app(a) {
            out.push(err(
                "apps",
                format!(
                    "\"{a}\" is not a program: write its file name like Discord.exe or its full path like C:\\Games\\Discord.exe"
                ),
            ));
        }
    }
    if rule.ips.is_empty() && rule.domains.is_empty() && rule.apps.is_empty() {
        out.push(err(
            "ips",
            "a rule needs at least one target: ips, domains or apps",
        ));
    }
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: Option<Spanned<String>>,
    enabled: Option<Spanned<bool>>,
    protocol: Spanned<Protocol>,
    ports: Option<Spanned<Vec<String>>>,
    ips: Option<Spanned<Vec<String>>>,
    domains: Option<Spanned<Vec<String>>>,
    apps: Option<Spanned<Vec<String>>>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    rule: Vec<Spanned<Raw>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    rule: Spanned<Raw>,
}

fn line_of(text: &str, offset: usize) -> u32 {
    1 + text[..offset.min(text.len())].matches('\n').count() as u32
}

fn syntax(text: &str, e: toml::de::Error) -> RulesError {
    RulesError {
        line: e.span().map(|s| line_of(text, s.start)),
        field: None,
        message: e.message().trim().to_string(),
    }
}

/// Checks one raw rule; `name` is the CLI file's name (such a file has none of its own).
fn build(text: &str, table: &Spanned<Raw>, name: Option<&str>) -> Result<Rule, RulesError> {
    let raw = table.get_ref();
    let at = |span: std::ops::Range<usize>| Some(line_of(text, span.start));
    let own = raw.name.as_ref();
    let rule_name = match (name, own) {
        (Some(_), Some(n)) => {
            return Err(RulesError {
                line: at(n.span()),
                field: Some("name".into()),
                message: "the name comes from the file name, remove it from the file".into(),
            });
        }
        (Some(n), None) => n.to_string(),
        (None, n) => n.map(|n| n.get_ref().clone()).unwrap_or_default(),
    };
    let rule = Rule {
        name: rule_name,
        enabled: raw.enabled.as_ref().is_none_or(|e| *e.get_ref()),
        protocol: *raw.protocol.get_ref(),
        ports: raw
            .ports
            .as_ref()
            .map(|p| p.get_ref().clone())
            .unwrap_or_default(),
        ips: raw
            .ips
            .as_ref()
            .map(|p| p.get_ref().clone())
            .unwrap_or_default(),
        domains: raw
            .domains
            .as_ref()
            .map(|p| p.get_ref().clone())
            .unwrap_or_default(),
        apps: raw
            .apps
            .as_ref()
            .map(|p| p.get_ref().clone())
            .unwrap_or_default(),
    };
    if let Some(e) = check(&rule).into_iter().next() {
        let span = match e.field.as_str() {
            "name" => own.map(|v| v.span()),
            "protocol" => Some(raw.protocol.span()),
            "ports" => raw.ports.as_ref().map(|v| v.span()),
            "domains" => raw.domains.as_ref().map(|v| v.span()),
            "apps" => raw.apps.as_ref().map(|v| v.span()),
            _ => raw.ips.as_ref().map(|v| v.span()),
        }
        .unwrap_or_else(|| table.span());
        return Err(RulesError {
            line: at(span),
            field: Some(e.field),
            message: e.message,
        });
    }
    Ok(rule)
}

pub fn parse(text: &str) -> Result<Vec<Rule>, RulesError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: File = toml::from_str(text).map_err(|e| syntax(text, e))?;
    let mut rules: Vec<Rule> = Vec::new();
    for table in &file.rule {
        let rule = build(text, table, None)?;
        if position(&rules, &rule.name).is_some() {
            let span = table
                .get_ref()
                .name
                .as_ref()
                .map_or(table.span(), |n| n.span());
            return Err(RulesError {
                line: Some(line_of(text, span.start)),
                field: Some("name".into()),
                message: format!("a rule named {} already exists", rule.name),
            });
        }
        rules.push(rule);
    }
    Ok(rules)
}

pub fn parse_rule_file(name: &str, text: &str) -> Result<Rule, RulesError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let one: One = toml::from_str(text).map_err(|e| syntax(text, e))?;
    build(text, &one.rule, Some(name))
}

pub fn load(path: &Path) -> Result<Vec<Rule>, RulesError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(RulesError {
            line: None,
            field: None,
            message: format!("{}: {e}", path.display()),
        }),
    }
}

pub fn save(path: &Path, rules: &[Rule]) -> Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        rule: &'a [Rule],
    }
    let text = toml::to_string(&Out { rule: rules }).context("serializing routes.toml")?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Imported {
    pub ips: Vec<String>,
    pub domains: Vec<String>,
}

/// `ips` and `domains` plus the new lines of a list file (one subnet or domain per line, `#`
/// comments), without repeats.
pub fn import(ips: &[String], domains: &[String], text: &str) -> Result<Imported, RulesError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Imported {
        ips: ips.to_vec(),
        domains: domains.to_vec(),
    };
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |field: &str, message: String| RulesError {
            line: Some(i as u32 + 1),
            field: Some(field.into()),
            message,
        };
        if parse_net(line).is_some() {
            if !out.ips.iter().any(|x| x == line) {
                out.ips.push(line.to_string());
            }
        } else if line
            .bytes()
            .all(|c| c.is_ascii_digit() || c == b'.' || c == b'/')
        {
            return Err(at(
                "ips",
                format!("\"{line}\" is not an IPv4 address or a.b.c.d/n subnet"),
            ));
        } else if let Some(message) = domain_error(line) {
            return Err(at("domains", message));
        } else {
            let d = line.strip_suffix('.').unwrap_or(line).to_ascii_lowercase();
            if !out.domains.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                out.domains.push(d);
            }
        }
    }
    Ok(out)
}

fn position(rules: &[Rule], name: &str) -> Option<usize> {
    let name = name.to_lowercase();
    rules.iter().position(|r| r.name.to_lowercase() == name)
}

pub fn find<'a>(rules: &'a [Rule], name: &str) -> Result<&'a Rule> {
    match position(rules, name) {
        Some(i) => Ok(&rules[i]),
        None => bail!("no rule named {name}"),
    }
}

pub fn add(rules: &mut Vec<Rule>, rule: Rule) -> Result<()> {
    if position(rules, &rule.name).is_some() {
        bail!("a rule named {} already exists", rule.name);
    }
    rules.push(rule);
    Ok(())
}

pub fn replace(rules: &mut [Rule], rule: Rule) -> Result<()> {
    match position(rules, &rule.name) {
        Some(i) => rules[i] = rule,
        None => bail!("no rule named {}", rule.name),
    }
    Ok(())
}

pub fn set_enabled(rules: &mut [Rule], name: &str, enabled: bool) -> Result<()> {
    match position(rules, name) {
        Some(i) => rules[i].enabled = enabled,
        None => bail!("no rule named {name}"),
    }
    Ok(())
}

pub fn remove(rules: &mut Vec<Rule>, name: &str) -> Result<Rule> {
    match position(rules, name) {
        Some(i) => Ok(rules.remove(i)),
        None => bail!("no rule named {name}"),
    }
}

/// Sorts and merges overlapping or adjacent inclusive ranges.
pub(crate) fn merge(mut nets: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    nets.sort_unstable();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for n in nets {
        match ranges.last_mut() {
            Some(cur) if n.0 <= cur.1.saturating_add(1) => cur.1 = cur.1.max(n.1),
            _ => ranges.push(n),
        }
    }
    ranges
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub name: String,
    pub protocol: Protocol,
    /// Lowercase, no trailing dot.
    pub domains: Vec<String>,
    /// Empty means every port.
    pub ports: Vec<(u16, u16)>,
    /// Sorted, merged, inclusive.
    pub ranges: Vec<(u32, u32)>,
    /// Programs, lowercase with backslashes.
    pub apps: Vec<String>,
    /// A UDP rule's program is running, so its UDP is captured.
    pub running: bool,
}

impl Compiled {
    fn takes(&self, protocol: Protocol, port: u16) -> bool {
        (self.protocol == Protocol::Both || self.protocol == protocol)
            && (self.ports.is_empty() || self.ports.iter().any(|&(a, b)| (a..=b).contains(&port)))
    }

    fn runs_any(&self, exe: &str) -> bool {
        self.apps.iter().any(|a| runs(a, exe))
    }

    fn udp_apps(&self) -> bool {
        self.protocol != Protocol::Tcp && !self.apps.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub rules: Vec<Compiled>,
}

impl Table {
    /// Only enabled rules; the rules must already pass `check`.
    pub fn new(rules: &[Rule]) -> Table {
        let rules = rules
            .iter()
            .filter(|r| r.enabled)
            .map(|r| Compiled {
                name: r.name.clone(),
                protocol: r.protocol,
                domains: r
                    .domains
                    .iter()
                    .map(|d| d.strip_suffix('.').unwrap_or(d).to_ascii_lowercase())
                    .collect(),
                ports: r.ports.iter().filter_map(|s| parse_port(s)).collect(),
                ranges: merge(r.ips.iter().filter_map(|s| parse_net(s)).collect()),
                apps: r.apps.iter().map(|a| norm_app(a)).collect(),
                running: false,
            })
            .collect();
        Table { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Name of the first rule (file order) that matches a flow of `protocol` to the destination.
    pub fn matches(&self, protocol: Protocol, dst: SocketAddrV4) -> Option<&str> {
        let ip = u32::from(*dst.ip());
        let port = dst.port();
        self.rules
            .iter()
            .find(|r| {
                (r.protocol == Protocol::Both || r.protocol == protocol)
                    && (r.ports.is_empty() || r.ports.iter().any(|&(a, b)| (a..=b).contains(&port)))
                    && r.ranges.iter().any(|&(a, b)| (a..=b).contains(&ip))
            })
            .map(|r| r.name.as_str())
    }

    /// Like matches, and a rule with programs also takes the flow of its exe.
    pub fn matches_flow(
        &self,
        protocol: Protocol,
        dst: SocketAddrV4,
        exe: Option<&str>,
    ) -> Option<&str> {
        let ip = u32::from(*dst.ip());
        self.rules
            .iter()
            .find(|r| {
                r.takes(protocol, dst.port())
                    && (r.ranges.iter().any(|&(a, b)| (a..=b).contains(&ip))
                        || exe.is_some_and(|e| r.runs_any(e)))
            })
            .map(|r| r.name.as_str())
    }

    /// Whether a rule with programs takes flows of protocol to port (UDP: only while it runs).
    pub fn wants_owner(&self, protocol: Protocol, port: u16) -> bool {
        self.rules.iter().any(|r| {
            !r.apps.is_empty()
                && r.takes(protocol, port)
                && (protocol != Protocol::Udp || r.running)
        })
    }

    /// Marks the UDP rules whose program is among exes as running.
    pub fn with_running(mut self, exes: &[&str]) -> Table {
        for r in self.rules.iter_mut().filter(|r| r.udp_apps()) {
            r.running = exes.iter().any(|e| r.runs_any(e));
        }
        self
    }

    /// Some UDP rule with programs has none running.
    pub fn idle_apps(&self) -> bool {
        self.rules.iter().any(|r| r.udp_apps() && !r.running)
    }

    /// The exe is the program of a UDP rule not yet marked running.
    pub fn starts(&self, exe: &str) -> bool {
        self.rules
            .iter()
            .any(|r| r.udp_apps() && !r.running && r.runs_any(exe))
    }

    pub fn match_udp(&self, dst: SocketAddrV4) -> Option<&str> {
        self.matches(Protocol::Udp, dst)
    }

    /// Whether a domain of some rule covers `name` (lowercase).
    pub fn wants(&self, name: &str) -> bool {
        self.rules
            .iter()
            .any(|r| r.domains.iter().any(|d| covers(d, name)))
    }

    /// Adds each resolved IP to the rules whose domains cover its name.
    pub fn with_names(mut self, names: &[(Ipv4Addr, String)]) -> Table {
        for r in &mut self.rules {
            let mut nets = std::mem::take(&mut r.ranges);
            for (ip, name) in names {
                let name = name.to_ascii_lowercase();
                if r.domains.iter().any(|d| covers(d, &name)) {
                    nets.push((u32::from(*ip), u32::from(*ip)));
                }
            }
            r.ranges = merge(nets);
        }
        self
    }
}

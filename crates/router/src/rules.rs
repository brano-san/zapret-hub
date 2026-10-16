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
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
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
    if rule.protocol != Protocol::Udp {
        let p = if rule.protocol == Protocol::Tcp {
            "tcp"
        } else {
            "both"
        };
        out.push(err(
            "protocol",
            format!("protocol \"{p}\" is not supported yet: only \"udp\""),
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
    if rule.ips.is_empty() {
        out.push(err("ips", "a rule needs at least one target: ips"));
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
    domains: Option<Spanned<toml::Value>>,
    apps: Option<Spanned<toml::Value>>,
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
    for (field, value) in [("domains", &raw.domains), ("apps", &raw.apps)] {
        if let Some(v) = value {
            return Err(RulesError {
                line: at(v.span()),
                field: Some(field.into()),
                message: format!("`{field}` is not supported yet"),
            });
        }
    }
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
    };
    if let Some(e) = check(&rule).into_iter().next() {
        let span = match e.field.as_str() {
            "name" => own.map(|v| v.span()),
            "protocol" => Some(raw.protocol.span()),
            "ports" => raw.ports.as_ref().map(|v| v.span()),
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

/// `ips` plus the new subnets of an ipset-style file (one per line, `#` comments), without repeats.
pub fn import(ips: &[String], text: &str) -> Result<Vec<String>, RulesError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = ips.to_vec();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if parse_net(line).is_none() {
            return Err(RulesError {
                line: Some(i as u32 + 1),
                field: Some("ips".into()),
                message: format!("\"{line}\" is not an IPv4 address or a.b.c.d/n subnet"),
            });
        }
        if !out.iter().any(|x| x == line) {
            out.push(line.to_string());
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

#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub name: String,
    /// Empty means every port.
    pub ports: Vec<(u16, u16)>,
    /// Sorted, merged, inclusive.
    pub ranges: Vec<(u32, u32)>,
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
            .map(|r| {
                let mut nets: Vec<_> = r.ips.iter().filter_map(|s| parse_net(s)).collect();
                nets.sort_unstable();
                let mut ranges: Vec<(u32, u32)> = Vec::new();
                for n in nets {
                    match ranges.last_mut() {
                        Some(cur) if n.0 <= cur.1.saturating_add(1) => cur.1 = cur.1.max(n.1),
                        _ => ranges.push(n),
                    }
                }
                Compiled {
                    name: r.name.clone(),
                    ports: r.ports.iter().filter_map(|s| parse_port(s)).collect(),
                    ranges,
                }
            })
            .collect();
        Table { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Name of the first rule (file order) that matches the destination.
    pub fn match_udp(&self, dst: SocketAddrV4) -> Option<&str> {
        let ip = u32::from(*dst.ip());
        let port = dst.port();
        self.rules
            .iter()
            .find(|r| {
                (r.ports.is_empty() || r.ports.iter().any(|&(a, b)| (a..=b).contains(&port)))
                    && r.ranges.iter().any(|&(a, b)| (a..=b).contains(&ip))
            })
            .map(|r| r.name.as_str())
    }
}

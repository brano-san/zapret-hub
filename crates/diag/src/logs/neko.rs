//! NekoBox `neko.log`: where the proxy sent each connection.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Direct,
    Proxy,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    pub host: String,
    pub port: u16,
    pub route: Route,
    pub outbound_ms: Option<u32>,
    /// The client's local port (`inbound connection from 127.0.0.1:<port>`): the same as the
    /// observed flow's `local_port` of a program that went through the system proxy.
    pub from_port: Option<u16>,
}

/// `LEVEL[time] [id 166ms] rest` -> (id, duration in ms, rest).
fn split_id(line: &str) -> Option<(&str, Option<u32>, &str)> {
    let rest = &line[line.find("] [")? + 3..];
    let end = rest.find(']')?;
    let (id, dur) = rest[..end].split_once(' ')?;
    Some((id, go_ms(dur), rest[end + 1..].trim_start()))
}

/// A Go duration (`166ms`, `5.1s`, `1m5.2s`, `2h0m3s`, `900µs`) in whole ms.
fn go_ms(d: &str) -> Option<u32> {
    let mut ms = 0.0;
    let mut rest = d;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit() && c != '.')?;
        let n: f64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        ms += n * match &rest[..unit] {
            "h" => 3_600_000.0,
            "m" => 60_000.0,
            "s" => 1000.0,
            "ms" => 1.0,
            "us" | "µs" => 0.001,
            "ns" => 0.000_001,
            _ => return None,
        };
        rest = &rest[unit..];
    }
    Some(ms as u32)
}

fn classify(kind: &str, tag: &str) -> Route {
    match kind {
        "direct" => Route::Direct,
        "vless" | "vmess" | "trojan" | "shadowsocks" | "tuic" => Route::Proxy,
        k if k.starts_with("hysteria") => Route::Proxy,
        _ if tag == "proxy" => Route::Proxy,
        _ if tag == "bypass" => Route::Direct,
        k => Route::Other(format!("{k}[{tag}]")),
    }
}

/// Pairs each `inbound connection to host:port` with the outbound line of the same id.
pub fn parse(lines: &[String]) -> Vec<Conn> {
    let mut pending: HashMap<&str, (String, u16)> = HashMap::new();
    let mut from: HashMap<&str, u16> = HashMap::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<Conn> = Vec::new();
    for line in lines {
        let Some((id, ms, rest)) = split_id(line) else {
            continue;
        };
        if let Some((head, t)) = rest.split_once(": inbound connection from ") {
            if head.starts_with("inbound/")
                && let Some((_, p)) = t.trim().rsplit_once(':')
                && let Ok(p) = p.parse()
            {
                from.insert(id, p);
            }
        } else if let Some((head, t)) = rest.split_once(": inbound connection to ") {
            if head.starts_with("inbound/")
                && let Some((h, p)) = t.trim().rsplit_once(':')
                && let Ok(p) = p.parse()
            {
                pending.insert(id, (h.to_owned(), p));
            }
        } else if let Some((head, _)) = rest.split_once(": outbound connection to ") {
            let Some(inner) = head.strip_prefix("outbound/") else {
                continue;
            };
            let (kind, tag) = inner
                .split_once('[')
                .map_or((inner, ""), |(k, t)| (k, t.trim_end_matches(']')));
            if let Some(&i) = at.get(id) {
                out[i].outbound_ms = ms; // second line: time until connected
            } else if let Some((host, port)) = pending.remove(id) {
                at.insert(id, out.len());
                out.push(Conn {
                    host,
                    port,
                    route: classify(kind, tag),
                    outbound_ms: None,
                    from_port: from.get(id).copied(),
                });
            }
        }
    }
    out
}

/// More than half of the connections took over 2 s to connect (and there are enough to judge).
pub fn slow_dns(conns: &[Conn]) -> bool {
    conns.len() >= 5
        && conns
            .iter()
            .filter(|c| c.outbound_ms.is_some_and(|m| m > 2000))
            .count()
            * 2
            > conns.len()
}

pub(crate) fn matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

/// Route of the last connection to the domain or its subdomain.
pub fn route_of(conns: &[Conn], domain: &str) -> Option<Route> {
    conns
        .iter()
        .rev()
        .find(|c| matches(&c.host, domain))
        .map(|c| c.route.clone())
}

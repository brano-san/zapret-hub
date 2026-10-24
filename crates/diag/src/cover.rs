//! Which traffic a strategy's profiles match, by `--filter-*` (spec section 6).

use router::rules::Protocol;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cover {
    pub tcp: Vec<(u16, u16)>,
    pub udp: Vec<(u16, u16)>,
    pub l7: Vec<String>,
    /// Has `--hostlist*` or `--ipset*`.
    pub lists: bool,
}

/// `--name=value` and `--name value` (Flowseal's final args) as pairs; a flag has an empty value.
fn options(text: &str) -> Vec<(String, String)> {
    let tokens: Vec<String> = zapret::quoted_tokens(text)
        .into_iter()
        .map(|t| t.replace('"', ""))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        i += 1;
        if !token.starts_with("--") {
            continue;
        }
        if let Some((name, value)) = token.split_once('=') {
            out.push((name.to_owned(), value.to_owned()));
        } else if tokens.get(i).is_some_and(|next| !next.starts_with("--")) {
            out.push((token.clone(), tokens[i].clone()));
            i += 1;
        } else {
            out.push((token.clone(), String::new()));
        }
    }
    out
}

/// Ports and `a-b` ranges; the game filter placeholders are 1024-65535.
fn ranges(value: &str) -> Vec<(u16, u16)> {
    value
        .split(',')
        .filter_map(|part| {
            if part.starts_with("%GameFilter") {
                return Some((1024, 65535));
            }
            let (lo, hi) = part.split_once('-').unwrap_or((part, part));
            Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?))
        })
        .collect()
}

/// A profile's text (any options; only `--filter-*` and the lists count). No `--filter-tcp`
/// means it covers no TCP port, likewise UDP.
pub fn cover(profile_filter: &str) -> Cover {
    let mut c = Cover::default();
    for (name, value) in options(profile_filter) {
        match name.as_str() {
            "--filter-tcp" => c.tcp.extend(ranges(&value)),
            "--filter-udp" => c.udp.extend(ranges(&value)),
            "--filter-l7" => c.l7.extend(value.split(',').map(str::to_owned)),
            n if n.starts_with("--hostlist") || n.starts_with("--ipset") => c.lists = true,
            _ => {}
        }
    }
    c
}

pub fn covers(c: &Cover, protocol: Protocol, port: u16) -> bool {
    let hit = |ranges: &[(u16, u16)]| ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&port));
    match protocol {
        Protocol::Tcp => hit(&c.tcp),
        Protocol::Udp => hit(&c.udp),
        Protocol::Both => hit(&c.tcp) || hit(&c.udp),
    }
}

/// Some profile of `args` (Flowseal's final args or zapret2's `strategies::args`) covers it.
pub fn strategy_covers(args: &str, protocol: Protocol, port: u16) -> bool {
    zapret::quoted_tokens(args)
        .split(|t| t == "--new" || t.starts_with("--new="))
        .any(|profile| covers(&cover(&profile.join(" ")), protocol, port))
}

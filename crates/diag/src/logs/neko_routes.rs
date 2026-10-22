//! NekoBox `config/routes_box/<name>`: the rules that sent a domain direct or through the proxy.

use super::neko::{Route, matches};
use anyhow::Context;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Routes {
    pub direct: Vec<String>,
    pub proxy: Vec<String>,
    pub direct_dns: String,
    pub remote_dns: String,
}

pub fn read(path: &Path) -> anyhow::Result<Routes> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).context("parse routes")?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let list = |k: &str| {
        s(k).lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    };
    Ok(Routes {
        direct: list("direct_domain"),
        proxy: list("proxy_domain"),
        direct_dns: s("direct_dns"),
        remote_dns: s("remote_dns"),
    })
}

/// Does one NekoBox rule line cover the domain? `full:` exact; `keyword:` substring; `domain:`, plain
/// and leading-dot entries the domain and its subdomains; other prefixes (`regexp:`, `geosite:`, ...)
/// are not understood and never match. Case-insensitive.
fn rule_matches(rule: &str, domain: &str) -> bool {
    let (rule, domain) = (rule.to_lowercase(), domain.to_lowercase());
    match rule.split_once(':') {
        Some(("full", x)) => domain == x.trim(),
        Some(("keyword", x)) => !x.trim().is_empty() && domain.contains(x.trim()),
        Some(("domain", x)) => matches(&domain, x.trim().trim_start_matches('.')),
        Some(_) => false,
        None => matches(&domain, rule.trim_start_matches('.')),
    }
}

/// The rule line that applies to the domain; the direct list is checked first.
pub fn rule_for(r: &Routes, domain: &str) -> Option<(Route, String)> {
    let find = |l: &[String]| l.iter().find(|x| rule_matches(x, domain)).cloned();
    find(&r.direct)
        .map(|x| (Route::Direct, x))
        .or_else(|| find(&r.proxy).map(|x| (Route::Proxy, x)))
}

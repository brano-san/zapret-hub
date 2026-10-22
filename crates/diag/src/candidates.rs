//! zapret2 tricks to try on a culprit: each covering profile, narrowed to the group (spec 7).

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::cover::{cover, covers};
use crate::culprits::Group;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Candidate {
    /// The strategy's full name.
    pub source: String,
    /// Position among the strategy's profiles (`strategies::split`).
    pub profile: u32,
    /// The strategy cut down to this one profile.
    pub text: String,
}

/// `a.b.c.0/24`: a group by address; anything else is by domain.
pub(crate) fn is_subnet(key: &str) -> bool {
    key.strip_suffix("/24")
        .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
        .is_some_and(|ip| ip.octets()[3] == 0)
}

/// `strategies` are `(full name, text)`.
pub fn candidates(strategies: &[(String, String)], group: &Group) -> Vec<Candidate> {
    let Some(&port) = group.ports.first() else {
        return Vec::new();
    };
    let list = if is_subnet(&group.key) {
        format!("--ipset-ip={}", group.key)
    } else {
        format!("--hostlist-domains={}", group.key)
    };
    let mut out: Vec<Candidate> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (source, text) in strategies {
        let (global, profiles) = zapret2::strategies::split(&zapret2::strategies::args(text));
        for (i, profile) in profiles.iter().enumerate() {
            if !covers(&cover(profile), group.protocol, port) {
                continue;
            }
            let mut tokens = Vec::new();
            let mut at = None;
            for token in zapret::custom::quoted_tokens(profile) {
                if token.starts_with("--hostlist") || token.starts_with("--ipset") {
                    at.get_or_insert(tokens.len());
                } else {
                    tokens.push(token);
                }
            }
            // Where the first list was, else right after the filters.
            let at = at.unwrap_or_else(|| {
                tokens
                    .iter()
                    .rposition(|t| t.starts_with("--filter-"))
                    .map_or(0, |p| p + 1)
            });
            tokens.insert(at, list.clone());
            let profile_text = tokens.join(" ");
            // The whole text: the same profile on other blobs is another trick.
            let text = if global.is_empty() {
                profile_text
            } else {
                format!("{global} {profile_text}")
            };
            if seen.contains(&text) {
                continue;
            }
            seen.push(text.clone());
            out.push(Candidate {
                source: source.clone(),
                profile: i as u32,
                text,
            });
        }
    }
    out
}

//! zapret2 tricks to try on a culprit: each covering profile, narrowed to the group (spec 7).

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::cover::{cover, covers};
use crate::culprits::Group;
use crate::paths::PathKind;

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

/// What a candidate runs: its profile, and of the global part the `--wf-*` and `--lua-init` and the
/// blobs the profile names. Blobs only other profiles use do not make another trick.
fn effective(global: &str, profile: &str) -> String {
    let named = |name: &str| {
        profile.split("blob=").skip(1).any(|rest| {
            rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                == Some(name)
        })
    };
    let used: Vec<String> = zapret::custom::quoted_tokens(global)
        .into_iter()
        .filter(|t| {
            t.starts_with("--wf-")
                || t.starts_with("--lua-init")
                || t.strip_prefix("--blob=")
                    .and_then(|b| b.split_once(':'))
                    .is_some_and(|(name, _)| named(name))
        })
        .collect();
    format!("{} {profile}", used.join(" "))
}

/// `strategies` are `(full name, text)`. Also the left-out twins: `<label>: то же, что <source>`.
pub fn candidates(strategies: &[(String, String)], group: &Group) -> (Vec<Candidate>, Vec<String>) {
    let Some(&port) = group.ports.first() else {
        return (Vec::new(), Vec::new());
    };
    let list = if is_subnet(&group.key) {
        format!("--ipset-ip={}", group.key)
    } else {
        format!("--hostlist-domains={}", group.key)
    };
    let mut out: Vec<Candidate> = Vec::new();
    let mut twins = Vec::new();
    // (effective text, index into `out`)
    let mut seen: Vec<(String, usize)> = Vec::new();
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
            let key = effective(&global, &profile_text);
            let text = if global.is_empty() {
                profile_text
            } else {
                format!("{global} {profile_text}")
            };
            let candidate = Candidate {
                source: source.clone(),
                profile: i as u32,
                text,
            };
            // The same profile on other blobs is another trick; on unused ones it is not.
            if let Some((_, at)) = seen.iter().find(|(k, _)| *k == key) {
                let kept = &out[*at];
                twins.push(format!(
                    "{}: то же, что {}, профиль {}",
                    PathKind::Trick { candidate }.label(),
                    kept.source,
                    kept.profile + 1
                ));
                continue;
            }
            seen.push((key, out.len()));
            out.push(candidate);
        }
    }
    (out, twins)
}

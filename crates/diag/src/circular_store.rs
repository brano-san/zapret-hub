//! `data\diag\circular.toml`: the zapret2 tricks that worked, by group, and the `hub-circular`
//! strategy made of them (spec 7).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use zapret::custom::quoted_tokens;
use zapret2::circular::{merge, widen};
use zapret2::strategies::{args, split};

pub const NAME: &str = "hub-circular";

/// What `merge` takes at most.
const MAX_VARIANTS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Trick {
    /// The strategy's full name it came from.
    pub source: String,
    /// A candidate's text (`candidates::Candidate::text`): placeholders left as they are.
    pub text: String,
    /// When it was found.
    pub found: String,
    pub median_ms: Option<u32>,
    pub ok: u32,
    pub total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupTricks {
    /// The group's filter options: `--filter-udp=50000-50100 --filter-l7=discord`. Every trick's
    /// `--filter-tcp/udp/l7` is replaced by them, so `merge` sees one profile.
    pub filter: String,
    /// The culprit list every trick of the group carries: `--ipset-ip=<subnet>` or
    /// `--hostlist-domains=<domain>`. Part of the key: `merge` needs one list per profile.
    #[serde(default)]
    pub list: String,
    /// Best first.
    pub tricks: Vec<Trick>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub groups: Vec<GroupTricks>,
}

/// An empty store when there is no file.
pub fn load(path: &Path) -> anyhow::Result<Store> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| path.display().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(err) => Err(err).with_context(|| path.display().to_string()),
    }
}

/// Through a temporary file, so a crash never leaves half a store.
pub fn save(path: &Path, store: &Store) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string(store)?).with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())
}

/// The culprit list token of a trick's text, empty without one.
fn list_of(text: &str) -> String {
    quoted_tokens(text)
        .into_iter()
        .find(|t| t.starts_with("--ipset-ip=") || t.starts_with("--hostlist-domains="))
        .unwrap_or_default()
}

/// `found` into the group of `filter` and the tricks' culprit list: a trick with the same text takes the new measurements;
/// then most successes first, the faster first among equals, unmeasured last.
pub fn add(store: &mut Store, filter: &str, found: Vec<Trick>) {
    let list = found.first().map(|t| list_of(&t.text)).unwrap_or_default();
    let at = match store
        .groups
        .iter()
        .position(|g| g.filter == filter && g.list == list)
    {
        Some(at) => at,
        None => {
            store.groups.push(GroupTricks {
                filter: filter.to_owned(),
                list,
                tricks: Vec::new(),
            });
            store.groups.len() - 1
        }
    };
    let tricks = &mut store.groups[at].tricks;
    for trick in found {
        match tricks.iter_mut().find(|t| t.text == trick.text) {
            Some(old) => *old = trick,
            None => tricks.push(trick),
        }
    }
    tricks.sort_by_key(|t| (std::cmp::Reverse(t.ok), t.median_ms.unwrap_or(u32::MAX)));
}

pub(crate) fn is_filter(token: &str) -> bool {
    ["--filter-tcp=", "--filter-udp=", "--filter-l7="]
        .iter()
        .any(|prefix| token.starts_with(prefix))
}

/// `text` with its protocol filter replaced by `filter`, where the first one was.
fn refilter(text: &str, filter: &str) -> String {
    let mut tokens = quoted_tokens(&args(text));
    let at = tokens.iter().position(|t| is_filter(t));
    tokens.retain(|t| !is_filter(t));
    let at = at.unwrap_or(tokens.len()).min(tokens.len());
    tokens.splice(at..at, quoted_tokens(filter));
    tokens.join(" ")
}

/// The ports of `filter`'s `--filter-tcp=` or `--filter-udp=` (`protocol` is `tcp` or `udp`).
fn filter_ports(filter: &str, protocol: &str) -> Option<String> {
    let prefix = format!("--filter-{protocol}=");
    quoted_tokens(filter)
        .into_iter()
        .find_map(|t| t.replace('"', "").strip_prefix(&prefix).map(str::to_owned))
}

/// `token` from the global part of the group of `filter` into `globals`: a `--wf-` port list
/// the trick needs takes the group's own ports (not the trick strategy's whole list, nor its `~`
/// exclusion), widening the one `globals` has (winws2 keeps the last of each);
/// `--wf-raw-part`, `--lua-init` and `--blob` are added once; a blob already named stays as
/// `globals` has it. Other `--wf-` options stay as `globals` has them.
fn add_global(globals: &mut Vec<String>, token: String, filter: &str) {
    let bare = token.replace('"', "");
    let ports = [
        ("--wf-tcp-out=", "tcp"),
        ("--wf-tcp-in=", "tcp"),
        ("--wf-udp-out=", "udp"),
        ("--wf-udp-in=", "udp"),
    ]
    .into_iter()
    .find(|(prefix, _)| bare.starts_with(prefix));
    if let Some((prefix, protocol)) = ports {
        let Some(own) = filter_ports(filter, protocol) else {
            return;
        };
        match globals
            .iter()
            .rposition(|t| t.replace('"', "").starts_with(prefix))
        {
            Some(i) => globals[i] = widen(&globals[i].replace('"', ""), &own),
            None => globals.push(format!("{prefix}{own}")),
        }
        return;
    }
    if !["--wf-raw-part=", "--lua-init=", "--blob="]
        .iter()
        .any(|prefix| bare.starts_with(prefix))
    {
        return;
    }
    let blob = |t: &str| {
        t.replace('"', "")
            .strip_prefix("--blob=")
            .and_then(|rest| rest.split_once(':').map(|(name, _)| name.to_owned()))
    };
    let known = globals
        .iter()
        .any(|t| t.replace('"', "") == bare || (blob(t).is_some() && blob(t) == blob(&token)));
    if !known {
        globals.push(token);
    }
}

/// `current` (a zapret2 strategy's text, maybe empty) with a profile per group in front of its
/// own: a circular one over the group's best tricks, or its only trick as it is. Placeholders stay
/// for `strategies::tail`.
pub fn generate(current: &str, store: &Store) -> anyhow::Result<String> {
    let (global, profiles) = split(&args(current));
    let mut globals = quoted_tokens(&global);
    let mut front = Vec::new();
    for group in store.groups.iter().filter(|g| !g.tricks.is_empty()) {
        let text = match &group.tricks[..] {
            [only] => only.text.clone(),
            tricks => {
                let sources: Vec<(String, String)> = tricks
                    .iter()
                    .take(MAX_VARIANTS)
                    .map(|t| (t.source.clone(), refilter(&t.text, &group.filter)))
                    .collect();
                merge(&sources)
                    .with_context(|| format!("tricks for {}", group.filter))?
                    .text
            }
        };
        let (global, profiles) = split(&args(&text));
        for token in quoted_tokens(&global) {
            add_global(&mut globals, token, &group.filter);
        }
        front.extend(profiles);
    }
    let mut out = vec![format!(
        "# {NAME}: the tricks the diagnostics found, before the current strategy's profiles"
    )];
    out.extend(globals);
    let all: Vec<String> = front.into_iter().chain(profiles).collect();
    out.push(all.join("\n--new\n"));
    Ok(out.join("\n") + "\n")
}

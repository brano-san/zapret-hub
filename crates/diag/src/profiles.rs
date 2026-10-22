//! App profiles: built-in TOML (in the exe), overridable by `<dir>\<id>.toml`.

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    Https,
    DiscordIpDiscovery,
    Stun,
    Ping,
    Live,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct ProbeSpec {
    pub kind: ProbeKind,
    /// A host, or `"observed"` for addresses found by observation.
    pub target: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
#[serde(rename = "DiagProfile")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub apps: Vec<String>,
    pub hint: String,
    pub domains: Vec<String>,
    #[serde(rename = "probe", default)]
    pub probes: Vec<ProbeSpec>,
}

const BUILTIN: [&str; 3] = [
    include_str!("builtin/discord.toml"),
    include_str!("builtin/dbd.toml"),
    include_str!("builtin/youtube.toml"),
];

pub fn builtin() -> Vec<Profile> {
    BUILTIN
        .iter()
        .map(|s| toml::from_str(s).expect("built-in profile is valid"))
        .collect()
}

/// Built-ins, with `<dir>\<id>.toml` replacing the built-in of the same id and new ids appended.
pub fn load(dir: &Path) -> anyhow::Result<Vec<Profile>> {
    let mut all = builtin();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(all);
    };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    for path in files {
        let p: Profile = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|s| Ok(toml::from_str(&s)?))
            .with_context(|| format!("profile {}", path.display()))?;
        match all.iter_mut().find(|b| b.id == p.id) {
            Some(slot) => *slot = p,
            None => all.push(p),
        }
    }
    Ok(all)
}

/// The id of the "any program" profile.
pub const ANY: &str = "app";

impl Profile {
    /// The name of the proxy rule «Применить» adds: `diag-<id>`, for "any program"
    /// `diag-<exe stem>`, so each program gets its own. Characters a rule name refuses become
    /// `-`; 40 characters at most.
    pub fn rule_name(&self) -> String {
        let stem = match (self.id.as_str(), self.apps.first()) {
            (ANY, Some(exe)) => Path::new(exe)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| ANY.into()),
            _ => self.id.clone(),
        };
        let allowed = |c: char| {
            c.is_ascii_alphanumeric()
                || matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё' | ' ' | '-' | '_' | '.')
        };
        let name: String = format!("diag-{stem}")
            .chars()
            .map(|c| if allowed(c) { c } else { '-' })
            .take(40)
            .collect();
        name.trim_end().to_owned()
    }
}

/// The "any program" profile, built on the fly from an exe name.
pub fn any_program(exe: &str) -> Profile {
    let observed = |kind| ProbeSpec {
        kind,
        target: "observed".into(),
    };
    Profile {
        id: ANY.into(),
        name: exe.into(),
        apps: vec![exe.into()],
        hint: "Воспроизведите проблему в программе".into(),
        domains: vec![],
        probes: vec![observed(ProbeKind::Https), observed(ProbeKind::Live)],
    }
}

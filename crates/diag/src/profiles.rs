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
    /// TLS and a WebSocket upgrade to the voice server of the Discord log, else the gateway.
    DiscordWs,
    /// A 64 KiB exchange with a named TCP culprit: throughput, and the 16-20 KB freeze.
    HttpsBulk,
}

/// What a path is for: the culprit answering at all, or answering fast (games).
#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Goal {
    #[default]
    Reach,
    Latency,
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
    /// A request without a goal takes this one.
    #[serde(default)]
    pub goal: Goal,
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
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(all),
        Err(e) => return Err(e).with_context(|| format!("profiles {}", dir.display())),
    };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
        })
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
        goal: Goal::Reach,
    }
}

/// The `discord-ws` probe's host when the log names none.
pub const DISCORD_GATEWAY: &str = "gateway.discord.gg";

/// The newest voice WebSocket of Discord's `renderer_js.log`:
/// `[RTCControlSocket(default)] [CONNECT] wss://<host>:<port>/` → (host, port).
pub fn voice_endpoint(lines: &[String]) -> Option<(String, u16)> {
    lines.iter().rev().find_map(|l| {
        let (_, rest) = l.split_once("[RTCControlSocket(default)] [CONNECT] wss://")?;
        let authority = rest.split(['/', ' ', ',']).next()?;
        match authority.rsplit_once(':') {
            Some((host, port)) => Some((host.to_owned(), port.parse().ok()?)),
            None => Some((authority.to_owned(), 443)),
        }
        .filter(|(host, _)| !host.is_empty())
    })
}

/// A Discord WebSocket host: the log's voice server, any `*.discord.media`, the gateway.
pub fn discord_ws_host(name: &str, endpoint: Option<&str>) -> bool {
    let name = name.to_ascii_lowercase();
    Some(name.as_str()) == endpoint.map(str::to_ascii_lowercase).as_deref()
        || name.ends_with(".discord.media")
        || (name.starts_with("gateway") && name.ends_with(".discord.gg"))
}

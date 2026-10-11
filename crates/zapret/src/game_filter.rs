//! `utils\game_filter.enabled`, read like `:game_switch_status`, written like
//! `:save_game_filter_settings`.

use crate::batparse::GameFilterVars;
use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum GameFilterMode {
    Disabled,
    All,
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct GameFilter {
    pub mode: GameFilterMode,
    pub tcp: String,
    pub udp: String,
}

pub const DEFAULT_RANGE: &str = "1024-65535";

impl Default for GameFilter {
    fn default() -> Self {
        Self {
            mode: GameFilterMode::Disabled,
            tcp: DEFAULT_RANGE.into(),
            udp: DEFAULT_RANGE.into(),
        }
    }
}

impl GameFilter {
    /// The values `service.bat` substitutes; `12` stands for "off".
    pub fn vars(&self) -> GameFilterVars {
        let off = || "12".to_owned();
        match self.mode {
            GameFilterMode::All => GameFilterVars {
                all: self.tcp.clone(),
                tcp: self.tcp.clone(),
                udp: self.udp.clone(),
            },
            GameFilterMode::Tcp => GameFilterVars {
                all: self.tcp.clone(),
                tcp: self.tcp.clone(),
                udp: off(),
            },
            GameFilterMode::Udp => GameFilterVars {
                all: self.udp.clone(),
                tcp: off(),
                udp: self.udp.clone(),
            },
            GameFilterMode::Disabled => GameFilterVars::disabled(),
        }
    }
}

fn path(folder: &Folder) -> std::path::PathBuf {
    folder.utils().join("game_filter.enabled")
}

/// A missing file is the default (off), as in `service.bat`; any other read error is an error.
pub fn try_read(folder: &Folder) -> anyhow::Result<GameFilter> {
    let path = path(folder);
    match crate::read_text(&path) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(GameFilter::default()),
        Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
    }
}

/// For display, and for a restore, which writes the snapshot's value when this reads differently:
/// an unreadable file reads as the default.
pub fn read(folder: &Folder) -> GameFilter {
    try_read(folder).unwrap_or_default()
}

/// `for /f "usebackq tokens=1,* delims==" %%A in (file)` and the range checks after it.
pub fn parse(text: &str) -> GameFilter {
    let mut mode = String::from("disabled");
    let (mut tcp, mut udp) = (None, None);
    for line in text.lines() {
        let line = line.trim_start_matches('=');
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key, value.trim_start_matches('=')),
            None => (line, ""),
        };
        match (key.to_ascii_lowercase().as_str(), value.is_empty()) {
            ("mode", _) => mode = value.to_owned(),
            ("all", _) => mode = "all".into(),
            ("udp", true) => mode = "udp".into(),
            ("udp", false) => udp = Some(value.to_owned()),
            ("tcp", true) => mode = "tcp".into(),
            ("tcp", false) => tcp = Some(value.to_owned()),
            _ => {}
        }
    }
    let mode = match mode.to_ascii_lowercase().as_str() {
        "all" => GameFilterMode::All,
        "tcp" => GameFilterMode::Tcp,
        "udp" => GameFilterMode::Udp,
        _ => GameFilterMode::Disabled,
    };
    GameFilter {
        mode,
        tcp: tcp
            .as_deref()
            .and_then(validate_range)
            .unwrap_or_else(|| DEFAULT_RANGE.into()),
        udp: udp
            .as_deref()
            .and_then(validate_range)
            .unwrap_or_else(|| DEFAULT_RANGE.into()),
    }
}

/// `:validate_game_filter_range`: spaces dropped; every comma item is `N` or `N-M`,
/// at most 5 digits, no leading zero, up to 65535, `N <= M`.
pub fn validate_range(input: &str) -> Option<String> {
    let range: String = input.chars().filter(|c| *c != ' ').collect();
    if range.is_empty() {
        return None;
    }
    let number = |part: &str| {
        !part.is_empty()
            && part.len() <= 5
            && !part.starts_with('0')
            && part.bytes().all(|b| b.is_ascii_digit())
    };
    for item in range.split(',') {
        let (start, end) = item.split_once('-').unwrap_or((item, item));
        if !number(start) || !number(end) {
            return None;
        }
        let (start, end): (u32, u32) = (start.parse().ok()?, end.parse().ok()?);
        if start > 65535 || end > 65535 || start > end {
            return None;
        }
    }
    Some(range)
}

/// `:save_game_filter_settings`: `mode=`, `tcp=`, `udp=` with CRLF. "Off" is `mode=disabled`
/// and keeps the ranges, as `service.bat` does.
pub fn write(folder: &Folder, gf: &GameFilter) -> anyhow::Result<()> {
    let tcp = validate_range(&gf.tcp)
        .ok_or_else(|| anyhow::anyhow!("invalid TCP port range: {}", gf.tcp))?;
    let udp = validate_range(&gf.udp)
        .ok_or_else(|| anyhow::anyhow!("invalid UDP port range: {}", gf.udp))?;
    let mode = match gf.mode {
        GameFilterMode::Disabled => "disabled",
        GameFilterMode::All => "all",
        GameFilterMode::Tcp => "tcp",
        GameFilterMode::Udp => "udp",
    };
    std::fs::write(
        path(folder),
        format!("mode={mode}\r\ntcp={tcp}\r\nudp={udp}\r\n"),
    )?;
    log::info!("game filter: mode={mode} tcp={tcp} udp={udp}");
    Ok(())
}

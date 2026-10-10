//! `zapret-hub.toml` next to the exe (spec 4.3).

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context as _;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub zapret_dir: Option<String>,
    /// `127.0.0.1:2080`: checked on the overview, used when a download fails directly.
    pub socks5: Option<String>,
    pub autostart: bool,
    /// A logon start stays in the tray.
    pub start_minimized: bool,
    pub theme: Theme,
    pub wizard_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zapret_dir: None,
            socks5: None,
            autostart: false,
            start_minimized: true,
            theme: Theme::System,
            wizard_done: false,
        }
    }
}

pub fn load(path: &Path) -> anyhow::Result<Settings> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("{} is damaged", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Through a temporary file: a crash mid-write leaves the old settings, not half of the new.
pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, toml::to_string(settings)?)
        .with_context(|| format!("cannot write {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("cannot replace {}", path.display()))
}

/// A damaged file is set aside as `.broken` and the defaults are used: the app still starts, and
/// the second value says what happened.
pub fn load_or_default(path: &Path) -> (Settings, Option<String>) {
    let err = match load(path) {
        Ok(settings) => return (settings, None),
        Err(err) => err,
    };
    let broken = path.with_extension("toml.broken");
    let kept = match std::fs::rename(path, &broken) {
        Ok(()) => crate::texts::settings_kept(&broken),
        Err(rename) => format!("{}: {rename}", broken.display()),
    };
    (Settings::default(), Some(format!("{kept}\n\n{err:#}")))
}

/// A SOCKS5 address the way local clients listen: `IP:port`. Blank means none.
pub fn validate_socks(text: &str) -> anyhow::Result<Option<String>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let addr: SocketAddr = text
        .parse()
        .map_err(|_| anyhow::anyhow!("{text} is not an address like 127.0.0.1:2080"))?;
    Ok(Some(addr.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&dir.path().join("zapret-hub.toml")).unwrap(),
            Settings::default()
        );
        assert!(Settings::default().start_minimized);
    }

    #[test]
    fn settings_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        let settings = Settings {
            zapret_dir: Some(r"E:\Zapret".into()),
            socks5: Some("127.0.0.1:18082".into()),
            autostart: true,
            start_minimized: false,
            theme: Theme::Dark,
            wizard_done: true,
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("theme = \"dark\"")
        );
    }

    #[test]
    fn a_damaged_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        std::fs::write(&path, "zapret_dir = [").unwrap();
        let (settings, notice) = load_or_default(&path);
        assert_eq!(settings, Settings::default());
        assert!(notice.unwrap().contains("zapret-hub.toml.broken"));
        assert!(dir.path().join("zapret-hub.toml.broken").exists());
        assert!(!path.exists());
    }

    #[test]
    fn socks_addresses_are_an_ip_and_a_port() {
        assert_eq!(
            validate_socks(" 127.0.0.1:2080 ").unwrap().as_deref(),
            Some("127.0.0.1:2080")
        );
        assert_eq!(validate_socks("").unwrap(), None);
        assert!(validate_socks("localhost:2080").is_err());
        assert!(validate_socks("127.0.0.1").is_err());
    }
}

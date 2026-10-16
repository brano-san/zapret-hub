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
    /// The zapret2 folder (zapret2 spec 4); `None` while zapret2 is off.
    pub zapret2_dir: Option<String>,
    /// `127.0.0.1:2080`: checked on the overview, used when a download fails directly.
    pub socks5: Option<String>,
    pub autostart: bool,
    /// A logon start stays in the tray.
    pub start_minimized: bool,
    pub theme: Theme,
    pub wizard_done: bool,
    /// The pipe of `zapret-hub-cli` runs commands (CLI spec 5).
    pub cli_enabled: bool,
    /// Routing through the SOCKS5 proxy (router spec 3); only `set_router_enabled` writes it.
    pub router_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zapret_dir: None,
            zapret2_dir: None,
            socks5: None,
            autostart: false,
            start_minimized: true,
            theme: Theme::System,
            wizard_done: false,
            cli_enabled: false,
            router_enabled: false,
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

/// What a save leaves of `sent` against the settings in force. The window's goes as sent. One from
/// zapret-hub-cli (`pipe`) never turns the CLI on: a save that passed the gate just before the user
/// turned the CLI off must not turn it back on (CLI spec 7.2). Nor does it move the zapret or the
/// zapret2 folder: the SYSTEM service runs winws.exe or winws2.exe from it, so any program of the
/// user could pick what SYSTEM runs (CLI spec 5, zapret2 spec 10). Neither one changes
/// `router_enabled`: a Settings tab opened before the switch must not undo it (`routing::set_enabled`).
pub fn to_save(current: &Settings, mut sent: Settings, pipe: bool) -> anyhow::Result<Settings> {
    sent.router_enabled = current.router_enabled;
    if pipe {
        anyhow::ensure!(
            sent.zapret_dir == current.zapret_dir,
            "the zapret folder can be changed only in the Zapret Hub window (Settings)"
        );
        anyhow::ensure!(
            sent.zapret2_dir == current.zapret2_dir,
            "the zapret2 folder can be changed only in the Zapret Hub window (Settings)"
        );
        sent.cli_enabled &= current.cli_enabled;
    }
    Ok(sent)
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
        assert!(
            !Settings::default().cli_enabled,
            "off until the user turns it on"
        );
        assert!(!Settings::default().router_enabled);
    }

    #[test]
    fn a_settings_save_keeps_router_enabled() {
        let on = Settings {
            router_enabled: true,
            ..Settings::default()
        };
        let stale = Settings {
            theme: Theme::Dark,
            ..Settings::default()
        };
        for pipe in [false, true] {
            let saved = to_save(&on, stale.clone(), pipe).unwrap();
            assert!(
                saved.router_enabled,
                "only set_router_enabled changes it (pipe: {pipe})"
            );
            assert_eq!(saved.theme, Theme::Dark);
            assert!(
                !to_save(&Settings::default(), on.clone(), pipe)
                    .unwrap()
                    .router_enabled
            );
        }
    }

    #[test]
    fn settings_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        let settings = Settings {
            zapret_dir: Some(r"E:\Zapret".into()),
            zapret2_dir: Some(r"D:\zapret2".into()),
            socks5: Some("127.0.0.1:18082".into()),
            autostart: true,
            start_minimized: false,
            theme: Theme::Dark,
            wizard_done: true,
            cli_enabled: true,
            router_enabled: true,
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
    fn a_save_from_the_pipe_never_turns_the_cli_on() {
        let on = Settings {
            cli_enabled: true,
            ..Settings::default()
        };
        let off = Settings::default();
        let from_pipe = |current: &Settings, sent: Settings| to_save(current, sent, true).unwrap();
        assert!(
            !from_pipe(&off, on.clone()).cli_enabled,
            "the user turned it off while the save was on its way"
        );
        assert_eq!(from_pipe(&on, off.clone()), off, "it may turn itself off");
        let dark = Settings {
            theme: Theme::Dark,
            ..on.clone()
        };
        assert_eq!(from_pipe(&on, dark.clone()), dark, "the rest goes as sent");
        assert!(
            to_save(&off, on, false).unwrap().cli_enabled,
            "the window turns it on"
        );
    }

    #[test]
    fn only_the_window_moves_the_zapret_folder() {
        let current = Settings {
            zapret_dir: Some(r"E:\Zapret".into()),
            cli_enabled: true,
            ..Settings::default()
        };
        let in_dir = |dir: Option<&str>| Settings {
            zapret_dir: dir.map(str::to_owned),
            ..current.clone()
        };
        for moved in [in_dir(Some(r"C:\Users\me\winws")), in_dir(None)] {
            let refused = to_save(&current, moved.clone(), true).map_err(|err| err.to_string());
            assert_eq!(
                refused,
                Err(
                    "the zapret folder can be changed only in the Zapret Hub window (Settings)"
                        .into()
                ),
                "{moved:?} from the pipe"
            );
            assert_eq!(
                to_save(&current, moved.clone(), false).unwrap(),
                moved,
                "the window moves it"
            );
        }
        assert_eq!(
            to_save(&current, current.clone(), true).unwrap(),
            current,
            "the same folder goes through"
        );
    }

    #[test]
    fn only_the_window_moves_the_zapret2_folder() {
        let current = Settings {
            zapret2_dir: Some(r"D:\zapret2".into()),
            cli_enabled: true,
            ..Settings::default()
        };
        let in_dir = |dir: Option<&str>| Settings {
            zapret2_dir: dir.map(str::to_owned),
            ..current.clone()
        };
        for moved in [in_dir(Some(r"C:\Users\me\zapret2")), in_dir(None)] {
            let refused = to_save(&current, moved.clone(), true).map_err(|err| err.to_string());
            assert_eq!(
                refused,
                Err(
                    "the zapret2 folder can be changed only in the Zapret Hub window (Settings)"
                        .into()
                ),
                "{moved:?} from the pipe"
            );
            assert_eq!(
                to_save(&current, moved.clone(), false).unwrap(),
                moved,
                "the window moves it"
            );
        }
        assert_eq!(
            to_save(&current, current.clone(), true).unwrap(),
            current,
            "the same folder goes through"
        );
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

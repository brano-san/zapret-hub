//! `zapret-hub.toml`: next to the exe on Windows, in `~/.config/zapret-hub` on Linux (spec 4.3).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

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
    /// How long diagnostics watch the program, 10–120 s (diag spec 3).
    pub diag_observe_secs: u32,
    /// The time diagnostics have for the paths, 1–60 min (diag spec 6).
    pub diag_budget_min: u32,
    /// NekoBox's folder, for its logs (diag spec 5); `None`: the running NekoBox's.
    pub nekobox_dir: Option<String>,
    /// The game filter: the built-in strategies' `%GameFilter*%` ports open the unprivileged range
    /// (`zapret2::Engine::with_game_filter`).
    pub game_filter: bool,
    /// Ports whose traffic never reaches the engine (`51413`, `6881-6889`): Linux's nftables
    /// queue leaves them out (`zapret2::Engine::with_excluded_ports`); only `save_exclusions`
    /// writes them.
    pub excluded_ports: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zapret2_dir: None,
            socks5: None,
            autostart: false,
            start_minimized: true,
            theme: Theme::System,
            wizard_done: false,
            cli_enabled: false,
            router_enabled: false,
            diag_observe_secs: 30,
            diag_budget_min: 10,
            nekobox_dir: None,
            game_filter: true,
            excluded_ports: Vec::new(),
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
/// turned the CLI off must not turn it back on (CLI spec 7.2). Nor does it move the zapret2 folder:
/// the SYSTEM service runs winws2.exe from it, so any program of the user could pick what SYSTEM
/// runs (CLI spec 5, zapret2 spec 10). Neither one changes `router_enabled` or the excluded ports:
/// their own commands write them, and a Settings tab opened before must not undo them.
pub fn to_save(current: &Settings, mut sent: Settings, pipe: bool) -> anyhow::Result<Settings> {
    sent.router_enabled = current.router_enabled;
    sent.excluded_ports.clone_from(&current.excluded_ports);
    if pipe {
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

/// A setting diagnostics read changed: the folder, the game filter, the proxy, routing, NekoBox, their own.
pub fn diag_reads_changed(old: &Settings, new: &Settings) -> bool {
    old.zapret2_dir != new.zapret2_dir
        || old.socks5 != new.socks5
        || old.router_enabled != new.router_enabled
        || old.nekobox_dir != new.nekobox_dir
        || old.diag_observe_secs != new.diag_observe_secs
        || old.diag_budget_min != new.diag_budget_min
        || old.game_filter != new.game_filter
        || old.excluded_ports != new.excluded_ports
}

/// `zapret_dir` of a settings file 0.8.0 wrote: the Flowseal folder of a user who ran Flowseal.
/// `Settings` no longer has the key (serde skips it), so it is read from the file as it is.
pub fn flowseal_dir(path: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let dir = table.get("zapret_dir")?.as_str()?.trim();
    (!dir.is_empty()).then(|| PathBuf::from(dir))
}

/// The settings of a 0.8.0 Flowseal user on zapret2: the game filter as Flowseal had it (its flag
/// file `utils\game_filter.enabled`), and the wizard again when there is no zapret2 folder yet, so
/// that the user gets one.
pub fn migrate_flowseal(settings: &mut Settings, flowseal_dir: &Path) {
    settings.game_filter = flowseal_dir
        .join("utils")
        .join("game_filter.enabled")
        .is_file();
    if settings.zapret2_dir.is_none() {
        settings.wizard_done = false;
    }
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
    fn diagnostics_refuse_only_what_they_read() {
        let old = Settings::default();
        let looks = Settings {
            theme: Theme::Dark,
            autostart: true,
            start_minimized: true,
            ..old.clone()
        };
        assert!(!diag_reads_changed(&old, &looks));
        for read in [
            Settings {
                socks5: Some("127.0.0.1:2080".into()),
                ..old.clone()
            },
            Settings {
                zapret2_dir: Some(r"D:\zapret2".into()),
                ..old.clone()
            },
            Settings {
                diag_budget_min: old.diag_budget_min + 1,
                ..old.clone()
            },
            Settings {
                nekobox_dir: Some(r"E:\nekoray".into()),
                ..old.clone()
            },
        ] {
            assert!(diag_reads_changed(&old, &read), "{read:?}");
        }
    }

    #[test]
    fn settings_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        let settings = Settings {
            zapret2_dir: Some(r"D:\zapret2".into()),
            socks5: Some("127.0.0.1:18082".into()),
            autostart: true,
            start_minimized: false,
            theme: Theme::Dark,
            wizard_done: true,
            cli_enabled: true,
            router_enabled: true,
            diag_observe_secs: 45,
            diag_budget_min: 7,
            nekobox_dir: Some(r"E:\nekoray".into()),
            game_filter: false,
            excluded_ports: vec!["51413".into(), "6881-6889".into()],
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
        std::fs::write(&path, "zapret2_dir = [").unwrap();
        let (settings, notice) = load_or_default(&path);
        assert_eq!(settings, Settings::default());
        assert!(notice.unwrap().contains("zapret-hub.toml.broken"));
        assert!(dir.path().join("zapret-hub.toml.broken").exists());
        assert!(!path.exists());
    }

    #[test]
    fn a_settings_save_keeps_what_its_own_commands_write() {
        let current = Settings {
            excluded_ports: vec!["51413".into()],
            ..Settings::default()
        };
        let stale = Settings {
            theme: Theme::Dark,
            ..Settings::default()
        };
        for pipe in [false, true] {
            let saved = to_save(&current, stale.clone(), pipe).unwrap();
            assert_eq!(
                saved,
                Settings {
                    theme: Theme::Dark,
                    ..current.clone()
                },
                "pipe: {pipe}"
            );
        }
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

    /// `zapret-hub.toml` as 0.8.0 wrote it for a user of Flowseal and zapret2.
    const SETTINGS_0_8_0: &str = r#"zapret_dir = 'E:\Zapret'
zapret2_dir = 'D:\zapret2-dl\zapret2-v1.0.5.2'
socks5 = "127.0.0.1:18082"
autostart = true
start_minimized = true
theme = "system"
wizard_done = true
cli_enabled = true
router_enabled = true
diag_observe_secs = 30
diag_budget_min = 10
"#;

    #[test]
    fn a_settings_file_of_0_8_0_loads_and_names_its_flowseal_folder_until_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        std::fs::write(&path, SETTINGS_0_8_0).unwrap();
        let settings = load(&path).unwrap();
        assert_eq!(
            settings.zapret2_dir.as_deref(),
            Some(r"D:\zapret2-dl\zapret2-v1.0.5.2")
        );
        assert_eq!(settings.socks5.as_deref(), Some("127.0.0.1:18082"));
        assert!(settings.wizard_done && settings.cli_enabled && settings.router_enabled);
        assert!(settings.game_filter, "a new field takes its default");
        assert!(settings.excluded_ports.is_empty());
        assert_eq!(flowseal_dir(&path), Some(PathBuf::from(r"E:\Zapret")));
        save(&path, &settings).unwrap();
        assert_eq!(flowseal_dir(&path), None, "the migration runs once");
        assert_eq!(load(&path).unwrap(), settings);
        assert_eq!(flowseal_dir(&dir.path().join("none.toml")), None);
    }

    #[test]
    fn a_flowseal_user_keeps_the_game_filter_and_gets_the_wizard_without_zapret2() {
        let flowseal = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            wizard_done: true,
            zapret2_dir: Some(r"D:\zapret2".into()),
            ..Settings::default()
        };
        migrate_flowseal(&mut settings, flowseal.path());
        assert!(!settings.game_filter, "Flowseal had it off");
        assert!(settings.wizard_done, "zapret2 is there already");

        let utils = flowseal.path().join("utils");
        std::fs::create_dir_all(&utils).unwrap();
        std::fs::write(utils.join("game_filter.enabled"), "mode=all\r\n").unwrap();
        let mut alone = Settings {
            wizard_done: true,
            game_filter: false,
            ..Settings::default()
        };
        migrate_flowseal(&mut alone, flowseal.path());
        assert!(alone.game_filter);
        assert!(!alone.wizard_done, "the wizard gets zapret2");
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

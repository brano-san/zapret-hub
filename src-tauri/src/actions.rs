//! What commands, the tray and background jobs share.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Manager as _};
use tauri_specta::Event as _;
use zapret::custom::CustomStrategy;
use zapret::folder::Folder;
use zapret::service::{QuickCheck, active};
use zapret::user_lists::UserList;

use crate::dto::{
    Candidates, Detected, FlowsealState, FolderCheck, FolderKind, ListState, Notice, NoticeLevel,
    NoticesChanged, Overview, StateChanged,
};
use crate::settings::Settings;
use crate::state::AppState;
use crate::texts;

pub fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// Something to show even with the window closed: it waits in the queue (spec 10).
pub fn notice(app: &AppHandle, level: NoticeLevel, title: &str, text: String) {
    log::info!("notice: {title}: {text}");
    state(app).push_notice(Notice {
        level,
        title: title.to_owned(),
        text,
    });
    if let Err(err) = NoticesChanged.emit(app) {
        log::warn!("cannot announce a notice: {err}");
    }
}

/// zapret or the settings changed: the page reads them again, the tray redraws.
pub fn changed(app: &AppHandle) {
    if let Err(err) = StateChanged.emit(app) {
        log::warn!("cannot announce a change: {err}");
    }
    crate::tray::refresh(app);
}

/// A change that ends in a quick check: the check feeds the tray and the overview.
fn finish_with_check(
    app: &AppHandle,
    result: anyhow::Result<QuickCheck>,
) -> anyhow::Result<QuickCheck> {
    if let Ok(check) = &result {
        state(app).set_quick(check.clone());
    }
    changed(app);
    result
}

/// «Применить» (spec 5.4). A `zapret2:` name goes to zapret2 (zapret2 spec 3.1, 6); its folder
/// is opened first, so a broken one changes nothing. zapret2 needs no Flowseal folder. A switch to
/// the other engine pauses routing: it comes back with the new engine's DLL (router spec 3.1, 7).
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    if crate::routing::switches_engine(name, active().0) {
        return crate::routing::paused(app, || apply_now(app, name));
    }
    apply_now(app, name)
}

fn apply_now(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let state = state(app);
    let result = match zapret2::strip_prefix(name) {
        Some(name) => state.engine2_required().and_then(|engine| {
            // The check runs first: a failure changes nothing, no snapshot, no restart (spec 6).
            let (strategy, tail) = state.change_any(|folder| {
                zapret2::service::prepare(&engine, folder, engine.root(), name)
            })?;
            state.change_any_safely("apply", |folder| {
                zapret2::service::install_checked(&engine, folder, &strategy, &tail)
            })
        }),
        None => state.change_safely("apply", |folder| zapret::service::apply(folder, name)),
    };
    finish_with_check(app, result)
}

/// «Применить лучшую»: the first of the newest standard run (spec 7.4).
pub fn apply_best(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let best = state(app)
        .best(1)
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no test results yet: run the standard tests first"))?;
    apply(app, &best)
}

pub fn restart(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let result = state(app).change_any(|folder| {
        zapret::service::restart(active().0)?;
        // winws needs a moment before it intercepts anything (spec 5.4).
        std::thread::sleep(Duration::from_secs(2));
        Ok(zapret::service::quick_check(folder))
    });
    finish_with_check(app, result)
}

/// Reads only, but in the queue: a restart or an apply that is half done would fail it for nothing,
/// and during a test run (`change` refuses it) it would check the strategy under test. A refusal
/// changes nothing, so it is not announced.
pub fn quick_check(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let check = state(app).change_any(|folder| Ok(zapret::service::quick_check(folder)))?;
    finish_with_check(app, Ok(check))
}

pub fn check_flowseal_update(app: &AppHandle) -> anyhow::Result<Option<String>> {
    let state = state(app);
    let newer = zapret::updates::newer_version(&state.folder()?, state.socks5().as_deref())?;
    state.set_update(newer.clone());
    changed(app);
    Ok(newer)
}

/// One save at a time, from reading the settings in force to setting the new ones: otherwise the
/// file, the memory and the scheduled task could disagree. A save from the `pipe` never turns the
/// CLI on and never moves the zapret or zapret2 folder (`settings::to_save`, CLI spec 5, 7.2,
/// zapret2 spec 10).
pub fn save_settings(app: &AppHandle, settings: Settings, pipe: bool) -> anyhow::Result<()> {
    let state = state(app);
    let _saving = state.saving();
    let old = state.settings();
    let mut settings = crate::settings::to_save(&old, settings, pipe)?;
    settings.socks5 =
        crate::settings::validate_socks(settings.socks5.as_deref().unwrap_or_default())?;
    let folder_changed = settings.zapret_dir != old.zapret_dir;
    let engine_changed = settings.zapret2_dir != old.zapret2_dir;
    if folder_changed || engine_changed {
        anyhow::ensure!(
            !state.testing(),
            "a test run is going: wait for it or cancel it"
        );
    }
    if folder_changed && let Some(dir) = &settings.zapret_dir {
        Folder::open(dir)?;
    }
    if engine_changed && let Some(dir) = &settings.zapret2_dir {
        zapret2::Engine::open(dir)?;
    }
    if settings.autostart != old.autostart {
        if settings.autostart {
            crate::autostart::enable(&std::env::current_exe()?)?;
        } else {
            crate::autostart::disable()?;
        }
    }
    crate::settings::save(&state.paths.settings(), &settings)?;
    state.set_settings(settings);
    if engine_changed {
        state.migrate_strategies();
    }
    if folder_changed {
        prepare_folder(app);
    }
    // The proxy or a folder, and with it the DLL, may have changed.
    crate::routing::sync(app);
    changed(app);
    Ok(())
}

/// The active strategy's arguments are baked into the service: saving it installs it again.
pub fn save_custom(
    app: &AppHandle,
    strategy: CustomStrategy,
    create: bool,
) -> anyhow::Result<Option<QuickCheck>> {
    let state = state(app);
    let store = state.store();
    let active = zapret::service::strategy();
    if !create && active.is_some_and(|active| active.eq_ignore_ascii_case(&strategy.file_stem())) {
        let result = state.change_safely("custom", |folder| {
            let saved = zapret::custom::save(folder, &store, &strategy, create)?;
            zapret::service::apply(folder, &saved.file_stem())
        });
        return finish_with_check(app, result).map(Some);
    }
    let result = state.change(|folder| zapret::custom::save(folder, &store, &strategy, create));
    changed(app);
    result.map(|_| None)
}

/// `save_zapret2` (zapret2 spec 5.2). The active strategy's command line is baked into the
/// service: saving it installs it again, under a snapshot.
pub fn save_zapret2(
    app: &AppHandle,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<Option<QuickCheck>> {
    let state = state(app);
    let engine = state.engine2_required()?;
    let data = engine.root().to_path_buf();
    let full = format!("{}{}", zapret2::PREFIX, name.trim());
    let active = zapret::service::strategy();
    if !create && active.is_some_and(|active| active.eq_ignore_ascii_case(&full)) {
        // The check runs first: a failure changes nothing (spec 6).
        let (claimed, tail) = state.change_any(|folder| {
            zapret2::service::check_save(&engine, folder, &data, name, text, false)
        })?;
        let saved = zapret2::strategies::Strategy {
            name: claimed,
            text: text.to_owned(),
            builtin: false,
            flowseal_copy: false,
        };
        let result = state.change_any_safely("custom", |folder| {
            zapret2::strategies::write(&data, &saved.name, text)?;
            zapret2::service::install(&engine, &saved, &tail)?;
            // winws2 needs a moment before it intercepts anything (spec 5.4).
            std::thread::sleep(Duration::from_secs(2));
            Ok(zapret::service::quick_check(folder))
        });
        return finish_with_check(app, result).map(Some);
    }
    let result = state
        .change_any(|folder| zapret2::service::save(&engine, folder, &data, name, text, create));
    changed(app);
    result.map(|_| None)
}

/// A folder found at start or chosen in Settings: the user lists exist, and the custom strategies'
/// files are back after a Flowseal update (spec 5.5, 6).
pub fn prepare_folder(app: &AppHandle) {
    let state = state(app);
    let Ok(folder) = state.folder() else { return };
    if let Err(err) = zapret::user_lists::ensure_defaults(&folder) {
        notice(
            app,
            NoticeLevel::Error,
            texts::PREPARE_FAILED,
            format!("{err:#}"),
        );
    }
    match zapret::custom::materialize_all(&folder, &state.store()) {
        Ok(restored) if !restored.is_empty() => notice(
            app,
            NoticeLevel::Info,
            texts::FILES_RESTORED,
            restored.join(", "),
        ),
        Ok(_) => {}
        Err(err) => notice(
            app,
            NoticeLevel::Error,
            texts::PREPARE_FAILED,
            format!("{err:#}"),
        ),
    }
}

pub fn overview(app: &AppHandle) -> Overview {
    let state = state(app);
    let folder = state.folder_if_set();
    let settings = state.settings();
    let (service, image) = active();
    Overview {
        folder_error: folder_error(&folder, settings.zapret2_dir.is_some()),
        folder: settings.zapret_dir,
        version: folder.ok().flatten().as_ref().and_then(Folder::version),
        update: state.update(),
        service_name: service.to_owned(),
        service: zapret::service::state(service),
        windivert: zapret::service::state("WinDivert"),
        winws_running: zapret::procs::is_running(image),
        strategy: zapret::service::strategy(),
        zapret2: zapret2::info(settings.zapret2_dir.as_deref()),
        quick: state.quick(),
        last_run: state.last_run(),
        testing: state.testing(),
    }
}

/// A Flowseal folder that does not open, or none at all while zapret2 is off too: zapret2 alone
/// needs no Flowseal folder.
fn folder_error(folder: &anyhow::Result<Option<Folder>>, zapret2: bool) -> Option<String> {
    match folder {
        Err(err) => Some(format!("{err:#}")),
        Ok(None) if !zapret2 => Some(zapret::folder::NOT_SET.to_owned()),
        Ok(_) => None,
    }
}

pub fn flowseal(app: &AppHandle) -> anyhow::Result<FlowsealState> {
    let folder = state(app).folder()?;
    let lists = UserList::ALL
        .iter()
        .map(|&list| {
            Ok(ListState {
                list,
                entries: zapret::user_lists::read(&folder, list)?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(FlowsealState {
        game_filter: zapret::game_filter::read(&folder),
        ipset: zapret::ipset::mode(&folder),
        ipset_backup: zapret::ipset::has_backup(&folder),
        check_updates: zapret::flags::check_updates(&folder),
        fakes: zapret::fakes::state(&folder)?,
        lists,
    })
}

pub fn check_folder(path: &Path) -> FolderCheck {
    FolderCheck {
        path: path.display().to_string(),
        missing: zapret::folder::missing_parts(path)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        version: Folder::open(path).ok().and_then(|folder| folder.version()),
    }
}

/// Where each engine's folder is, from the service, running processes and the cheap places a
/// release is unpacked to (about a second at most).
pub fn detect_folders() -> Detected {
    let mut dirs = zapret::folder::candidates();
    dirs.extend(zapret::folder::nearby_dirs(
        std::time::Duration::from_millis(800),
    ));
    Detected {
        flowseal: zapret::folder::find_flowseal(&dirs).map(|path| check_folder(&path)),
        zapret2: zapret2::find_zapret2(&dirs).map(|path| path.display().to_string()),
    }
}

/// Folders where Flowseal probably is, and SOCKS5 ports that answer now: NekoBox's own port when
/// it runs, then the default ports of known clients (spec 9.2).
pub fn candidates() -> Candidates {
    let folders = zapret::folder::candidates()
        .iter()
        .map(|path| check_folder(path))
        .collect();
    let mut ports: Vec<u16> = ["nekobox.exe", "nekoray.exe"]
        .iter()
        .flat_map(|image| zapret::procs::find(image))
        .filter_map(|proc| {
            proc.exe
                .as_deref()
                .and_then(Path::parent)
                .and_then(probe::socks::nekobox_port)
        })
        .collect();
    ports.extend(probe::socks::KNOWN_PORTS);
    Candidates {
        folders,
        proxies: probe::socks::listening(&ports),
    }
}

/// Release pages and the issues `service.bat` links to: https only.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        url.starts_with("https://"),
        "only https links are opened: {url}"
    );
    crate::shell::open(OsStr::new(url))
}

/// The folder of the user's zapret2 strategies, created when missing.
fn strategies_folder(home: &Path) -> anyhow::Result<PathBuf> {
    let dir = zapret2::strategies::dir(home);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The folder of a strategy's file: Flowseal's `.bat` (its own and the custom ones) in the
/// Flowseal folder, a zapret2 strategy of the user's in the zapret2 folder's `strategies`. Only names that are in
/// the lists resolve, so the page cannot point anywhere else.
fn strategy_folder(
    folder: Option<&Folder>,
    home: Option<&Path>,
    name: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(name) = zapret2::strip_prefix(name) {
        let home = home.ok_or_else(|| {
            anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings")
        })?;
        let strategy = zapret2::strategies::find(home, name)?;
        anyhow::ensure!(
            !strategy.builtin,
            "{} is built into Zapret Hub: it has no file",
            strategy.full_name()
        );
        return Ok(zapret2::strategies::dir(home));
    }
    let folder = folder.ok_or_else(|| anyhow::anyhow!(zapret::folder::NOT_SET))?;
    let file = zapret::strategies::find(folder, name)?;
    Ok(file.path.parent().unwrap_or(folder.root()).to_path_buf())
}

pub fn open_folder(app: &AppHandle, kind: FolderKind) -> anyhow::Result<()> {
    let state = state(app);
    let path = match kind {
        FolderKind::Zapret => state.folder()?.root().to_path_buf(),
        FolderKind::Zapret2 => strategies_folder(state.engine2_required()?.root())?,
        FolderKind::Logs => {
            let logs = state.paths.logs();
            std::fs::create_dir_all(&logs)?;
            logs
        }
        FolderKind::Strategy(name) => {
            // A zapret2 name needs its engine; a Flowseal one does not.
            let home = match state.engine2() {
                Ok(Some(engine)) => Some(engine.root().to_path_buf()),
                Ok(None) => None,
                Err(err) if zapret2::strip_prefix(&name).is_some() => return Err(err),
                Err(_) => None,
            };
            strategy_folder(state.folder_if_set()?.as_ref(), home.as_deref(), &name)?
        }
    };
    crate::shell::open(path.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overview_wants_a_flowseal_folder_only_without_zapret2() {
        assert_eq!(
            folder_error(&Ok(None), false).as_deref(),
            Some(zapret::folder::NOT_SET)
        );
        assert_eq!(folder_error(&Ok(None), true), None);
        let broken = Err(anyhow::anyhow!(r"E:\Zapret is not a zapret folder"));
        assert!(
            folder_error(&broken, true)
                .unwrap()
                .contains("not a zapret folder")
        );
    }

    #[test]
    fn a_strategy_opens_the_folder_of_its_file() {
        let zapret = tempfile::tempdir().unwrap();
        let root = zapret.path();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lists")).unwrap();
        for file in [
            "service.bat",
            "general (ALT11).bat",
            r"bin\winws.exe",
            r"bin\WinDivert64.sys",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let folder = Folder::open(root).unwrap();
        let data = tempfile::tempdir().unwrap();
        zapret2::strategies::write(data.path(), "mine", "--wf-tcp-out=443").unwrap();

        let open = |folder: Option<&Folder>, name: &str| {
            strategy_folder(folder, Some(data.path()), name).map_err(|err| format!("{err:#}"))
        };
        assert_eq!(
            open(Some(&folder), "GENERAL (alt11)"),
            Ok(root.to_path_buf())
        );
        assert_eq!(
            open(None, "zapret2:mine"),
            Ok(data.path().join("strategies")),
            "zapret2 needs no Flowseal folder"
        );
        assert!(
            open(Some(&folder), "zapret2:general")
                .unwrap_err()
                .contains("no file")
        );
        assert!(open(Some(&folder), "zapret2:other").is_err());
        assert!(
            open(Some(&folder), r"..\..\Windows").is_err(),
            "a path is not a name"
        );
        assert!(
            open(None, "general (ALT11)")
                .unwrap_err()
                .contains("zapret folder is not set")
        );
    }

    #[test]
    fn the_strategies_folder_is_in_the_zapret2_folder_and_is_created() {
        let data = tempfile::tempdir().unwrap();
        let dir = strategies_folder(data.path()).unwrap();
        assert_eq!(dir, data.path().join("strategies"));
        assert!(dir.is_dir());
    }
}

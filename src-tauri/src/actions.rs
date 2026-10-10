//! What commands, the tray and background jobs share.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tauri::{AppHandle, Manager as _};
use tauri_specta::Event as _;
use zapret::custom::CustomStrategy;
use zapret::folder::Folder;
use zapret::service::{QuickCheck, SERVICE};
use zapret::user_lists::UserList;

use crate::dto::{
    Candidates, FlowsealState, FolderCheck, FolderKind, ListState, Notice, NoticeLevel,
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

/// «Применить» (spec 5.4).
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let result = state(app).change_safely("apply", |folder| zapret::service::apply(folder, name));
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
    let result = state(app).change(|folder| {
        zapret::service::restart(SERVICE)?;
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
    let check = state(app).change(|folder| Ok(zapret::service::quick_check(folder)))?;
    finish_with_check(app, Ok(check))
}

pub fn check_flowseal_update(app: &AppHandle) -> anyhow::Result<Option<String>> {
    let state = state(app);
    let newer = zapret::updates::newer_version(&state.folder()?, state.socks5().as_deref())?;
    state.set_update(newer.clone());
    changed(app);
    Ok(newer)
}

pub fn save_settings(app: &AppHandle, mut settings: Settings) -> anyhow::Result<()> {
    let state = state(app);
    let old = state.settings();
    settings.socks5 =
        crate::settings::validate_socks(settings.socks5.as_deref().unwrap_or_default())?;
    let folder_changed = settings.zapret_dir != old.zapret_dir;
    if folder_changed {
        anyhow::ensure!(
            !state.testing(),
            "a test run is going: wait for it or cancel it"
        );
        if let Some(dir) = &settings.zapret_dir {
            Folder::open(dir)?;
        }
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
    if folder_changed {
        prepare_folder(app);
    }
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
    let folder = state.folder();
    Overview {
        folder: state.settings().zapret_dir,
        folder_error: folder.as_ref().err().map(|err| format!("{err:#}")),
        version: folder.as_ref().ok().and_then(Folder::version),
        update: state.update(),
        service: zapret::service::state(SERVICE),
        windivert: zapret::service::state("WinDivert"),
        winws_running: zapret::procs::is_running("winws.exe"),
        strategy: zapret::service::strategy(),
        quick: state.quick(),
        last_run: state.last_run(),
        testing: state.testing(),
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

/// Through explorer: it hands the path or the link to the user's own, unelevated shell.
fn explorer(target: &OsStr) -> anyhow::Result<()> {
    let mut cmd = Command::new("explorer.exe");
    cmd.arg(target);
    // explorer exits with 1 even when it opened the target: the code means nothing.
    exec::run(&mut cmd, Duration::from_secs(10), None)?;
    Ok(())
}

/// Release pages and the issues `service.bat` links to: https only.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        url.starts_with("https://"),
        "only https links are opened: {url}"
    );
    explorer(OsStr::new(url))
}

pub fn open_folder(app: &AppHandle, kind: FolderKind) -> anyhow::Result<()> {
    let state = state(app);
    let path = match kind {
        FolderKind::Zapret => state.folder()?.root().to_path_buf(),
        FolderKind::Logs => {
            let logs = state.paths.logs();
            std::fs::create_dir_all(&logs)?;
            logs
        }
    };
    explorer(path.as_os_str())
}

//! What commands, the tray and background jobs share.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;

use tauri::{AppHandle, Manager as _};
use tauri_specta::Event as _;
use zapret::service::{QuickCheck, SERVICE, SERVICE2, SvcState, active};

use crate::dto::{
    Candidates, Detected, Exclusions, FolderKind, Notice, NoticeLevel, NoticesChanged, Overview,
    StateChanged,
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

/// «Применить» (spec 5.4) of a `zapret2:` name (zapret2 spec 3.1, 6): the folder is opened and
/// the strategy checked first, so a broken one changes nothing.
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let state = state(app);
    let result = state.engine2_required().and_then(|engine| {
        let name = zapret2::strip_prefix(name)
            .ok_or_else(|| anyhow::anyhow!("no zapret2 strategy named {name}"))?;
        // The check runs first: a failure changes nothing, no snapshot, no restart (spec 6).
        let (strategy, tail) =
            state.change(|| zapret2::service::prepare(&engine, engine.root(), name))?;
        state.change_safely("apply", || {
            zapret2::service::install_checked(&engine, &strategy, &tail)
        })
    });
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
    let result = state(app).change(|| {
        zapret::service::restart(active().0)?;
        // winws2 needs a moment before it intercepts anything (spec 5.4).
        std::thread::sleep(Duration::from_secs(2));
        Ok(zapret::service::quick_check())
    });
    finish_with_check(app, result)
}

/// Reads only, but in the queue: a restart or an apply that is half done would fail it for nothing,
/// and during a test run (`change` refuses it) it would check the strategy under test. A refusal
/// changes nothing, so it is not announced.
pub fn quick_check(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let check = state(app).change(|| Ok(zapret::service::quick_check()))?;
    finish_with_check(app, Ok(check))
}

/// What the move off Flowseal does to the services, from what is there.
#[derive(Debug, PartialEq, Eq)]
enum MoveStep {
    Nothing,
    /// Only Flowseal's service: it gives way to the zapret2 copy of its strategy.
    Switch,
    /// The zapret2 service with this strategy: 0.8.0 baked the Flowseal folder's lists and fakes
    /// into its arguments, so it is installed again to read the zapret2 folder. A Flowseal service
    /// next to it stays, for the conflict check: the user's zapret2 strategy is not replaced.
    Reinstall(String),
}

fn move_step(
    zapret2_ready: bool,
    flowseal_installed: bool,
    zapret2_active: Option<String>,
) -> MoveStep {
    match zapret2_active {
        _ if !zapret2_ready => MoveStep::Nothing,
        Some(active) => MoveStep::Reinstall(active),
        None if flowseal_installed => MoveStep::Switch,
        None => MoveStep::Nothing,
    }
}

/// The move off Flowseal of a 0.8.0 user (its folder was in the settings; `settings::migrate_flowseal`
/// already took the rest), once, at start: its lists and fakes into the zapret2 folder, and the
/// services as `move_step` says. Without zapret2 the wizard gets it and the system checks name the
/// Flowseal service a conflict. One notice (and its journal line) says what happened.
pub fn migrate_flowseal(app: &AppHandle, flowseal_dir: &Path) {
    let state = state(app);
    let engine = state.engine2().ok().flatten();
    if let Some(engine) = &engine {
        let copied = zapret2::strategies::adopt_files(engine, flowseal_dir);
        if !copied.is_empty() {
            log::info!(
                "copied from {} into the zapret2 folder: {}",
                flowseal_dir.display(),
                copied.join(", ")
            );
        }
    }
    let installed = zapret::service::state(SERVICE) != SvcState::NotInstalled;
    let old = installed
        .then(|| zapret::service::strategy_of(SERVICE))
        .flatten();
    let target = zapret2::strategies::flowseal_copy(old.as_deref().unwrap_or("general"));
    let zapret2_active = (zapret::service::state(SERVICE2) != SvcState::NotInstalled)
        .then(|| zapret::service::strategy_of(SERVICE2))
        .flatten();
    let step = move_step(engine.is_some(), installed, zapret2_active);
    let result = |r: anyhow::Result<QuickCheck>| r.map(|_| ()).map_err(|err| format!("{err:#}"));
    let outcome = match (&step, &engine) {
        (MoveStep::Switch, Some(engine)) => Some(result(crate::routing::paused(app, || {
            leave_flowseal(app, engine, &target)
        }))),
        (MoveStep::Reinstall(active), _) => {
            Some(result(crate::routing::paused(app, || apply(app, active))))
        }
        _ => None,
    };
    let level = match &outcome {
        Some(Err(_)) => NoticeLevel::Error,
        _ => NoticeLevel::Info,
    };
    let moved = match (&step, &outcome) {
        (MoveStep::Switch, Some(r)) => texts::Moved::Switched(r.as_ref().map(|_| ())),
        (MoveStep::Reinstall(active), Some(r)) => {
            texts::Moved::Reinstalled(active, r.as_ref().map(|_| ()))
        }
        _ => texts::Moved::NotTried,
    };
    let text = texts::flowseal_moved(old.as_deref(), &target, engine.is_some(), installed, moved);
    notice(app, level, texts::FLOWSEAL_MOVED, text);
}

/// Flowseal's service gives way to `target` on zapret2: the strategy is checked while nothing has
/// changed, then `zapret` goes and `zapret2` comes. When that fails, the snapshot puts zapret2
/// back, and Flowseal's service is made again as it was.
fn leave_flowseal(
    app: &AppHandle,
    engine: &zapret2::Engine,
    target: &str,
) -> anyhow::Result<QuickCheck> {
    let state = state(app);
    let name = zapret2::strip_prefix(target).unwrap_or(target);
    let (strategy, tail) =
        state.change(|| zapret2::service::prepare(engine, engine.root(), name))?;
    let flowseal = zapret::service::config(SERVICE);
    let flowseal_strategy = zapret::service::strategy_of(SERVICE);
    let result = state.change_safely("apply", || {
        zapret::service::stop_and_delete(SERVICE)?;
        if let Err(err) = zapret::procs::kill_all("winws.exe") {
            log::warn!(target: "exec", "winws.exe was not ended: {err:#}");
        }
        zapret2::service::install_checked(engine, &strategy, &tail)
    });
    if result.is_err()
        && let Some(config) = flowseal
        && zapret::service::state(SERVICE) == SvcState::NotInstalled
    {
        let back = zapret::service::create(SERVICE, &config.image_path, config.auto_start)
            .and_then(|()| match &flowseal_strategy {
                Some(name) => zapret::service::set_strategy_of(SERVICE, name),
                None => Ok(()),
            })
            .and_then(|()| zapret::service::start(SERVICE));
        if let Err(err) = back {
            log::error!("the Flowseal service did not come back: {err:#}");
        }
    }
    finish_with_check(app, result)
}

/// One save at a time, from reading the settings in force to setting the new ones: otherwise the
/// file, the memory and the scheduled task could disagree. A save from the `pipe` never turns the
/// CLI on and never moves the zapret2 folder (`settings::to_save`, CLI spec 5, 7.2, zapret2 spec
/// 10). A new game filter installs the active strategy again: its ports are in the service.
pub fn save_settings(app: &AppHandle, settings: Settings, pipe: bool) -> anyhow::Result<()> {
    let state = state(app);
    let _saving = state.saving();
    let old = state.settings();
    let mut settings = crate::settings::to_save(&old, settings, pipe)?;
    settings.socks5 =
        crate::settings::validate_socks(settings.socks5.as_deref().unwrap_or_default())?;
    // Diagnostics read these settings and put the state back as they found it; the theme and
    // the like save as usual.
    let diagnosing = state.diagnosing();
    if diagnosing && crate::settings::diag_reads_changed(&old, &settings) {
        state.not_diagnosing()?;
    }
    anyhow::ensure!(
        (10..=120).contains(&settings.diag_observe_secs),
        "diagnostics watch for 10 to 120 s, not {}",
        settings.diag_observe_secs
    );
    anyhow::ensure!(
        (1..=60).contains(&settings.diag_budget_min),
        "diagnostics take 1 to 60 min, not {}",
        settings.diag_budget_min
    );
    let engine_changed = settings.zapret2_dir != old.zapret2_dir;
    let filter_changed = settings.game_filter != old.game_filter;
    if engine_changed || filter_changed {
        anyhow::ensure!(
            !state.testing(),
            "a test run is going: wait for it or cancel it"
        );
    }
    if engine_changed && let Some(dir) = &settings.zapret2_dir {
        zapret2::Engine::open(dir)?;
    }
    // Linux: against the entry on the disk, which the desktop's settings may have changed.
    #[cfg(not(windows))]
    let old = Settings {
        autostart: crate::autostart::is_enabled(&crate::exe()?),
        ..old
    };
    if settings.autostart != old.autostart {
        if settings.autostart {
            crate::autostart::enable(&crate::exe()?)?;
        } else {
            crate::autostart::disable()?;
        }
    }
    crate::settings::save(&state.paths.settings(), &settings)?;
    state.set_settings(settings);
    if engine_changed {
        state.migrate_strategies();
    }
    // The proxy or the folder, and with it the DLL, may have changed; not while diagnostics drive
    // the router (and then neither did).
    if !diagnosing {
        crate::routing::sync(app);
    }
    let reinstall = filter_changed && zapret::service::state(SERVICE2) != SvcState::NotInstalled;
    if let Some(active) = zapret::service::strategy().filter(|_| reinstall) {
        apply(app, &active)?;
    }
    changed(app);
    Ok(())
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
        let (claimed, tail) =
            state.change(|| zapret2::service::check_save(&engine, &data, name, text, false))?;
        let saved = zapret2::strategies::Strategy {
            name: claimed,
            text: text.to_owned(),
            builtin: false,
            flowseal_copy: false,
        };
        let result = state.change_safely("custom", || {
            zapret2::strategies::write(&data, &saved.name, text)?;
            zapret2::service::install(&engine, &saved, &tail)?;
            // winws2 needs a moment before it intercepts anything (spec 5.4).
            std::thread::sleep(Duration::from_secs(2));
            Ok(zapret::service::quick_check())
        });
        return finish_with_check(app, result).map(Some);
    }
    let result = state.change(|| zapret2::service::save(&engine, &data, name, text, create));
    changed(app);
    result.map(|_| None)
}

pub fn overview(app: &AppHandle) -> Overview {
    let state = state(app);
    let settings = state.settings();
    let (service, image) = active();
    Overview {
        service_name: service.to_owned(),
        service: zapret::service::state(service),
        windivert: zapret::service::state("WinDivert"),
        winws_running: zapret::procs::is_running(image),
        strategy: zapret::service::strategy(),
        zapret2: zapret2::info(settings.zapret2_dir.as_deref()),
        quick: state.quick(),
        last_run: state.last_run(),
        testing: state.testing(),
        platform: crate::dto::Platform::CURRENT,
    }
}

/// Where zapret2 is, from running processes and the cheap places a release is unpacked to (about
/// a second at most).
pub fn detect_folders() -> Detected {
    let mut dirs: Vec<PathBuf> = zapret::procs::find(zapret::service::WINWS2)
        .into_iter()
        .filter_map(|p| {
            p.exe
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .flat_map(|dir| {
            // `<root>\binaries\windows-x86_64\winws2.exe`, `<root>\winws2.exe`, `<root>\bin\winws2.exe`.
            dir.ancestors()
                .take(3)
                .map(Path::to_path_buf)
                .collect::<Vec<_>>()
        })
        .collect();
    dirs.extend(nearby_dirs(Duration::from_millis(800)));
    Detected {
        zapret2: zapret2::find_zapret2(&dirs).map(|path| path.display().to_string()),
    }
}

/// The cheap places a release is unpacked to, each with its direct subfolders and nothing
/// deeper: next to the app, Downloads, Desktop, Documents, then Program Files and drive roots
/// (Windows) or the home, the data folder, `/opt` and `/usr/local/share` (Linux).
/// Stops listing when `budget` is spent; the order is the order of preference.
fn nearby_dirs(budget: Duration) -> Vec<PathBuf> {
    let deadline = std::time::Instant::now() + budget;
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(app) = crate::exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        roots.extend(app.parent().map(Path::to_path_buf));
        roots.push(app);
    }
    let home = var(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if let Some(home) = &home {
        roots.extend(["Downloads", "Desktop", "Documents"].map(|name| home.join(name)));
    }
    if cfg!(windows) {
        roots.extend(
            ["ProgramFiles", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|name| var(name)),
        );
        roots.extend(
            ('A'..='Z')
                .map(|drive| PathBuf::from(format!("{drive}:\\")))
                .filter(|root| root.is_dir()),
        );
    } else {
        roots.extend(home);
        roots.push(zapret::release::default_parent());
        roots.extend(["/opt", "/usr/local/share"].map(PathBuf::from));
    }
    expand(&roots, deadline)
}

/// Each root, then its subfolders; a root that cannot be listed counts for itself only.
fn expand(roots: &[PathBuf], deadline: std::time::Instant) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for root in roots {
        if std::time::Instant::now() > deadline {
            break;
        }
        if !root.is_dir() || dirs.contains(root) {
            continue;
        }
        dirs.push(root.clone());
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if std::time::Instant::now() > deadline {
                break;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(entry.path());
            }
        }
    }
    dirs
}

/// SOCKS5 ports that answer now, of the default ports of common clients (spec 9.2).
pub fn candidates() -> Candidates {
    Candidates {
        proxies: probe::socks::listening(&probe::socks::KNOWN_PORTS),
    }
}

/// Release pages and the issues the page links to: https only.
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

/// The folder of a zapret2 strategy of the user's: the zapret2 folder's `strategies`. Only names
/// that are in the list resolve, so the page cannot point anywhere else.
fn strategy_folder(home: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let name = zapret2::strip_prefix(name)
        .ok_or_else(|| anyhow::anyhow!("no zapret2 strategy named {name}"))?;
    let strategy = zapret2::strategies::find(home, name)?;
    anyhow::ensure!(
        !strategy.builtin,
        "{} is built into Zapret Hub: it has no file",
        strategy.full_name()
    );
    Ok(zapret2::strategies::dir(home))
}

pub fn open_folder(app: &AppHandle, kind: FolderKind) -> anyhow::Result<()> {
    let state = state(app);
    let path = match kind {
        FolderKind::Zapret2 => strategies_folder(state.engine2_required()?.root())?,
        FolderKind::Logs => {
            let logs = state.paths.logs();
            std::fs::create_dir_all(&logs)?;
            logs
        }
        FolderKind::Strategy(name) => strategy_folder(state.engine2_required()?.root(), &name)?,
    };
    crate::shell::open(path.as_os_str())
}

/// The zapret2 exclude lists the built-in strategies read (`--hostlist-exclude`,
/// `--ipset-exclude`).
const EXCLUDE_DOMAINS: &str = "list-exclude-user.txt";
const EXCLUDE_IPS: &str = "ipset-exclude-user.txt";

/// One entry per line; blank lines and `#` comments are not entries.
fn is_entry(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with('#')
}

fn read_list(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| is_entry(line))
        .map(|line| line.trim().to_owned())
        .collect()
}

/// The user's list file `old` with `entries` as its entries: blank lines, comments and kept
/// entries stay where they are (with the file's line ends), removed entries go, new ones are
/// appended. `None` when the file already has exactly these entries: nothing to write.
fn merge_list(old: &str, entries: &[String]) -> Option<String> {
    let wanted: Vec<&str> = entries.iter().map(|entry| entry.trim()).collect();
    let current: Vec<&str> = old.lines().filter(|l| is_entry(l)).map(str::trim).collect();
    if current == wanted {
        return None;
    }
    let eol = if old.contains("\r\n") { "\r\n" } else { "\n" };
    let mut text = String::new();
    let kept = old
        .lines()
        .filter(|line| !is_entry(line) || wanted.contains(&line.trim()));
    let added = wanted
        .iter()
        .copied()
        .filter(|entry| !current.contains(entry));
    for line in kept.chain(added) {
        text.push_str(line);
        text.push_str(eol);
    }
    Some(text)
}

/// An entry is one line of text the engine reads as one entry: not empty, no comment.
fn check_entries(list: &str, entries: &[String]) -> anyhow::Result<()> {
    for entry in entries {
        anyhow::ensure!(
            !entry.trim().is_empty()
                && !entry.contains('#')
                && !entry.chars().any(|c| c.is_control() || c.is_whitespace()),
            "{entry:?} is not an entry of {list}: one domain or address, no spaces or #"
        );
    }
    Ok(())
}

/// The exclusions card: the two lists and the settings' ports.
pub fn exclusions(app: &AppHandle) -> anyhow::Result<Exclusions> {
    let state = state(app);
    let lists = zapret2::strategies::lists_dir(&state.engine2_required()?);
    Ok(Exclusions {
        domains: read_list(&lists.join(EXCLUDE_DOMAINS)),
        ips: read_list(&lists.join(EXCLUDE_IPS)),
        ports: state.settings().excluded_ports,
    })
}

/// The lists merged into their files (comments stay, an unchanged file is not written), the
/// ports into the settings, then the active
/// zapret2 strategy installed again: the engine reads the lists at its start, and Linux's
/// nftables queue takes the ports.
pub fn save_exclusions(app: &AppHandle, exclusions: Exclusions) -> anyhow::Result<()> {
    let state = state(app);
    let engine = state.engine2_required()?;
    check_entries(EXCLUDE_DOMAINS, &exclusions.domains)?;
    check_entries(EXCLUDE_IPS, &exclusions.ips)?;
    let ports: Vec<String> = exclusions
        .ports
        .iter()
        .map(|port| port.trim().to_owned())
        .collect();
    // Refuses a bad port before anything is written.
    engine.clone().with_excluded_ports(&ports)?;
    anyhow::ensure!(!state.testing(), crate::state::TESTING);
    // Diagnostics read the ports and the lists (`diag_reads_changed`): nothing is written.
    state.not_diagnosing()?;
    let mut any = write_lists(&zapret2::strategies::lists_dir(&engine), &exclusions)?;
    {
        let _saving = state.saving();
        let mut settings = state.settings();
        if settings.excluded_ports != ports {
            settings.excluded_ports = ports;
            crate::settings::save(&state.paths.settings(), &settings)?;
            state.set_settings(settings);
            any = true;
        }
    }
    let installed = zapret::service::state(SERVICE2) != SvcState::NotInstalled;
    if let Some(active) = zapret::service::strategy().filter(|_| any && installed) {
        apply(app, &active)?;
    }
    changed(app);
    Ok(())
}

/// Both exclusion lists merged into their files in `lists`; whether either file changed.
fn write_lists(lists: &Path, exclusions: &Exclusions) -> anyhow::Result<bool> {
    std::fs::create_dir_all(lists).with_context(|| lists.display().to_string())?;
    let mut any = false;
    for (name, entries) in [
        (EXCLUDE_DOMAINS, &exclusions.domains),
        (EXCLUDE_IPS, &exclusions.ips),
    ] {
        let path = lists.join(name);
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let Some(text) = merge_list(&old, entries) else {
            continue;
        };
        let temp = path.with_extension("txt.tmp");
        std::fs::write(&temp, text).with_context(|| format!("cannot write {}", temp.display()))?;
        std::fs::rename(&temp, &path)
            .with_context(|| format!("cannot replace {}", path.display()))?;
        any = true;
    }
    Ok(any)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_its_entries_without_blanks_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.txt");
        std::fs::write(&path, "# mine\nexample.com\n\n  vk.com  \n").unwrap();
        assert_eq!(read_list(&path), ["example.com", "vk.com"]);
        assert!(read_list(&dir.path().join("none.txt")).is_empty());
        let ok = ["example.com".to_owned(), "10.0.0.0/8".to_owned()];
        assert!(check_entries("x", &ok).is_ok());
        for bad in ["", "a b", "a#b", "a\nb"] {
            assert!(check_entries("x", &[bad.to_owned()]).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_saved_list_keeps_the_users_comments_and_an_unchanged_one_is_not_written() {
        let old = "# work\r\nexample.com\r\n\r\n# games\r\n  vk.com  \r\nold.org\r\n";
        let entries = |list: &[&str]| list.iter().map(|e| (*e).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            merge_list(old, &entries(&["example.com", "vk.com", "old.org"])),
            None
        );
        assert_eq!(
            merge_list(old, &entries(&["vk.com", "example.com", "new.net"])).as_deref(),
            Some("# work\r\nexample.com\r\n\r\n# games\r\n  vk.com  \r\nnew.net\r\n")
        );
        assert_eq!(
            merge_list("", &entries(&["a.com"])).as_deref(),
            Some("a.com\n")
        );
        assert_eq!(merge_list("# only\n", &[]), None);
    }

    #[test]
    fn saving_the_same_exclusions_again_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let lists = dir.path().join("lists");
        let mut exclusions = Exclusions {
            domains: vec!["a.com".into()],
            ips: Vec::new(),
            ports: Vec::new(),
        };
        assert!(write_lists(&lists, &exclusions).unwrap());
        assert!(
            !write_lists(&lists, &exclusions).unwrap(),
            "nothing to reinstall"
        );
        exclusions.ips.push("10.0.0.0/8".into());
        assert!(write_lists(&lists, &exclusions).unwrap());
        assert_eq!(
            std::fs::read_to_string(lists.join(EXCLUDE_IPS)).unwrap(),
            "10.0.0.0/8\n"
        );
    }

    #[test]
    fn the_move_off_flowseal_keeps_a_zapret2_strategy_and_switches_only_flowseal() {
        let active = || Some("zapret2:mine".to_owned());
        assert_eq!(
            move_step(false, true, active()),
            MoveStep::Nothing,
            "no zapret2 folder"
        );
        assert_eq!(
            move_step(true, false, active()),
            MoveStep::Reinstall("zapret2:mine".into())
        );
        assert_eq!(
            move_step(true, true, active()),
            MoveStep::Reinstall("zapret2:mine".into()),
            "both services: the user's zapret2 strategy stays"
        );
        assert_eq!(move_step(true, true, None), MoveStep::Switch);
        assert_eq!(move_step(true, false, None), MoveStep::Nothing);
    }

    #[test]
    fn a_strategy_opens_the_folder_of_its_file() {
        let data = tempfile::tempdir().unwrap();
        zapret2::strategies::write(data.path(), "mine", "--wf-tcp-out=443").unwrap();
        let open =
            |name: &str| strategy_folder(data.path(), name).map_err(|err| format!("{err:#}"));
        assert_eq!(open("zapret2:mine"), Ok(data.path().join("strategies")));
        assert!(open("zapret2:general").unwrap_err().contains("no file"));
        assert!(open("zapret2:other").is_err());
        assert!(open(r"..\..\Windows").is_err(), "a path is not a name");
        assert!(open("general (ALT11)").is_err(), "Flowseal's are gone");
    }

    #[test]
    fn the_strategies_folder_is_in_the_zapret2_folder_and_is_created() {
        let data = tempfile::tempdir().unwrap();
        let dir = strategies_folder(data.path()).unwrap();
        assert_eq!(dir, data.path().join("strategies"));
        assert!(dir.is_dir());
    }

    #[test]
    fn a_subfolder_of_a_root_is_a_candidate_but_not_deeper() {
        let tmp = tempfile::tempdir().unwrap();
        let near = tmp.path().join("zapret2-v1.0");
        let deep = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&near).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        let dirs = expand(
            &[tmp.path().to_path_buf()],
            std::time::Instant::now() + Duration::from_secs(5),
        );
        assert!(dirs.contains(&near) && !dirs.contains(&deep));
        assert!(
            expand(
                &[tmp.path().to_path_buf()],
                std::time::Instant::now() - Duration::from_secs(1)
            )
            .is_empty(),
            "a spent budget lists nothing more"
        );
    }
}

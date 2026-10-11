//! IPC commands: argument plumbing only. The work is in `actions`, `jobs` and the crates.

use std::path::Path;

use probe::socks::ProxyStatus;
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_dialog::DialogExt as _;
use tester::catalog::{StrategyDetail, StrategyRow};
use tester::history::RunSummary;
use tester::model::TestRun;
use tester::run::Request;
use zapret::checks::{CheckAction, SystemCheck};
use zapret::custom::CustomStrategy;
use zapret::discord_cache::CacheReport;
use zapret::fakes::FakeKind;
use zapret::game_filter::GameFilter;
use zapret::hosts::HostsPlan;
use zapret::ipset::IpsetMode;
use zapret::service::{QuickCheck, active};
use zapret::user_lists::UserList;

use crate::actions::{self, state as app};
use crate::dto::{Candidates, FlowsealState, FolderCheck, FolderKind, LiveRun, Notice, Overview};
use crate::settings::Settings;
use crate::state::AppState;
use crate::{texts, window};

type Answer<T> = Result<T, String>;

/// Every command touches processes, the disk or the network: it runs off the async runtime, and
/// its error reaches the page with the whole chain of causes.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Answer<T> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            log::warn!("command failed: {err:#}");
            Err(format!("{err:#}"))
        }
        Err(err) => Err(format!("the command did not finish: {err}")),
    }
}

fn data_tests(handle: &AppHandle) -> std::path::PathBuf {
    tester::history::dir(&app(handle).paths.data())
}

#[tauri::command]
#[specta::specta]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
#[specta::specta]
pub async fn save_settings(handle: AppHandle, settings: Settings) -> Answer<()> {
    blocking(move || actions::save_settings(&handle, settings, false)).await
}

/// `save_settings` for zapret-hub-cli (`cli.rs`), not a window command: it never turns the CLI on
/// and never moves the zapret or zapret2 folder (`settings::to_save`).
pub async fn save_settings_from_pipe(handle: AppHandle, settings: Settings) -> Answer<()> {
    blocking(move || actions::save_settings(&handle, settings, true)).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_overview(handle: AppHandle) -> Answer<Overview> {
    blocking(move || Ok(actions::overview(&handle))).await
}

#[tauri::command]
#[specta::specta]
pub async fn check_proxy(addr: String) -> Answer<ProxyStatus> {
    blocking(move || Ok(probe::socks::check(&addr))).await
}

#[tauri::command]
#[specta::specta]
pub async fn wizard_candidates() -> Answer<Candidates> {
    blocking(|| Ok(actions::candidates())).await
}

#[tauri::command]
#[specta::specta]
pub async fn check_folder(path: String) -> Answer<FolderCheck> {
    blocking(move || Ok(actions::check_folder(Path::new(&path)))).await
}

#[tauri::command]
#[specta::specta]
pub async fn pick_folder(handle: AppHandle) -> Answer<Option<String>> {
    blocking(move || {
        let mut dialog = handle.dialog().file().set_title(texts::PICK_FOLDER);
        if let Some(window) = handle.get_webview_window(window::MAIN) {
            dialog = dialog.set_parent(&window);
        }
        let picked = dialog
            .blocking_pick_folder()
            .map(|path| path.into_path())
            .transpose()?;
        Ok(picked.map(|path| path.display().to_string()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn open_url(url: String) -> Answer<()> {
    blocking(move || actions::open_url(&url)).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_folder(handle: AppHandle, kind: FolderKind) -> Answer<()> {
    blocking(move || actions::open_folder(&handle, kind)).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_strategies(handle: AppHandle) -> Answer<Vec<StrategyRow>> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        tester::catalog::rows(
            &state.folder()?,
            state.engine2().ok().flatten().as_ref(),
            &state.store(),
            &state.paths.data(),
            active.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_strategy(handle: AppHandle, name: String) -> Answer<StrategyDetail> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        tester::catalog::detail(
            &state.folder()?,
            state.engine2().ok().flatten().as_ref(),
            &state.store(),
            &state.paths.data(),
            active.as_deref(),
            &name,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_strategy(handle: AppHandle, name: String) -> Answer<QuickCheck> {
    blocking(move || actions::apply(&handle, &name)).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_best(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::apply_best(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn restart_zapret(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::restart(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn clone_strategy(
    handle: AppHandle,
    source: String,
    name: String,
) -> Answer<CustomStrategy> {
    blocking(move || {
        let state = app(&handle);
        let cloned = state
            .change(|folder| zapret::custom::clone_from(folder, &state.store(), &source, &name));
        actions::changed(&handle);
        cloned
    })
    .await
}

/// The editor's «Проверить»: the parser and `winws --dry-run`, nothing saved.
#[tauri::command]
#[specta::specta]
pub async fn validate_custom(handle: AppHandle, strategy: CustomStrategy) -> Answer<()> {
    blocking(move || {
        app(&handle).change(|folder| zapret::custom::validate(folder, &strategy).map(|_| ()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn save_custom(
    handle: AppHandle,
    strategy: CustomStrategy,
    create: bool,
) -> Answer<Option<QuickCheck>> {
    blocking(move || actions::save_custom(&handle, strategy, create)).await
}

/// `name` is the record's name, not the file's.
#[tauri::command]
#[specta::specta]
pub async fn delete_custom(handle: AppHandle, name: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        let deleted = state.change(|folder| {
            zapret::custom::delete(folder, &state.store(), &name, active.as_deref())
        });
        actions::changed(&handle);
        deleted
    })
    .await
}

/// The zapret2 folder, its version and Lua compatibility, or why it cannot be used.
#[tauri::command]
#[specta::specta]
pub async fn zapret2_info(handle: AppHandle) -> Answer<zapret2::Info> {
    blocking(move || {
        Ok(zapret2::info(
            app(&handle).settings().zapret2_dir.as_deref(),
        ))
    })
    .await
}

/// The zapret2 editor's «Проверить»: the whole check of zapret2 spec 5.3, nothing saved.
#[tauri::command]
#[specta::specta]
pub async fn validate_zapret2(handle: AppHandle, text: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let engine = state.engine2_required()?;
        state.change(|folder| {
            let tail = zapret2::strategies::tail(&engine, Some(folder), &text)?;
            zapret2::service::validate(&engine, &tail)
        })
    })
    .await
}

/// `name` without the prefix: `mine` for `zapret2:mine`.
#[tauri::command]
#[specta::specta]
pub async fn save_zapret2(
    handle: AppHandle,
    name: String,
    text: String,
    create: bool,
) -> Answer<Option<QuickCheck>> {
    blocking(move || actions::save_zapret2(&handle, &name, &text, create)).await
}

/// `name` without the prefix: `mine` for `zapret2:mine`.
#[tauri::command]
#[specta::specta]
pub async fn delete_zapret2(handle: AppHandle, name: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        let deleted = state
            .change(|_| zapret2::strategies::delete(&state.paths.data(), &name, active.as_deref()));
        actions::changed(&handle);
        deleted
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn start_tests(handle: AppHandle, request: Request) -> Answer<()> {
    blocking(move || crate::jobs::start_tests(&handle, request)).await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_tests(state: State<'_, AppState>) {
    state.cancel_tests();
}

#[tauri::command]
#[specta::specta]
pub fn test_live(state: State<'_, AppState>) -> Option<LiveRun> {
    state.live()
}

#[tauri::command]
#[specta::specta]
pub async fn list_runs(handle: AppHandle) -> Answer<Vec<RunSummary>> {
    blocking(move || tester::history::summaries(&data_tests(&handle))).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_run(handle: AppHandle, id: String) -> Answer<TestRun> {
    blocking(move || tester::history::load(&data_tests(&handle), &id)).await
}

/// «Копировать»: the run as `test zapret.ps1` writes it.
#[tauri::command]
#[specta::specta]
pub async fn run_report(handle: AppHandle, id: String) -> Answer<String> {
    blocking(move || {
        Ok(tester::report::to_text(&tester::history::load(
            &data_tests(&handle),
            &id,
        )?))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_flowseal(handle: AppHandle) -> Answer<FlowsealState> {
    blocking(move || actions::flowseal(&handle)).await
}

/// The ports are baked into the active service's arguments: it is installed again (spec 5.5).
#[tauri::command]
#[specta::specta]
pub async fn set_game_filter(handle: AppHandle, filter: GameFilter) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let engine2 = state.engine2().ok().flatten();
        let done = state.change_safely("game_filter", |folder| {
            zapret::game_filter::write(folder, &filter)?;
            zapret::service::reinstall_active(folder)?;
            zapret2::service::reinstall_active(engine2.as_ref(), folder, &state.paths.data())
        });
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_ipset_mode(handle: AppHandle, mode: IpsetMode) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change_safely("ipset", |folder| {
            zapret::ipset::set_mode(folder, mode)?;
            zapret::service::restart_if_running(active().0)
        });
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_check_updates(handle: AppHandle, enabled: bool) -> Answer<()> {
    blocking(move || {
        let done =
            app(&handle).change(|folder| Ok(zapret::flags::set_check_updates(folder, enabled)?));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn replace_fake(handle: AppHandle, kind: FakeKind, source: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| {
            zapret::fakes::replace(folder, kind, &source)?;
            zapret::service::restart_if_running(active().0)
        });
        actions::changed(&handle);
        done
    })
    .await
}

/// winws reads the lists again by itself: no restart.
#[tauri::command]
#[specta::specta]
pub async fn add_list_entry(handle: AppHandle, list: UserList, entry: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| zapret::user_lists::add(folder, list, &entry));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_list_entry(handle: AppHandle, list: UserList, entry: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| zapret::user_lists::remove(folder, list, &entry));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn update_ipset(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        let done = state.change(|folder| zapret::updates::update_ipset(folder, socks5.as_deref()));
        actions::changed(&handle);
        done
    })
    .await
}

/// What applying would change in hosts; nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn hosts_plan(handle: AppHandle) -> Answer<HostsPlan> {
    blocking(move || zapret::updates::hosts_plan(app(&handle).socks5().as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_hosts(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        state.change(|_| zapret::updates::apply_hosts(socks5.as_deref()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn check_flowseal_update(handle: AppHandle) -> Answer<Option<String>> {
    blocking(move || actions::check_flowseal_update(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn system_checks(handle: AppHandle) -> Answer<Vec<SystemCheck>> {
    blocking(move || Ok(zapret::checks::run_all(&app(&handle).folder()?))).await
}

#[tauri::command]
#[specta::specta]
pub async fn fix_check(handle: AppHandle, action: CheckAction) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|_| zapret::checks::fix(action));
        actions::changed(&handle);
        done
    })
    .await
}

/// Not a change to zapret: no queue.
#[tauri::command]
#[specta::specta]
pub async fn clear_discord_cache() -> Answer<Vec<CacheReport>> {
    blocking(zapret::discord_cache::clear).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_service(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change_safely("remove_service", |_| zapret::service::remove());
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn quick_check(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::quick_check(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub fn take_notices(state: State<'_, AppState>) -> Vec<Notice> {
    state.take_notices()
}

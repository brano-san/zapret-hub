//! IPC commands: argument plumbing only. The work is in `actions`, `jobs` and the crates.

use std::path::Path;

use probe::socks::ProxyStatus;
use router::flows::FlowRow;
use router::rules::{self, FieldError, Preset, Rule};
use router::{RouterStatus, RoutesView};
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_dialog::DialogExt as _;
use tauri_specta::Event as _;
use tester::catalog::{StrategyDetail, StrategyRow};
use tester::history::RunSummary;
use tester::model::TestRun;
use tester::run::Request;
use zapret::checks::{CheckAction, SystemCheck};
use zapret::discord_cache::CacheReport;
use zapret::hosts::HostsPlan;
use zapret::release::{Downloaded, Engine};
use zapret::service::{QuickCheck, active};

use crate::actions::{self, state as app};
use crate::dto::{
    Candidates, Detected, DownloadProgress, Exclusions, FolderKind, LiveRun, Notice, Overview,
    TgWsStatus,
};
use crate::settings::Settings;
use crate::state::AppState;
use crate::{routing, texts, tgws, window};

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
    format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("ZH_COMMIT"))
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    let settings = state.settings();
    // Linux: the autostart entry may have been switched off in the desktop's own settings.
    #[cfg(not(windows))]
    let settings = Settings {
        autostart: std::env::current_exe().is_ok_and(|exe| crate::autostart::is_enabled(&exe)),
        ..settings
    };
    settings
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
pub async fn detect_folders() -> Answer<Detected> {
    blocking(|| Ok(actions::detect_folders())).await
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
            state.engine2().ok().flatten().as_ref(),
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
        // A zapret2 name shows why its folder cannot open, not a generic message.
        let engine2 = if zapret2::strip_prefix(&name).is_some() {
            Some(state.engine2_required()?)
        } else {
            state.engine2().ok().flatten()
        };
        tester::catalog::detail(
            engine2.as_ref(),
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

/// A zapret2 folder the first-run wizard was given, before it is saved: the same check as
/// `zapret2_info`. Window only: the pipe cannot choose the folder (zapret2 spec 10).
#[tauri::command]
#[specta::specta]
pub async fn check_zapret2_folder(path: String) -> Answer<zapret2::Info> {
    blocking(move || Ok(zapret2::info(Some(&path)))).await
}

/// The zapret2 editor's «Проверить»: the whole check of zapret2 spec 5.3, nothing saved.
#[tauri::command]
#[specta::specta]
pub async fn validate_zapret2(handle: AppHandle, text: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let engine = state.engine2_required()?;
        state.change(|| {
            let tail = zapret2::strategies::tail(&engine, &text)?;
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
        let deleted = state.engine2_required().and_then(|engine| {
            state.change(|| zapret2::strategies::delete(engine.root(), &name, active.as_deref()))
        });
        actions::changed(&handle);
        deleted
    })
    .await
}

/// The editor's circular builder reads the text it is given (zapret2 spec 5.4). Window only, as
/// are the two below: they shape a text the CLI writes itself.
#[tauri::command]
#[specta::specta]
pub fn zapret2_profiles(text: String) -> zapret2::circular::Analysis {
    zapret2::circular::analyze(&zapret2::strategies::args(&text))
}

/// `text` with the builder's circular profile written in; nothing saved.
#[tauri::command]
#[specta::specta]
pub fn zapret2_circular(
    text: String,
    target: zapret2::circular::CircularTarget,
    edit: zapret2::circular::CircularEdit,
) -> Answer<String> {
    zapret2::circular::apply(&text, &target, &edit).map_err(|err| format!("{err:#}"))
}

/// A new strategy's text made of 2-5 zapret2 strategies (full names), for its preview.
#[tauri::command]
#[specta::specta]
pub async fn zapret2_merge(
    handle: AppHandle,
    names: Vec<String>,
) -> Answer<zapret2::circular::Merged> {
    blocking(move || {
        let engine = app(&handle).engine2_required()?;
        let sources = names
            .iter()
            .map(|name| {
                let short = zapret2::strip_prefix(name).unwrap_or(name);
                let strategy = zapret2::strategies::find(engine.root(), short)?;
                Ok((strategy.name, strategy.text))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        zapret2::circular::merge(&sources)
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

/// Window only, like the folders themselves: the pipe has no download (cli.md).
#[tauri::command]
#[specta::specta]
pub fn default_download_parent() -> String {
    zapret::release::default_parent().display().to_string()
}

/// The latest release of `engine` unpacked into a new folder under `parent`; the page follows
/// `DownloadProgress` and puts the folder into its field.
#[tauri::command]
#[specta::specta]
pub async fn download_engine(
    handle: AppHandle,
    engine: Engine,
    parent: String,
) -> Answer<Downloaded> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        state.download(|cancel| {
            zapret::release::install(
                engine,
                Path::new(&parent),
                socks5.as_deref(),
                cancel,
                &|received, total| {
                    let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
                    let _ = DownloadProgress {
                        engine,
                        received: clamp(received),
                        total: clamp(total),
                    }
                    .emit(&handle);
                },
            )
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_download(state: State<'_, AppState>) {
    state.cancel_download();
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

/// What applying would change in hosts; nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn hosts_plan(handle: AppHandle) -> Answer<HostsPlan> {
    blocking(move || zapret::hosts::download_plan(app(&handle).socks5().as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_hosts(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        state.in_turn(|| zapret::hosts::download_and_apply(socks5.as_deref()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn system_checks(handle: AppHandle) -> Answer<Vec<SystemCheck>> {
    blocking(move || checks_now(&app(&handle))).await
}

/// The system checks as the page shows them; diagnostics take them too (read-only).
pub fn checks_now(state: &AppState) -> anyhow::Result<Vec<SystemCheck>> {
    let dir = state.settings().zapret2_dir;
    let mut checks = zapret::checks::run_all(dir.as_deref().map(Path::new));
    routing::hide_own_driver(&mut checks, &state.router.status().state);
    Ok(checks)
}

/// Removing WinDivert under the router's own handles: on a pause (router spec 11).
#[tauri::command]
#[specta::specta]
pub async fn fix_check(handle: AppHandle, action: CheckAction) -> Answer<()> {
    blocking(move || {
        let dir = app(&handle).settings().zapret2_dir;
        let fix =
            || app(&handle).in_turn(|| zapret::checks::fix(action, dir.as_deref().map(Path::new)));
        let done = if action == CheckAction::RemoveWinDivert {
            routing::paused(&handle, fix)
        } else {
            fix()
        };
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
        // It removes WinDivert too: the router's handles close first (router spec 11).
        let done = routing::paused(&handle, || {
            app(&handle).in_turn_safely("remove_service", zapret::service::remove)
        });
        actions::changed(&handle);
        done
    })
    .await
}

/// The zapret2 service by hand: `how` takes its name. Routing pauses around it, as around every
/// change of the service (router spec 11).
async fn service(handle: AppHandle, how: fn(&str) -> anyhow::Result<()>) -> Answer<()> {
    blocking(move || {
        let done = routing::paused(&handle, || app(&handle).in_turn(|| how(active().0)));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn start_service(handle: AppHandle) -> Answer<()> {
    service(handle, zapret::service::start).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_service(handle: AppHandle) -> Answer<()> {
    service(handle, zapret::service::stop).await
}

#[tauri::command]
#[specta::specta]
pub async fn restart_service(handle: AppHandle) -> Answer<()> {
    service(handle, zapret::service::restart).await
}

/// The service's journal (Linux; Windows keeps none).
#[tauri::command]
#[specta::specta]
pub async fn service_logs() -> Answer<String> {
    blocking(|| zapret::service::logs(active().0)).await
}

/// The nftables table the engine's queue runs through (Linux).
#[tauri::command]
#[specta::specta]
pub async fn nft_rules() -> Answer<String> {
    #[cfg(target_os = "linux")]
    let rules = blocking(zapret2::nft::rules).await;
    #[cfg(not(target_os = "linux"))]
    let rules = Err("nftables is on Linux only: Windows has WinDivert".to_owned());
    rules
}

/// The system's DNS cache, so the hosts file and a new strategy take effect at once.
#[tauri::command]
#[specta::specta]
pub async fn flush_dns() -> Answer<()> {
    blocking(zapret::hosts::flush_dns).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_exclusions(handle: AppHandle) -> Answer<Exclusions> {
    blocking(move || actions::exclusions(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_exclusions(handle: AppHandle, exclusions: Exclusions) -> Answer<()> {
    blocking(move || actions::save_exclusions(&handle, exclusions)).await
}

#[tauri::command]
#[specta::specta]
pub async fn tg_ws_status(handle: AppHandle) -> Answer<TgWsStatus> {
    blocking(move || Ok(tgws::status(&handle))).await
}

/// The latest TG WS Proxy release over the installed one.
#[tauri::command]
#[specta::specta]
pub async fn install_tg_ws(handle: AppHandle) -> Answer<TgWsStatus> {
    blocking(move || tgws::install(&handle)).await
}

/// The only command that writes `tg_ws_enabled`.
#[tauri::command]
#[specta::specta]
pub async fn set_tg_ws_enabled(handle: AppHandle, enabled: bool) -> Answer<TgWsStatus> {
    blocking(move || tgws::set_enabled(&handle, enabled)).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_tg_ws_config(
    handle: AppHandle,
    port: u16,
    secret: String,
    args: String,
) -> Answer<TgWsStatus> {
    blocking(move || tgws::save_config(&handle, port, secret, args)).await
}

/// The `tg://proxy` link to Telegram. Window only: the CLI prints the link.
#[tauri::command]
#[specta::specta]
pub async fn open_tg_proxy(handle: AppHandle) -> Answer<()> {
    blocking(move || tgws::open_link(&handle)).await
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

/// The only command that writes `router_enabled` (`routing::set_enabled`).
#[tauri::command]
#[specta::specta]
pub async fn set_router_enabled(handle: AppHandle, enabled: bool) -> Answer<RouterStatus> {
    blocking(move || routing::set_enabled(&handle, enabled)).await
}

#[tauri::command]
#[specta::specta]
pub fn router_status(state: State<'_, AppState>) -> RouterStatus {
    state.router.status()
}

/// Reading the rules may reopen the filter, up to a second: off the main thread.
#[tauri::command]
#[specta::specta]
pub async fn list_routes(handle: AppHandle) -> RoutesView {
    let router = app(&handle).router.clone();
    tauri::async_runtime::spawn_blocking(move || router.rules())
        .await
        .unwrap_or_else(|err| RoutesView {
            rules: Vec::new(),
            error: Some(format!("the command did not finish: {err}")),
            system_doh: false,
        })
}

#[tauri::command]
#[specta::specta]
pub async fn get_route(handle: AppHandle, name: String) -> Answer<Rule> {
    blocking(move || {
        let view = app(&handle).router.rules();
        let list: Vec<Rule> = view.rules.into_iter().map(|row| row.rule).collect();
        Ok(rules::find(&list, &name)?.clone())
    })
    .await
}

/// A rule changed in the window: the router edits `routes.toml` (it refuses a broken one).
fn edit_routes<T: Send + 'static>(
    handle: AppHandle,
    edit: impl FnOnce(&mut Vec<Rule>) -> anyhow::Result<T> + Send + 'static,
) -> impl Future<Output = Answer<T>> {
    blocking(move || {
        let done = app(&handle).router.edit(edit);
        actions::changed(&handle);
        done
    })
}

#[tauri::command]
#[specta::specta]
pub async fn save_route(handle: AppHandle, rule: Rule, create: bool) -> Answer<()> {
    edit_routes(handle, move |list| {
        if create {
            rules::add(list, rule)
        } else {
            rules::replace(list, rule)
        }
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_route_enabled(handle: AppHandle, name: String, enabled: bool) -> Answer<()> {
    edit_routes(handle, move |list| rules::set_enabled(list, &name, enabled)).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_route(handle: AppHandle, name: String) -> Answer<()> {
    edit_routes(handle, move |list| rules::remove(list, &name).map(|_| ())).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_route_preset(handle: AppHandle, preset: Preset) -> Answer<Rule> {
    let rule = rules::preset(preset);
    edit_routes(handle, move |list| {
        rules::add(list, rule.clone())?;
        Ok(rule)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn route_flows(state: State<'_, AppState>, all: bool) -> Vec<FlowRow> {
    state.router.flows(all)
}

/// The editor's checks as it types: nothing saved.
#[tauri::command]
#[specta::specta]
pub fn validate_route(rule: Rule) -> Vec<FieldError> {
    rules::check(&rule)
}

/// `ips` and `domains` plus the entries of a list file (one subnet or domain per line).
#[tauri::command]
#[specta::specta]
pub fn import_route_lines(
    ips: Vec<String>,
    domains: Vec<String>,
    text: String,
) -> Answer<rules::Imported> {
    rules::import(&ips, &domains, &text).map_err(|err| err.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn diag_profiles(handle: AppHandle) -> Answer<Vec<diag::profiles::Profile>> {
    blocking(move || crate::diag::profiles(&app(&handle))).await
}

#[tauri::command]
#[specta::specta]
pub async fn start_diag(handle: AppHandle, request: diag::run::Request) -> Answer<()> {
    blocking(move || crate::diag::start_diag(&handle, request)).await
}

#[tauri::command]
#[specta::specta]
pub fn diag_live(state: State<'_, AppState>) -> Option<diag::run::Live> {
    state.diag_live()
}

/// «Работает?» in the "with me" mode.
#[tauri::command]
#[specta::specta]
pub fn diag_answer(state: State<'_, AppState>, answer: diag::evaluate::Answer) -> Answer<()> {
    state
        .diag_reply(crate::diag::Reply::Answer(answer))
        .map_err(|err| format!("{err:#}"))
}

/// The culprit groups left ticked, by key.
#[tauri::command]
#[specta::specta]
pub fn diag_choose(state: State<'_, AppState>, keys: Vec<String>) -> Answer<()> {
    state
        .diag_reply(crate::diag::Reply::Choice(keys))
        .map_err(|err| format!("{err:#}"))
}

#[tauri::command]
#[specta::specta]
pub fn cancel_diag(state: State<'_, AppState>) {
    state.cancel_diag();
}

/// «Остановить»: unlike a cancel, the report keeps what was measured.
#[tauri::command]
#[specta::specta]
pub fn stop_diag(state: State<'_, AppState>) {
    state.stop_diag();
}

/// `(id, profile)`, newest first.
#[tauri::command]
#[specta::specta]
pub async fn diag_reports(handle: AppHandle) -> Answer<Vec<(String, String)>> {
    blocking(move || diag::report::list(&app(&handle).paths.diag())).await
}

#[tauri::command]
#[specta::specta]
pub async fn diag_report(handle: AppHandle, id: String) -> Answer<diag::summary::Summary> {
    blocking(move || diag::report::load(&app(&handle).paths.diag(), &id)).await
}

/// «Применить» of fix `fix` of report `id`; a quick check when it changed the strategy.
#[tauri::command]
#[specta::specta]
pub async fn diag_apply(handle: AppHandle, id: String, fix: u32) -> Answer<Option<QuickCheck>> {
    blocking(move || crate::diag::apply(&handle, &id, fix)).await
}

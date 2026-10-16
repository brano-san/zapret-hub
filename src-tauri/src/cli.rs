//! The app's end of `zapret-hub-cli` (CLI spec 4–6, 8): a pipe served from its own thread, each
//! request run by the same `commands::*` the window calls, so the queue, the snapshots, `changed()`
//! and the error texts are the window's.

use ::cli::pipe::{NAME, Server};
use ::cli::{Request, Response};
use serde_json::Value;
use tauri::async_runtime::block_on;
use tauri::{AppHandle, Manager as _};

use crate::actions;
use crate::commands;
use crate::dto::NoticeLevel;
use crate::state::AppState;
use crate::texts;

/// ERROR_ACCESS_DENIED from the first instance: another program holds the name.
const NAME_TAKEN: i32 = 5;

/// Called once from `setup`. The pipe is open even while the CLI is off: every request is checked
/// against the setting, so the switch works without a restart (spec 12, decision 3).
pub fn start(app: &AppHandle) {
    let server = match Server::bind(NAME) {
        Ok(server) => server,
        Err(err) => {
            log::error!("cli: {NAME} is not open: {err}");
            let title = if err.raw_os_error() == Some(NAME_TAKEN) {
                texts::CLI_PIPE_BUSY
            } else {
                texts::CLI_PIPE_FAILED
            };
            actions::notice(app, NoticeLevel::Error, title, format!("{NAME}: {err}"));
            return;
        }
    };
    log::info!("cli: listening on {NAME}");
    let app = app.clone();
    std::thread::spawn(move || {
        let clients = app.clone();
        let err = server.serve(move |line| {
            serde_json::to_string(&handle(&clients, line)).expect("an answer is plain data")
        });
        // No instance is left: zapret-hub-cli now takes the app for not running.
        log::error!("cli: {NAME} stopped: {err}");
        actions::notice(
            &app,
            NoticeLevel::Error,
            texts::CLI_PIPE_STOPPED,
            format!("{NAME}: {err}"),
        );
    });
}

/// One request line → one answer, and one journal line (spec 5).
fn handle(app: &AppHandle, line: &[u8]) -> Response {
    let enabled = app.state::<AppState>().settings().cli_enabled;
    let request = match ::cli::gate(line, enabled) {
        Ok(request) => request,
        Err(refusal) => {
            log::warn!(
                "cli: refused: {}",
                refusal.error.as_deref().unwrap_or_default()
            );
            return refusal;
        }
    };
    let cmd = request.name();
    match dispatch(app, request) {
        Ok(data) => {
            log::info!("cli: {cmd}: ok");
            Response::ok(data)
        }
        Err(err) => {
            log::warn!("cli: {cmd}: {err}");
            Response::error(err)
        }
    }
}

/// The window's command for each request: one arm each, so a new request does not compile until it
/// runs something.
fn dispatch(app: &AppHandle, request: Request) -> Result<Value, String> {
    let handle = app.clone();
    let state = app.state::<AppState>();
    match request {
        Request::AppVersion => json(commands::app_version()),
        Request::GetSettings => json(commands::get_settings(state)),
        Request::SaveSettings { settings } => {
            let settings = serde_json::from_value(settings)
                .map_err(|err| format!("the settings do not parse: {err}"))?;
            json(block_on(commands::save_settings_from_pipe(
                handle, settings,
            ))?)
        }
        Request::GetOverview => json(block_on(commands::get_overview(handle))?),
        Request::CheckProxy { addr } => json(block_on(commands::check_proxy(addr))?),
        Request::WizardCandidates => json(block_on(commands::wizard_candidates())?),
        Request::ListStrategies => json(block_on(commands::list_strategies(handle))?),
        Request::GetStrategy { name } => json(block_on(commands::get_strategy(handle, name))?),
        Request::ApplyStrategy { name } => json(block_on(commands::apply_strategy(handle, name))?),
        Request::ApplyBest => json(block_on(commands::apply_best(handle))?),
        Request::RestartZapret => json(block_on(commands::restart_zapret(handle))?),
        Request::CloneStrategy { source, name } => {
            json(block_on(commands::clone_strategy(handle, source, name))?)
        }
        Request::ValidateCustom { strategy } => {
            json(block_on(commands::validate_custom(handle, strategy))?)
        }
        Request::SaveCustom { strategy, create } => {
            json(block_on(commands::save_custom(handle, strategy, create))?)
        }
        Request::DeleteCustom { name } => json(block_on(commands::delete_custom(handle, name))?),
        Request::Zapret2Info => json(block_on(commands::zapret2_info(handle))?),
        Request::ValidateZapret2 { text } => {
            json(block_on(commands::validate_zapret2(handle, text))?)
        }
        Request::SaveZapret2 { name, text, create } => json(block_on(commands::save_zapret2(
            handle, name, text, create,
        ))?),
        Request::DeleteZapret2 { name } => json(block_on(commands::delete_zapret2(handle, name))?),
        Request::StartTests { request } => json(block_on(commands::start_tests(handle, request))?),
        Request::TestsStatus => Ok(tests_status(&state)),
        Request::CancelTests => {
            commands::cancel_tests(state);
            Ok(Value::Null)
        }
        Request::ListRuns => json(block_on(commands::list_runs(handle))?),
        Request::GetRun { id } => json(block_on(commands::get_run(handle, id))?),
        Request::RunReport { id } => json(block_on(commands::run_report(handle, id))?),
        Request::GetFlowseal => json(block_on(commands::get_flowseal(handle))?),
        Request::SetGameFilter { filter } => {
            json(block_on(commands::set_game_filter(handle, filter))?)
        }
        Request::SetIpsetMode { mode } => json(block_on(commands::set_ipset_mode(handle, mode))?),
        Request::SetCheckUpdates { enabled } => {
            json(block_on(commands::set_check_updates(handle, enabled))?)
        }
        Request::ReplaceFake { kind, source } => {
            json(block_on(commands::replace_fake(handle, kind, source))?)
        }
        Request::AddListEntry { list, entry } => {
            json(block_on(commands::add_list_entry(handle, list, entry))?)
        }
        Request::RemoveListEntry { list, entry } => {
            json(block_on(commands::remove_list_entry(handle, list, entry))?)
        }
        Request::UpdateIpset => json(block_on(commands::update_ipset(handle))?),
        Request::HostsPlan => json(block_on(commands::hosts_plan(handle))?),
        Request::ApplyHosts => json(block_on(commands::apply_hosts(handle))?),
        Request::CheckFlowsealUpdate => json(block_on(commands::check_flowseal_update(handle))?),
        Request::SystemChecks => json(block_on(commands::system_checks(handle))?),
        Request::FixCheck { action } => json(block_on(commands::fix_check(handle, action))?),
        Request::ClearDiscordCache => json(block_on(commands::clear_discord_cache())?),
        Request::RemoveService => json(block_on(commands::remove_service(handle))?),
        Request::QuickCheck => json(block_on(commands::quick_check(handle))?),
        Request::SetRouterEnabled { enabled } => {
            json(block_on(commands::set_router_enabled(handle, enabled))?)
        }
        Request::RouterStatus => json(commands::router_status(state)),
        Request::ListRoutes => json(block_on(commands::list_routes(handle))),
        Request::GetRoute { name } => json(block_on(commands::get_route(handle, name))?),
        Request::SaveRoute { rule, create } => {
            json(block_on(commands::save_route(handle, rule, create))?)
        }
        Request::SetRouteEnabled { name, enabled } => json(block_on(commands::set_route_enabled(
            handle, name, enabled,
        ))?),
        Request::DeleteRoute { name } => json(block_on(commands::delete_route(handle, name))?),
        Request::AddRoutePreset { preset } => {
            json(block_on(commands::add_route_preset(handle, preset))?)
        }
        Request::RouteFlows { all } => json(commands::route_flows(state, all)),
    }
}

fn json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| format!("the answer does not serialize: {err}"))
}

/// `tests_status` (spec 7.1): `test_live` and how the newest run ended.
fn tests_status(state: &AppState) -> Value {
    serde_json::json!({
        "testing": state.testing(),
        "live": state.live(),
        "last": state.last_outcome(),
    })
}

#[cfg(test)]
mod tests {
    use tester::model::Suite;
    use tester::run::Request as TestRequest;

    use super::*;
    use crate::dto::LiveRun;
    use crate::settings::Settings;
    use crate::state::Paths;

    #[test]
    fn tests_status_is_what_the_cli_reads() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(
            Paths {
                exe_dir: dir.path().to_path_buf(),
            },
            Settings::default(),
        );
        let read = |state: &AppState| -> ::cli::TestsStatus {
            serde_json::from_value(tests_status(state)).unwrap()
        };
        assert_eq!(
            read(&state),
            ::cli::TestsStatus {
                testing: false,
                live: None,
                last: None
            }
        );
        let request = TestRequest {
            suite: Suite::Standard,
            strategies: vec!["general".into(), "general (ALT2)".into()],
            repeats: 2,
        };
        assert!(state.begin_testing());
        state.set_live(Some(LiveRun {
            request: request.clone(),
            current: Some(1),
            restoring: false,
            pass: 1,
            results: Vec::new(),
        }));
        let going = read(&state);
        assert!(going.testing);
        assert_eq!(
            going.live,
            Some(::cli::Live {
                request,
                current: Some(1),
                restoring: false,
                pass: 1,
            })
        );
        let failed = ::cli::Outcome::Failed {
            error: "curl.exe was not found".into(),
        };
        state.end_testing(failed.clone());
        assert_eq!(
            read(&state),
            ::cli::TestsStatus {
                testing: false,
                live: None,
                last: Some(failed)
            }
        );
    }
}

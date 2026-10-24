//! Work that outlives a command: a test run, and what happens once at start.

use std::path::PathBuf;
use std::time::Duration;

use router::RouterState;
use tauri::AppHandle;
use tauri_specta::Event as _;
use tester::model::{Suite, TestRun};
use tester::run::{Progress, Request};
use zapret::service::{QuickCheck, SvcState, active};
use zapret::snapshot::{Snapshot, pending_path};

use crate::actions::{self, state};
use crate::dto::{LiveRun, NoticeLevel, TestProgress, TestsFinished};
use crate::{notify, texts};

/// Starts a run in its own thread; the page follows `TestProgress` and `TestsFinished`.
pub fn start_tests(app: &AppHandle, request: Request) -> anyhow::Result<()> {
    anyhow::ensure!(
        !request.strategies.is_empty(),
        "choose at least one strategy"
    );
    let state = state(app);
    tester::run::zapret2_only(&request)?;
    let engine2 = Some(state.engine2_required()?);
    state.begin_testing()?;
    state.set_live(Some(LiveRun {
        request: request.clone(),
        current: None,
        restoring: false,
        pass: 0,
        results: Vec::new(),
    }));
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = actions::state(&handle);
        // Routing is paused for the run and comes back however it ends (router spec 7).
        let outcome = crate::routing::paused(&handle, || {
            // The whole run is one turn of the queue: nothing else changes zapret meanwhile.
            let _turn = state.turn();
            let socks5 = state.socks5();
            // `run` puts zapret back before it passes a panic on: here the run only has to end.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tester::run::run(
                    engine2.as_ref(),
                    &state.paths.data(),
                    &request,
                    socks5.as_deref(),
                    state.cancel_flag(),
                    &|progress| on_progress(&handle, progress),
                )
            }))
            .unwrap_or_else(|panic| {
                // `panic!` leaves a `&str` or a `String`.
                let message = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
                Err(anyhow::anyhow!(
                    "the test run panicked: {}",
                    message.unwrap_or("no message")
                ))
            })
        });
        finish(&handle, outcome);
    });
    actions::changed(app);
    Ok(())
}

fn on_progress(app: &AppHandle, progress: Progress) {
    state(app).update_live(|live| match &progress {
        Progress::Strategy { index, pass, .. } => {
            live.current = Some(*index);
            live.pass = *pass;
        }
        // A later pass brings the strategy's new aggregate: it takes the place of the old one.
        Progress::Done { result } => {
            match live.results.iter_mut().find(|r| r.name == result.name) {
                Some(old) => *old = (**result).clone(),
                None => live.results.push((**result).clone()),
            }
        }
        Progress::Restoring => {
            live.current = None;
            live.restoring = true;
        }
    });
    if let Err(err) = TestProgress(progress).emit(app) {
        log::warn!("cannot report test progress: {err}");
    }
}

fn finish(app: &AppHandle, outcome: anyhow::Result<TestRun>) {
    let state = state(app);
    let (run_id, last) = match outcome {
        Ok(run) => {
            if let Some(err) = &run.restore_error {
                actions::notice(
                    app,
                    NoticeLevel::Error,
                    texts::RESTORE_AFTER_TESTS,
                    err.clone(),
                );
            }
            let title = if run.canceled {
                texts::TESTS_CANCELED
            } else {
                texts::TESTS_DONE
            };
            notify::show(app, title, &texts::tests_done(&run));
            let id = run.id.clone();
            if run.suite == Suite::Standard && !run.canceled {
                state.set_ranking(Some(run));
            }
            (Some(id.clone()), cli::Outcome::Run { run_id: id })
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(app, texts::TESTS_FAILED, &text);
            actions::notice(app, NoticeLevel::Error, texts::TESTS_FAILED, text.clone());
            (None, cli::Outcome::Failed { error: text })
        }
    };
    state.end_testing(last);
    if let Err(err) = (TestsFinished { run_id }).emit(app) {
        log::warn!("cannot report the end of a test run: {err}");
    }
    actions::changed(app);
}

const QUICK_EVERY: Duration = Duration::from_secs(30 * 60);

/// Called once from `setup`: the start (with the move off Flowseal when 0.8.0 left its folder in
/// the settings), then a quick check every 30 minutes (spec 9.3).
pub fn start(app: &AppHandle, flowseal: Option<PathBuf>) {
    let app = app.clone();
    std::thread::spawn(move || {
        startup(&app, flowseal);
        loop {
            background_check(&app);
            std::thread::sleep(QUICK_EVERY);
        }
    });
}

/// A toast only when a check that passed now fails while the service runs (spec 9.3). Not for a
/// first check (at logon the network may not be up yet), and not again on every tick of a service
/// that stays broken.
fn stopped_passing(before: Option<&QuickCheck>, now: &QuickCheck) -> bool {
    now.service == SvcState::Running && !now.ok && before.is_some_and(|before| before.ok)
}

fn background_check(app: &AppHandle) {
    let before = state(app).quick();
    // No folder yet, or a test run: nothing to check, and nothing to say about it.
    let Ok(check) = actions::quick_check(app) else {
        return;
    };
    if stopped_passing(before.as_ref(), &check) {
        notify::show(app, texts::QUICK_FAILED, &texts::quick_result(&check));
    }
}

/// Before anything else changes zapret, an interrupted operation is put back (spec 10).
fn startup(app: &AppHandle, flowseal: Option<PathBuf>) {
    recover(app);
    raise_services(app);
    let state = state(app);
    // Routing comes back as it was left (router spec 3).
    crate::routing::sync(app);
    if let RouterState::Unavailable { reason } = state.router.status().state {
        actions::notice(app, NoticeLevel::Error, texts::ROUTER_FAILED, reason);
    }
    if let Some(dir) = flowseal {
        actions::migrate_flowseal(app, &dir);
    }
    if state.settings().autostart {
        let refreshed = std::env::current_exe()
            .map_err(anyhow::Error::from)
            .and_then(|exe| crate::autostart::refresh(&exe));
        if let Err(err) = refreshed {
            actions::notice(
                app,
                NoticeLevel::Error,
                texts::AUTOSTART_FAILED,
                format!("{err:#}"),
            );
        }
    }
    match tester::history::latest_ranking(&tester::history::dir(&state.paths.data())) {
        Ok(run) => state.set_ranking(run),
        Err(err) => actions::notice(
            app,
            NoticeLevel::Error,
            texts::HISTORY_FAILED,
            format!("{err:#}"),
        ),
    }
    actions::changed(app);
}

/// Linux: after a boot the strategy's unit may be enabled yet down (`/home` or the network were
/// not ready and systemd's retries ran out): it is started here, and the first background check
/// turns the tray green. Windows' service manager does this itself. TG WS Proxy comes up on
/// both when it is on (Windows: it is the app's own child).
fn raise_services(app: &AppHandle) {
    let (name, _) = active();
    let down = cfg!(target_os = "linux")
        && zapret::service::strategy().is_some()
        && zapret::service::config(name).is_some_and(|config| config.auto_start)
        && zapret::service::state(name) != SvcState::Running;
    if down && let Err(err) = state(app).in_turn(|| zapret::service::start(name)) {
        actions::notice(
            app,
            NoticeLevel::Error,
            texts::SERVICE_RAISE_FAILED,
            format!("{err:#}"),
        );
    }
    if let Err(err) = crate::tgws::raise(app) {
        actions::notice(
            app,
            NoticeLevel::Error,
            texts::TG_WS_RAISE_FAILED,
            format!("{err:#}"),
        );
    }
}

fn recover(app: &AppHandle) {
    let state = state(app);
    // From the read on: an operation that starts meanwhile would restore the same file first, and
    // this would then undo it.
    let _turn = state.turn();
    let pending = pending_path(&state.paths.data());
    let snapshot = match Snapshot::load(&pending) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return,
        Err(err) => {
            // Unreadable: set aside, or every start would trip over it.
            let broken = pending.with_extension("json.broken");
            let moved = match std::fs::rename(&pending, &broken) {
                Ok(()) => broken.display().to_string(),
                Err(rename) => format!("{}\n\n{rename}", texts::recovery_failed(&pending)),
            };
            actions::notice(
                app,
                NoticeLevel::Error,
                texts::RECOVERY_FAILED,
                format!("{err:#}\n\n{moved}"),
            );
            return;
        }
    };
    log::warn!(
        "unfinished operation {} found, restoring",
        snapshot.operation
    );
    match snapshot.restore().and_then(|()| Snapshot::clear(&pending)) {
        Ok(()) => actions::notice(
            app,
            NoticeLevel::Info,
            texts::RECOVERED,
            texts::recovered(texts::operation(&snapshot.operation)),
        ),
        // pending.json stays: the next start or operation tries again.
        Err(err) => actions::notice(
            app,
            NoticeLevel::Error,
            texts::RECOVERY_FAILED,
            format!("{}\n\n{err:#}", texts::recovery_failed(&pending)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(ok: bool, service: SvcState) -> QuickCheck {
        QuickCheck {
            ok,
            service,
            winws_running: ok,
            checks: Vec::new(),
        }
    }

    #[test]
    fn a_toast_only_when_a_passing_check_starts_failing() {
        let pass = check(true, SvcState::Running);
        let fail = check(false, SvcState::Running);
        assert!(stopped_passing(Some(&pass), &fail));
        assert!(
            !stopped_passing(None, &fail),
            "a first check has nothing it stopped passing"
        );
        assert!(
            !stopped_passing(Some(&fail), &fail),
            "no second toast while it stays broken"
        );
        assert!(!stopped_passing(Some(&pass), &pass));
        assert!(
            !stopped_passing(Some(&pass), &check(false, SvcState::Stopped)),
            "a stopped service is the tray's gray, not a toast"
        );
    }
}

//! Work that outlives a command: a test run, and what happens once at start.

use std::time::{Duration, Instant};

use router::RouterState;
use tauri::AppHandle;
use tauri_specta::Event as _;
use tester::model::{Suite, TestRun};
use tester::run::{Progress, Request};
use zapret::service::{QuickCheck, SvcState};
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
    // A run of zapret2 strategies alone needs no Flowseal folder (`tester::run::needs_flowseal`).
    let folder = state.folder_if_set()?;
    // Only a run with zapret2 strategies needs zapret2: then a folder that opens, or the reason.
    let engine2 = if request
        .strategies
        .iter()
        .any(|name| zapret2::strip_prefix(name).is_some())
    {
        Some(state.engine2_required()?)
    } else {
        None
    };
    anyhow::ensure!(state.begin_testing(), "a test run is already going");
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
                    folder.as_ref(),
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
const VERSION_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Called once from `setup`: the start, then a quick check every 30 minutes and the Flowseal
/// version once a day (spec 5.6, 9.3).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        startup(&app);
        let mut version_checked: Option<Instant> = None;
        loop {
            // First: a download that hangs (up to 30 s per attempt) must not hold back the tray's
            // first color.
            background_check(&app);
            // The day counts from a check that ran: a skipped or failed one is tried again on the
            // next tick.
            if version_checked.is_none_or(|at| at.elapsed() >= VERSION_EVERY) && check_version(&app)
            {
                version_checked = Some(Instant::now());
            }
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

/// When Flowseal's own flag asks for it; one notification per new version. True when the check
/// ran: no folder, the flag off (both silent) and a failed download all return false.
fn check_version(app: &AppHandle) -> bool {
    let state = state(app);
    let Ok(folder) = state.folder() else {
        return false;
    };
    if !zapret::flags::check_updates(&folder) {
        return false;
    }
    let known = state.update();
    match actions::check_flowseal_update(app) {
        Ok(Some(version)) if known.as_deref() != Some(version.as_str()) => {
            notify::show(app, texts::UPDATE, &texts::update(&version));
            true
        }
        Ok(_) => true,
        Err(err) => {
            log::warn!("cannot check the Flowseal version: {err:#}");
            false
        }
    }
}

/// Before anything else changes zapret, an interrupted operation is put back (spec 10).
fn startup(app: &AppHandle) {
    recover(app);
    let state = state(app);
    // Routing comes back as it was left (router spec 3).
    crate::routing::sync(app);
    if let RouterState::Unavailable { reason } = state.router.status().state {
        actions::notice(app, NoticeLevel::Error, texts::ROUTER_FAILED, reason);
    }
    actions::prepare_folder(app);
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
    let restored = state
        .folder_if_set()
        .and_then(|folder| snapshot.restore(folder.as_ref()));
    match restored.and_then(|()| Snapshot::clear(&pending)) {
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

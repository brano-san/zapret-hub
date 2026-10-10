//! Work that outlives a command: a test run, and what happens once at start.

use tauri::AppHandle;
use tauri_specta::Event as _;
use tester::model::{Suite, TestRun};
use tester::run::{Progress, Request};
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
    let folder = state.folder()?;
    anyhow::ensure!(state.begin_testing(), "a test run is already going");
    state.set_live(Some(LiveRun {
        request: request.clone(),
        current: None,
        restoring: false,
        results: Vec::new(),
    }));
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = actions::state(&handle);
        let outcome = {
            // The whole run is one turn of the queue: nothing else changes zapret meanwhile.
            let _turn = state.turn();
            let socks5 = state.socks5();
            // `run` puts zapret back before it passes a panic on: here the run only has to end.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tester::run::run(
                    &folder,
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
        };
        finish(&handle, outcome);
    });
    actions::changed(app);
    Ok(())
}

fn on_progress(app: &AppHandle, progress: Progress) {
    state(app).update_live(|live| match &progress {
        Progress::Strategy { index, .. } => live.current = Some(*index),
        Progress::Done { result } => live.results.push(result.clone()),
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
    let run_id = match outcome {
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
            Some(id)
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(app, texts::TESTS_FAILED, &text);
            actions::notice(app, NoticeLevel::Error, texts::TESTS_FAILED, text);
            None
        }
    };
    state.set_live(None);
    state.end_testing();
    if let Err(err) = (TestsFinished { run_id }).emit(app) {
        log::warn!("cannot report the end of a test run: {err}");
    }
    actions::changed(app);
}

/// Called once from `setup`; the rest of the app does not wait for it.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || startup(&app));
}

/// Before anything else changes zapret, an interrupted operation is put back (spec 10).
fn startup(app: &AppHandle) {
    recover(app);
    actions::prepare_folder(app);
    let state = state(app);
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
                Err(rename) => rename.to_string(),
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
    let restored = state.folder().and_then(|folder| snapshot.restore(&folder));
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

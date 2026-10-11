//! Where the app keeps its files (spec 4.3) and what it holds while it runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use anyhow::Context as _;
use tester::history::RunSummary;
use tester::model::TestRun;
use zapret::custom::Store;
use zapret::folder::Folder;
use zapret::service::QuickCheck;
use zapret::snapshot::{Snapshot, pending_path};

use crate::dto::{LiveRun, Notice, NoticeLevel};
use crate::settings::Settings;

/// Everything lives next to the exe: the app is portable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub exe_dir: PathBuf,
}

impl Paths {
    pub fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("cannot find the executable's path")?;
        let exe_dir = exe
            .parent()
            .context("the executable has no folder")?
            .to_path_buf();
        Ok(Self { exe_dir })
    }

    pub fn settings(&self) -> PathBuf {
        self.exe_dir.join("zapret-hub.toml")
    }

    pub fn data(&self) -> PathBuf {
        self.exe_dir.join("data")
    }

    pub fn strategies(&self) -> PathBuf {
        self.data().join("strategies")
    }

    pub fn logs(&self) -> PathBuf {
        self.data().join("logs")
    }
}

/// A poisoned lock still holds good data: a panic elsewhere must not take the app down.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

const TESTING: &str = "a test run is going: wait for it or cancel it";

pub struct AppState {
    pub paths: Paths,
    settings: Mutex<Settings>,
    /// Held by every change to zapret and by a whole test run: one at a time (spec 10).
    queue: Mutex<()>,
    testing: AtomicBool,
    cancel: AtomicBool,
    live: Mutex<Option<LiveRun>>,
    quick: Mutex<Option<QuickCheck>>,
    /// The newest finished standard run: «Применить лучшую», the overview, the tray.
    ranking: Mutex<Option<TestRun>>,
    /// A Flowseal version newer than the folder's.
    update: Mutex<Option<String>>,
    notices: Mutex<Vec<Notice>>,
    /// How the newest run since the start ended: `tests_status` (CLI spec 7.1).
    last_outcome: Mutex<Option<cli::Outcome>>,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            settings: Mutex::new(settings),
            queue: Mutex::new(()),
            testing: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            live: Mutex::new(None),
            quick: Mutex::new(None),
            ranking: Mutex::new(None),
            update: Mutex::new(None),
            notices: Mutex::new(Vec::new()),
            last_outcome: Mutex::new(None),
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        *lock(&self.settings) = settings;
    }

    pub fn socks5(&self) -> Option<String> {
        lock(&self.settings).socks5.clone()
    }

    pub fn folder(&self) -> anyhow::Result<Folder> {
        let dir = lock(&self.settings)
            .zapret_dir
            .clone()
            .context("the zapret folder is not set: choose it in Settings")?;
        Folder::open(dir)
    }

    pub fn store(&self) -> Store {
        Store::new(self.paths.strategies())
    }

    pub fn testing(&self) -> bool {
        self.testing.load(Ordering::SeqCst)
    }

    /// Marks a test run as going; false when one already is.
    pub fn begin_testing(&self) -> bool {
        let began = self
            .testing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if began {
            self.cancel.store(false, Ordering::SeqCst);
        }
        began
    }

    /// The outcome is in place before `testing` turns false: whoever polls `tests_status` never
    /// sees a finished run with the previous run's outcome (CLI spec 7.1).
    pub fn end_testing(&self, outcome: cli::Outcome) {
        *lock(&self.last_outcome) = Some(outcome);
        self.set_live(None);
        self.testing.store(false, Ordering::SeqCst);
    }

    pub fn last_outcome(&self) -> Option<cli::Outcome> {
        lock(&self.last_outcome).clone()
    }

    pub fn cancel_tests(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// The queue's turn. A test run holds it from start to end.
    pub fn turn(&self) -> MutexGuard<'_, ()> {
        lock(&self.queue)
    }

    /// A change to zapret: one at a time, never during a test run.
    pub fn change<T>(&self, f: impl FnOnce(&Folder) -> anyhow::Result<T>) -> anyhow::Result<T> {
        anyhow::ensure!(!self.testing(), TESTING);
        let _turn = self.turn();
        f(&self.folder()?)
    }

    /// A change that could leave zapret half done: the state goes to `pending.json` before the
    /// first step and is put back if any step fails (spec 10). A `pending.json` an earlier
    /// operation left is restored first, and when that fails this one does not start
    /// (`zapret::snapshot::begin`). `operation` is the id `texts::operation` names.
    pub fn change_safely<T>(
        &self,
        operation: &str,
        f: impl FnOnce(&Folder) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.change(|folder| {
            let pending = pending_path(&self.paths.data());
            let snapshot = zapret::snapshot::begin(folder, operation, &pending)?;
            let result = f(folder);
            if let Err(err) = &result {
                log::error!("{operation} failed, restoring the previous state: {err:#}");
                if let Err(undo) = snapshot.restore(folder) {
                    // pending.json stays: the next start or operation tries again.
                    return Err(anyhow::anyhow!(
                        "{err:#}\n\nthe previous state was not restored either: {undo:#}"
                    ));
                }
            }
            Snapshot::clear(&pending)?;
            result
        })
    }

    pub fn live(&self) -> Option<LiveRun> {
        lock(&self.live).clone()
    }

    pub fn set_live(&self, live: Option<LiveRun>) {
        *lock(&self.live) = live;
    }

    pub fn update_live(&self, f: impl FnOnce(&mut LiveRun)) {
        if let Some(live) = lock(&self.live).as_mut() {
            f(live);
        }
    }

    pub fn quick(&self) -> Option<QuickCheck> {
        lock(&self.quick).clone()
    }

    /// Returns the check it replaces.
    pub fn set_quick(&self, check: QuickCheck) -> Option<QuickCheck> {
        lock(&self.quick).replace(check)
    }

    pub fn set_ranking(&self, run: Option<TestRun>) {
        *lock(&self.ranking) = run;
    }

    pub fn last_run(&self) -> Option<RunSummary> {
        lock(&self.ranking).as_ref().map(tester::history::summary)
    }

    /// The first `n` of the ranking that started at all.
    pub fn best(&self, n: usize) -> Vec<String> {
        lock(&self.ranking)
            .as_ref()
            .map(|run| {
                run.strategies
                    .iter()
                    .filter(|s| s.launch_error.is_none())
                    .take(n)
                    .map(|s| s.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn update(&self) -> Option<String> {
        lock(&self.update).clone()
    }

    pub fn set_update(&self, version: Option<String>) {
        *lock(&self.update) = version;
    }

    pub fn push_notice(&self, notice: Notice) {
        lock(&self.notices).push(notice);
    }

    /// Everything queued, errors first (spec 10).
    pub fn take_notices(&self) -> Vec<Notice> {
        let mut notices = std::mem::take(&mut *lock(&self.notices));
        // Stable: within a level the order they came in.
        notices.sort_by_key(|notice| notice.level != NoticeLevel::Error);
        notices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(dir: &std::path::Path) -> AppState {
        AppState::new(
            Paths {
                exe_dir: dir.to_path_buf(),
            },
            Settings::default(),
        )
    }

    fn notice(level: NoticeLevel, title: &str) -> Notice {
        Notice {
            level,
            title: title.into(),
            text: String::new(),
        }
    }

    #[test]
    fn notices_come_errors_first_and_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.push_notice(notice(NoticeLevel::Info, "a"));
        state.push_notice(notice(NoticeLevel::Error, "b"));
        state.push_notice(notice(NoticeLevel::Info, "c"));
        let titles: Vec<String> = state.take_notices().into_iter().map(|n| n.title).collect();
        assert_eq!(titles, ["b", "a", "c"]);
        assert!(state.take_notices().is_empty());
    }

    #[test]
    fn changes_need_a_folder_and_wait_out_a_test_run() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let err = state.change(|_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("folder is not set"), "{err:#}");
        assert!(state.begin_testing());
        assert!(!state.begin_testing(), "a second run must not start");
        let err = state.change(|_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("test run"), "{err:#}");
        state.end_testing(cli::Outcome::Failed {
            error: "canceled".into(),
        });
        assert!(!state.testing());
    }

    #[test]
    fn a_run_ends_with_its_outcome_in_place() {
        use tester::model::Suite;
        use tester::run::Request;

        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        assert!(state.begin_testing());
        state.set_live(Some(LiveRun {
            request: Request {
                suite: Suite::Standard,
                strategies: vec!["general".into()],
            },
            current: Some(0),
            restoring: false,
            results: Vec::new(),
        }));
        let saved = cli::Outcome::Run {
            run_id: "2026-10-11_10-00-00".into(),
        };
        state.end_testing(saved.clone());
        assert!(!state.testing());
        assert_eq!(state.live(), None);
        assert_eq!(state.last_outcome(), Some(saved));
    }
}

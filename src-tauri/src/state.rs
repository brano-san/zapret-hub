//! Where the app keeps its files (spec 4.3) and what it holds while it runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::Context as _;
use tester::history::RunSummary;
use tester::model::TestRun;
use zapret::service::QuickCheck;
use zapret::snapshot::{Snapshot, pending_path};

use crate::diag::Reply;
use crate::dto::{LiveRun, Notice, NoticeLevel};
use crate::settings::Settings;

/// Windows: everything lives next to the exe, the app is portable. Linux: the settings in
/// `~/.config/zapret-hub`, the data in `~/.local/share/zapret-hub` (XDG): the exe usually sits in
/// a folder the user cannot write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Holds `zapret-hub.toml`.
    config: PathBuf,
    data: PathBuf,
}

impl Paths {
    /// `zapret-hub.toml` in `dir`, the data in its `data` folder.
    #[cfg(any(windows, test))]
    pub fn portable(dir: &std::path::Path) -> Self {
        Self {
            config: dir.to_path_buf(),
            data: dir.join("data"),
        }
    }

    #[cfg(windows)]
    pub fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("cannot find the executable's path")?;
        Ok(Self::portable(
            exe.parent().context("the executable has no folder")?,
        ))
    }

    /// The settings folder is made here: a save writes a temporary file next to the settings.
    #[cfg(not(windows))]
    pub fn current() -> anyhow::Result<Self> {
        let paths = Self {
            config: zapret::config_home().join("zapret-hub"),
            data: zapret::data_home().join("zapret-hub"),
        };
        anyhow::ensure!(
            paths.config.is_absolute() && paths.data.is_absolute(),
            "neither HOME nor the XDG folders are set: there is nowhere to keep the settings"
        );
        std::fs::create_dir_all(&paths.config)
            .with_context(|| format!("cannot create {}", paths.config.display()))?;
        Ok(paths)
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("zapret-hub.toml")
    }

    pub fn data(&self) -> PathBuf {
        self.data.clone()
    }

    pub fn logs(&self) -> PathBuf {
        self.data().join("logs")
    }

    pub fn routes(&self) -> PathBuf {
        self.data().join("routes.toml")
    }

    /// Diagnostics reports, `circular.toml` and the profiles that replace the built-in ones.
    pub fn diag(&self) -> PathBuf {
        diag::report::dir(&self.data())
    }
}

/// A poisoned lock still holds good data: a panic elsewhere must not take the app down.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub const TESTING: &str = "a test run is going: wait for it or cancel it";
pub const DIAGNOSING: &str = "diagnostics are running: wait for them or cancel them";

pub struct AppState {
    pub paths: Paths,
    settings: Mutex<Settings>,
    /// Held by a whole save of the settings: the window and zapret-hub-cli save one at a time.
    /// Not `queue`: a test run holds that for minutes.
    saving: Mutex<()>,
    /// Held by every change to zapret and by a whole test run: one at a time (spec 10).
    queue: Mutex<()>,
    testing: AtomicBool,
    cancel: AtomicBool,
    /// Held while a test run or diagnostics starts: one of them at a time.
    starting: Mutex<()>,
    /// Diagnostics (diag spec 9): like a test run, they hold the turn and block changes.
    diagnosing: AtomicBool,
    diag_cancel: AtomicBool,
    /// «Остановить»: ends the observation (taken) or the path search (kept), see `stop_diag`.
    diag_stop: AtomicBool,
    diag_live: Mutex<Option<diag::run::Live>>,
    /// Why the newest diagnostics failed: `diag_failed` for the CLI, which has no notices.
    diag_failed: Mutex<Option<String>>,
    /// The page's answer or choice the diagnostics wait for.
    diag_reply: Mutex<Option<Reply>>,
    diag_wake: Condvar,
    /// A release download from the window (`release::install`): one at a time, its own cancel.
    downloading: AtomicBool,
    download_cancel: AtomicBool,
    live: Mutex<Option<LiveRun>>,
    quick: Mutex<Option<QuickCheck>>,
    /// The newest finished standard run: «Применить лучшую», the overview, the tray.
    ranking: Mutex<Option<TestRun>>,
    notices: Mutex<Vec<Notice>>,
    /// How the newest run since the start ended: `tests_status` (CLI spec 7.1).
    last_outcome: Mutex<Option<cli::Outcome>>,
    /// Routing through the SOCKS5 proxy: `routing` drives it.
    pub router: router::Router,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            router: router::Router::new(paths.routes()),
            paths,
            settings: Mutex::new(settings),
            saving: Mutex::new(()),
            queue: Mutex::new(()),
            testing: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            starting: Mutex::new(()),
            diagnosing: AtomicBool::new(false),
            diag_cancel: AtomicBool::new(false),
            diag_stop: AtomicBool::new(false),
            diag_live: Mutex::new(None),
            diag_failed: Mutex::new(None),
            diag_reply: Mutex::new(None),
            diag_wake: Condvar::new(),
            downloading: AtomicBool::new(false),
            download_cancel: AtomicBool::new(false),
            live: Mutex::new(None),
            quick: Mutex::new(None),
            ranking: Mutex::new(None),
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

    /// The settings' own turn: one save at a time (`actions::save_settings`).
    pub fn saving(&self) -> MutexGuard<'_, ()> {
        lock(&self.saving)
    }

    pub fn socks5(&self) -> Option<String> {
        lock(&self.settings).socks5.clone()
    }

    /// The zapret2 engine with the settings' game filter and excluded ports; `None` while
    /// `zapret2_dir` is empty (zapret2 spec 4).
    pub fn engine2(&self) -> anyhow::Result<Option<zapret2::Engine>> {
        let settings = self.settings();
        settings
            .zapret2_dir
            .map(|dir| {
                zapret2::Engine::open(dir)?
                    .with_game_filter(settings.game_filter)
                    .with_excluded_ports(&settings.excluded_ports)
            })
            .transpose()
    }

    /// Moves the strategies an older version kept in `data\zapret2` into the zapret2 folder.
    pub fn migrate_strategies(&self) {
        if let Ok(Some(engine)) = self.engine2() {
            let moved =
                zapret2::strategies::migrate(&self.paths.data().join("zapret2"), engine.root());
            if !moved.is_empty() {
                log::info!(
                    "zapret2 strategies moved into the zapret2 folder: {}",
                    moved.join(", ")
                );
            }
        }
    }

    /// For what cannot go without zapret2: applying, checking and saving its strategies.
    pub fn engine2_required(&self) -> anyhow::Result<zapret2::Engine> {
        self.engine2()?
            .context("the zapret2 folder is not set: choose it in Settings")
    }

    pub fn testing(&self) -> bool {
        self.testing.load(Ordering::SeqCst)
    }

    /// Marks a test run as going; refused while one already is or diagnostics run.
    pub fn begin_testing(&self) -> anyhow::Result<()> {
        let _starting = lock(&self.starting);
        anyhow::ensure!(!self.diagnosing(), DIAGNOSING);
        anyhow::ensure!(!self.testing(), "a test run is already going");
        self.cancel.store(false, Ordering::SeqCst);
        self.testing.store(true, Ordering::SeqCst);
        Ok(())
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

    pub fn diagnosing(&self) -> bool {
        self.diagnosing.load(Ordering::SeqCst)
    }

    /// Marks diagnostics as going, with `live` for the page; refused during a test run or other
    /// diagnostics.
    pub fn begin_diag(&self, live: diag::run::Live) -> anyhow::Result<()> {
        let _starting = lock(&self.starting);
        anyhow::ensure!(!self.testing(), TESTING);
        anyhow::ensure!(!self.diagnosing(), "diagnostics are already running");
        self.diag_cancel.store(false, Ordering::SeqCst);
        self.diag_stop.store(false, Ordering::SeqCst);
        *lock(&self.diag_reply) = None;
        *lock(&self.diag_live) = Some(live);
        *lock(&self.diag_failed) = None;
        self.diagnosing.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Set before the diagnostics end, so whoever sees them over also sees why they failed.
    pub fn set_diag_failed(&self, error: String) {
        *lock(&self.diag_failed) = Some(error);
    }

    pub fn diag_failed(&self) -> Option<String> {
        lock(&self.diag_failed).clone()
    }

    fn end_diag(&self) {
        *lock(&self.diag_live) = None;
        self.diagnosing.store(false, Ordering::SeqCst);
    }

    /// `end_diag` when dropped: held by the diagnostics thread, so even a panic in its end ends
    /// them.
    pub fn diag_ends(&self) -> DiagEnds<'_> {
        DiagEnds(self)
    }

    /// Refuses a change while diagnostics run: they put back what they found.
    pub fn not_diagnosing(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.diagnosing(), DIAGNOSING);
        Ok(())
    }

    /// Drops a reply sent before the next question: called before the question is shown.
    pub fn clear_diag_reply(&self) {
        *lock(&self.diag_reply) = None;
    }

    /// Also wakes a wait for an answer.
    pub fn cancel_diag(&self) {
        self.diag_cancel.store(true, Ordering::SeqCst);
        self.diag_wake.notify_all();
    }

    pub fn diag_canceled(&self) -> bool {
        self.diag_cancel.load(Ordering::SeqCst)
    }

    /// «Остановить»: the observation ends early and goes on to the search; during the search, no
    /// more paths and the measured ones make the report. Also wakes a wait for an answer.
    pub fn stop_diag(&self) {
        self.diag_stop.store(true, Ordering::SeqCst);
        self.diag_wake.notify_all();
    }

    /// The search stops (the flag stays).
    pub fn diag_stopped(&self) -> bool {
        self.diag_stop.load(Ordering::SeqCst)
    }

    /// The observation stops: the flag is cleared, so the search still runs.
    pub fn take_diag_stop(&self) -> bool {
        self.diag_stop.swap(false, Ordering::SeqCst)
    }

    pub fn diag_live(&self) -> Option<diag::run::Live> {
        lock(&self.diag_live).clone()
    }

    pub fn update_diag_live(&self, f: impl FnOnce(&mut diag::run::Live)) {
        if let Some(live) = lock(&self.diag_live).as_mut() {
            f(live);
        }
    }

    /// From the page: taken by the diagnostics waiting for it, dropped when none waits.
    pub fn diag_reply(&self, reply: Reply) -> anyhow::Result<()> {
        anyhow::ensure!(self.diagnosing(), "no diagnostics are running");
        *lock(&self.diag_reply) = Some(reply);
        self.diag_wake.notify_all();
        Ok(())
    }

    /// The reply `take` accepts, or `None` on cancel or after `timeout`.
    pub fn wait_diag_reply<T>(
        &self,
        timeout: Duration,
        take: impl Fn(&Reply) -> Option<T>,
    ) -> Option<T> {
        let deadline = std::time::Instant::now() + timeout;
        let mut reply = lock(&self.diag_reply);
        loop {
            if self.diag_canceled() || self.diag_stopped() {
                return None;
            }
            if let Some(got) = reply.take().and_then(|r| take(&r)) {
                return Some(got);
            }
            let left = deadline.checked_duration_since(std::time::Instant::now())?;
            reply = self
                .diag_wake
                .wait_timeout(reply, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Runs `work` as the one download, with the download's cancel flag cleared for it.
    pub fn download<T>(
        &self,
        work: impl FnOnce(&AtomicBool) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        // Cleared even when `work` panics.
        struct Done<'a>(&'a AtomicBool);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        anyhow::ensure!(
            self.downloading
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "a download is already going"
        );
        let _done = Done(&self.downloading);
        self.download_cancel.store(false, Ordering::SeqCst);
        work(&self.download_cancel)
    }

    pub fn cancel_download(&self) {
        self.download_cancel.store(true, Ordering::SeqCst);
    }

    /// The queue's turn. A test run holds it from start to end.
    pub fn turn(&self) -> MutexGuard<'_, ()> {
        lock(&self.queue)
    }

    /// A change of the system (a service, hosts): one at a time, never during a test run or
    /// diagnostics.
    pub fn in_turn<T>(&self, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
        anyhow::ensure!(!self.testing(), TESTING);
        self.not_diagnosing()?;
        let _turn = self.turn();
        f()
    }

    /// A change of zapret2: `in_turn`, and only with the zapret2 folder set.
    pub fn change<T>(&self, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
        self.in_turn(|| {
            anyhow::ensure!(
                lock(&self.settings).zapret2_dir.is_some(),
                "the zapret2 folder is not set: choose it in Settings"
            );
            f()
        })
    }

    /// A change that could leave zapret2 half done: the state goes to `pending.json` before the
    /// first step and is put back if any step fails (spec 10). A `pending.json` an earlier
    /// operation left is restored first, and when that fails this one does not start
    /// (`zapret::snapshot::begin`). `operation` is the id `texts::operation` names.
    pub fn change_safely<T>(
        &self,
        operation: &str,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        // The folder is checked before the snapshot: a refusal has nothing to restore.
        self.change(|| self.guarded(operation, f))
    }

    /// `change_safely` without the folder: removing the services needs none.
    pub fn in_turn_safely<T>(
        &self,
        operation: &str,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.in_turn(|| self.guarded(operation, f))
    }

    /// The snapshot around `f`, in the queue's turn.
    fn guarded<T>(
        &self,
        operation: &str,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let pending = pending_path(&self.paths.data());
        let snapshot = zapret::snapshot::begin(operation, &pending)?;
        let result = f();
        if let Err(err) = &result {
            log::error!("{operation} failed, restoring the previous state: {err:#}");
            if let Err(undo) = snapshot.restore() {
                // pending.json stays: the next start or operation tries again.
                return Err(anyhow::anyhow!(
                    "{err:#}\n\nthe previous state was not restored either: {undo:#}"
                ));
            }
        }
        Snapshot::clear(&pending)?;
        result
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

/// Ends the diagnostics when dropped (`AppState::diag_ends`).
pub struct DiagEnds<'a>(&'a AppState);

impl Drop for DiagEnds<'_> {
    fn drop(&mut self) {
        self.0.end_diag();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(dir: &std::path::Path) -> AppState {
        AppState::new(Paths::portable(dir), Settings::default())
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
        let err = state.change(|| Ok(())).unwrap_err();
        assert!(
            format!("{err:#}").contains("zapret2 folder is not set"),
            "{err:#}"
        );
        state.set_settings(Settings {
            zapret2_dir: Some(dir.path().display().to_string()),
            ..Settings::default()
        });
        assert!(state.change(|| Ok(true)).unwrap());
        state.begin_testing().unwrap();
        assert!(
            state.begin_testing().is_err(),
            "a second run must not start"
        );
        let err = state.change(|| Ok(())).unwrap_err();
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
        state.begin_testing().unwrap();
        state.set_live(Some(LiveRun {
            request: Request {
                suite: Suite::Standard,
                strategies: vec!["zapret2:general".into()],
                repeats: 1,
            },
            current: Some(0),
            restoring: false,
            pass: 0,
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

    fn live() -> diag::run::Live {
        diag::run::Live {
            request: diag::run::Request {
                profile: Some("discord".into()),
                app: None,
                mode: diag::run::Mode::Auto,
                observe_secs: None,
                budget_min: None,
                goal: None,
            },
            progress: None,
            question: None,
            choosing: None,
            results: Vec::new(),
        }
    }

    #[test]
    fn diag_blocks_changes() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.set_settings(Settings {
            zapret2_dir: Some(dir.path().display().to_string()),
            ..Settings::default()
        });
        state.begin_diag(live()).unwrap();
        let err = state.change(|| Ok(())).unwrap_err();
        assert_eq!(format!("{err:#}"), DIAGNOSING);
        let err = state.begin_testing().unwrap_err();
        assert_eq!(format!("{err:#}"), DIAGNOSING);
        assert!(
            state.begin_diag(live()).is_err(),
            "a second one must not start"
        );
        // The check `save_settings` and `routing::set_enabled` make.
        let err = state.not_diagnosing().unwrap_err();
        assert_eq!(format!("{err:#}"), DIAGNOSING);
        drop(state.diag_ends());
        assert!(state.not_diagnosing().is_ok());
        assert!(state.change(|| Ok(())).is_ok());
        assert_eq!(state.diag_live(), None);
    }

    #[test]
    fn a_new_diagnostics_run_forgets_why_the_last_one_failed() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.begin_diag(live()).unwrap();
        state.set_diag_failed("WinDivert refused the sniff handle".into());
        drop(state.diag_ends());
        assert_eq!(
            state.diag_failed().as_deref(),
            Some("WinDivert refused the sniff handle")
        );
        state.begin_diag(live()).unwrap();
        assert_eq!(state.diag_failed(), None);
    }

    #[test]
    fn tests_block_diag() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.begin_testing().unwrap();
        let err = state.begin_diag(live()).unwrap_err();
        assert_eq!(format!("{err:#}"), TESTING);
        assert!(!state.diagnosing());
    }

    #[test]
    fn a_reply_reaches_the_wait_and_cancel_ends_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = std::sync::Arc::new(state(dir.path()));
        state.begin_diag(live()).unwrap();
        let choice = |r: &Reply| match r {
            Reply::Choice(keys) => Some(keys.clone()),
            Reply::Answer(_) => None,
        };
        let short = Duration::from_millis(50);
        // A reply sent between the question and the wait is kept.
        state.diag_reply(Reply::Choice(vec!["a".into()])).unwrap();
        assert_eq!(
            state.wait_diag_reply(short, choice),
            Some(vec!["a".to_owned()])
        );
        // One from before the question is cleared with it.
        state.diag_reply(Reply::Choice(vec!["old".into()])).unwrap();
        state.clear_diag_reply();
        assert_eq!(state.wait_diag_reply(short, choice), None);
        // One that comes while waiting wakes the wait.
        let s = state.clone();
        let waiter = std::thread::spawn(move || s.wait_diag_reply(Duration::from_secs(10), choice));
        std::thread::sleep(short);
        state.diag_reply(Reply::Choice(vec!["b".into()])).unwrap();
        assert_eq!(waiter.join().unwrap(), Some(vec!["b".to_owned()]));
        state.cancel_diag();
        assert_eq!(
            state.wait_diag_reply(Duration::from_secs(10), |_| Some(())),
            None
        );
        assert_eq!(
            state.wait_diag_reply(Duration::ZERO, |_| Some(())),
            None,
            "a timeout"
        );
    }

    #[test]
    fn zapret2_is_off_until_its_folder_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        assert_eq!(state.engine2().unwrap(), None);
        let err = state.engine2_required().unwrap_err();
        assert!(
            format!("{err:#}").contains("zapret2 folder is not set"),
            "{err:#}"
        );
        state.set_settings(Settings {
            zapret2_dir: Some(dir.path().display().to_string()),
            ..Settings::default()
        });
        let err = state.engine2_required().unwrap_err();
        let missing = format!("missing {}", zapret2::EXE);
        assert!(format!("{err:#}").contains(&missing), "{err:#}");
    }
}

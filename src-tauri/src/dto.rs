//! What the app layer sends to the page besides the crates' own types, and its events.

use tester::history::RunSummary;
use tester::model::StrategyResult;
use tester::run::{Progress, Request};
use zapret::fakes::Fakes;
use zapret::game_filter::GameFilter;
use zapret::ipset::IpsetMode;
use zapret::service::{QuickCheck, SvcState};
use zapret::user_lists::UserList;

/// The «Обзор» tab, read in one go.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Overview {
    /// `None` until the wizard sets it.
    pub folder: Option<String>,
    /// Why the folder cannot be used.
    pub folder_error: Option<String>,
    pub version: Option<String>,
    /// A newer Flowseal version, once a check found one.
    pub update: Option<String>,
    /// The active engine's service: `zapret`, or `zapret2` once installed (zapret2 spec 3.1).
    pub service_name: String,
    pub service: SvcState,
    pub windivert: SvcState,
    /// That service's process: `winws.exe` or `winws2.exe`.
    pub winws_running: bool,
    pub strategy: Option<String>,
    /// The zapret2 folder and its version, or why it cannot be used.
    pub zapret2: zapret2::Info,
    pub quick: Option<QuickCheck>,
    pub last_run: Option<RunSummary>,
    pub testing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ListState {
    pub list: UserList,
    pub entries: Vec<String>,
}

/// The settings `service.bat` keeps in the folder: the «Zapret» tab.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FlowsealState {
    pub game_filter: GameFilter,
    pub ipset: IpsetMode,
    pub ipset_backup: bool,
    pub check_updates: bool,
    pub fakes: Fakes,
    pub lists: Vec<ListState>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FolderCheck {
    pub path: String,
    /// Empty for a usable Flowseal folder.
    pub missing: Vec<String>,
    pub version: Option<String>,
}

/// What the wizard offers (spec 9.2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Candidates {
    pub folders: Vec<FolderCheck>,
    /// `127.0.0.1:<port>` that answer a SOCKS5 greeting now.
    pub proxies: Vec<String>,
}

/// What the wizard finds before the user picks anything: one folder per engine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Detected {
    pub flowseal: Option<FolderCheck>,
    pub zapret2: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum NoticeLevel {
    Error,
    Info,
}

/// Something the user should see even if the window was closed when it happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Notice {
    pub level: NoticeLevel,
    pub title: String,
    pub text: String,
}

/// The run in progress, for a page opened in the middle of it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LiveRun {
    pub request: Request,
    /// The strategy under test; `None` before the first and while restoring.
    pub current: Option<u32>,
    pub restoring: bool,
    /// The pass under way, from 0, of `request.repeats`.
    pub pass: u32,
    /// Each strategy once, its passes so far aggregated, in the order they first finished.
    pub results: Vec<StrategyResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FolderKind {
    Zapret,
    /// `<zapret2 folder>/strategies`: the user's zapret2 strategies; needs the zapret2 folder to be set.
    Zapret2,
    Logs,
    /// The folder of a strategy's file, by the name the list shows; the path is found here, the
    /// page never sends one. A built-in zapret2 strategy has no file.
    Strategy(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct TestProgress(pub Progress);

/// The run is over and zapret is back; `run_id` is `None` when it failed before a result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct TestsFinished {
    pub run_id: Option<String>,
}

/// zapret, the settings or the history changed: the page reads what it shows again.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChanged;

/// `take_notices` has something.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct NoticesChanged;

/// A release download from the window: bytes so far and the zip's size (0 when GitHub gave none).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct DownloadProgress {
    pub engine: zapret::release::Engine,
    pub received: u32,
    pub total: u32,
}

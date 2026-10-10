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
    pub service: SvcState,
    pub windivert: SvcState,
    pub winws_running: bool,
    pub strategy: Option<String>,
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
    pub results: Vec<StrategyResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FolderKind {
    Zapret,
    Logs,
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

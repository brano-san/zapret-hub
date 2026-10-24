//! What the app layer sends to the page besides the crates' own types, and its events.

use tester::history::RunSummary;
use tester::model::StrategyResult;
use tester::run::{Progress, Request};
use zapret::service::{QuickCheck, SvcState};

/// The «Обзор» tab, read in one go.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Overview {
    /// The engine's service: `zapret2` (zapret2 spec 3.1).
    pub service_name: String,
    pub service: SvcState,
    pub windivert: SvcState,
    /// That service's process: `winws2.exe`.
    pub winws_running: bool,
    pub strategy: Option<String>,
    /// The zapret2 folder and its version, or why it cannot be used.
    pub zapret2: zapret2::Info,
    pub quick: Option<QuickCheck>,
    pub last_run: Option<RunSummary>,
    pub testing: bool,
}

/// SOCKS5 ports that answer, for the wizard (spec 9.2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Candidates {
    /// `127.0.0.1:<port>` that answer a SOCKS5 greeting now.
    pub proxies: Vec<String>,
}

/// What the wizard finds before the user picks anything: the zapret2 folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Detected {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct DiagProgress(pub diag::run::Progress);

/// The diagnostics are over and zapret is back; `id` is the report's, `None` when they failed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct DiagFinished {
    pub id: Option<String>,
}

/// The tray asks the open window to show a tab (a new window gets it as the URL fragment).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct OpenTab {
    pub tab: String,
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

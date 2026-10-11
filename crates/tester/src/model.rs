//! What a run produces and `data/tests/<id>.json` keeps (spec 7.5).

use probe::curl::{CheckResult, DpiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum Suite {
    Standard,
    Dpi,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TargetResult {
    pub name: String,
    /// `None` for a ping-only target.
    pub url: Option<String>,
    /// A profile host `targets.txt` lacks: shown, never counted (spec 7.4).
    pub extra: bool,
    pub checks: Vec<CheckResult>,
    /// ICMP round trip; `None` on timeout.
    pub ping_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DpiTargetResult {
    pub id: String,
    pub provider: String,
    pub country: String,
    pub host: String,
    pub checks: Vec<DpiResult>,
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct ProfileScore {
    pub ok: u32,
    pub total: u32,
    pub median_ms: Option<u32>,
}

/// The counts behind the ranking (spec 7.4). A DPI run fills `ok`, `total`, `fail`, `unsup`,
/// `likely_blocked`.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct Score {
    pub ok: u32,
    /// 3 checks per URL target of `targets.txt`: 36 with Flowseal's file.
    pub total: u32,
    pub errors: u32,
    pub ssl: u32,
    pub unsup: u32,
    /// Median `time_total` of the checks that passed.
    pub median_ms: Option<u32>,
    pub ping_ok: u32,
    pub ping_total: u32,
    pub ping_median_ms: Option<u32>,
    pub fail: u32,
    pub likely_blocked: u32,
    pub discord: ProfileScore,
    pub youtube: ProfileScore,
    pub dbd: ProfileScore,
}

impl Score {
    /// Checks that did not pass: the first ranking key.
    pub fn not_ok(&self) -> u32 {
        self.total.saturating_sub(self.ok)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyResult {
    pub name: String,
    /// SHA-256 of the arguments it ran with: once they change, this result is stale.
    pub args_sha256: String,
    /// The file did not parse, winws did not start or exited: the whole output.
    pub launch_error: Option<String>,
    pub targets: Vec<TargetResult>,
    pub dpi: Vec<DpiTargetResult>,
    pub score: Score,
    /// A run with repeats: every pass as it ran; the fields above are then their aggregate
    /// (`rank::aggregate`). Empty for a single pass and in runs from before repeats.
    #[serde(default)]
    pub passes: Vec<Pass>,
}

/// One pass of a strategy in a run with repeats, as measured.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Pass {
    pub launch_error: Option<String>,
    pub targets: Vec<TargetResult>,
    pub dpi: Vec<DpiTargetResult>,
    pub score: Score,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TestRun {
    /// `2026-10-10_05-03-34`, also the file name.
    pub id: String,
    pub suite: Suite,
    pub flowseal_version: Option<String>,
    /// `winws2.exe --version` when zapret2 strategies ran: the hash of a command line does not see
    /// an engine update (zapret2 spec 7). Runs from before zapret2 have none.
    #[serde(default)]
    pub zapret2_version: Option<String>,
    pub canceled: bool,
    /// What could not be put back after the run.
    pub restore_error: Option<String>,
    /// Best first.
    pub strategies: Vec<StrategyResult>,
}

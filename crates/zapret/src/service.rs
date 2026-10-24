//! The `zapret2` service (and Flowseal's `zapret`, a conflict now): the Service Control Manager on
//! Windows (`windows.rs`), systemd on Linux (`linux.rs`), and the quick check after an install
//! (5.4).

use std::time::{Duration, Instant};

use probe::curl::{CheckKind, CheckStatus};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;

/// Flowseal's service on Windows, bol-van/zapret's unit on Linux: the hub no longer installs it,
/// the system checks offer to remove it.
pub const SERVICE: &str = "zapret";
/// zapret2's service (zapret2 spec 3.1, 6).
pub const SERVICE2: &str = "zapret2";
/// zapret2's engine process: `winws2.exe` on Windows, `nfqws2` on Linux. This crate knows the
/// engine by this name and `SERVICE2` only.
#[cfg(windows)]
pub const WINWS2: &str = "winws2.exe";
#[cfg(target_os = "linux")]
pub const WINWS2: &str = "nfqws2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum SvcState {
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SvcConfig {
    /// The command line: `ImagePath` on Windows, `ExecStart` on Linux.
    pub image_path: String,
    pub auto_start: bool,
}

pub fn state(name: &str) -> SvcState {
    status(name).map_or(SvcState::NotInstalled, |(state, _)| state)
}

/// The engine's service and process (zapret2 spec 3.1).
pub fn active() -> (&'static str, &'static str) {
    (SERVICE2, WINWS2)
}

/// The active strategy: `zapret2:<name>`.
pub fn strategy() -> Option<String> {
    strategy_of(active().0)
}

fn wait_until(max: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < max {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    done()
}

/// `request_delete`, then waits until the service manager forgets the service.
pub fn stop_and_delete(name: &str) -> anyhow::Result<()> {
    if state(name) == SvcState::NotInstalled {
        return Ok(());
    }
    request_delete(name);
    anyhow::ensure!(
        wait_until(Duration::from_secs(10), || state(name)
            == SvcState::NotInstalled),
        "{}",
        still_there(name)
    );
    Ok(())
}

pub fn restart(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        state(name) != SvcState::NotInstalled,
        "service {name} is not installed"
    );
    stop(name)?;
    start(name)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct QuickItem {
    pub name: String,
    pub status: CheckStatus,
    pub time_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct QuickCheck {
    pub ok: bool,
    pub service: SvcState,
    pub winws_running: bool,
    pub checks: Vec<QuickItem>,
}

/// 5.4: the engine's process runs and the first two URL targets answer `HTTP:OK`.
pub fn quick_check() -> QuickCheck {
    let (service, image) = active();
    let service = state(service);
    let winws_running = crate::procs::is_running(image);
    let checks: Vec<QuickItem> = crate::targets::defaults()
        .into_iter()
        .filter_map(|target| Some((target.name, target.url?)))
        .take(2)
        .map(|(name, url)| {
            let result = probe::curl::check(&url, CheckKind::Http, None);
            QuickItem {
                name,
                status: result.status,
                time_ms: result.time_ms,
            }
        })
        .collect();
    let ok =
        winws_running && !checks.is_empty() && checks.iter().all(|c| c.status == CheckStatus::Ok);
    QuickCheck {
        ok,
        service,
        winws_running,
        checks,
    }
}

//! System checks (spec 5.7): Flowseal's `service.bat :service_diagnostics` on Windows
//! (`windows.rs`), the fork's `doctor` on Linux (`linux.rs`), both on the zapret2 folder. Each
//! gets a level; what can be fixed becomes an action behind a button.

use crate::service::{self, SvcState};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::run_all;
#[cfg(windows)]
pub use windows::run_all;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckLevel {
    Ok,
    Warn,
    Problem,
}

/// One list for both systems, so a report names a check the same way everywhere; each system runs
/// its own part of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckId {
    // Windows.
    Bfe,
    Proxy,
    Adguard,
    Killer,
    IntelConnectivity,
    CheckPoint,
    SmartByte,
    CyrillicPath,
    OneDrive,
    WinDivertSys,
    SecureDns,
    WinDivertOrphan,
    // Both.
    TcpTimestamps,
    Vpn,
    HostsYoutube,
    Conflicts,
    // Linux.
    /// The `zapret2` unit is installed but not running.
    ServiceState,
    /// No `nfqws2` in the zapret2 folder.
    Nfqws2Binary,
    /// `nfqws2` is not executable or lacks `cap_net_admin`/`cap_net_raw` (the tests run it as the
    /// user; the service gets them from its unit).
    Nfqws2Caps,
    /// A fake `.bin` of the `bin` folder is empty: a strategy would send an empty packet.
    FakeBins,
    /// A general list is missing or empty: the strategies would match nothing by name.
    Lists,
    /// The service runs, but nothing holds queue 200.
    NfQueue,
    /// The service runs, but the `inet zapret_hub` table or its guards are missing.
    NftTable,
    /// An engine process the service does not own takes packets too.
    ForeignEngines,
    /// Another tool's zapret nftables table filters traffic too.
    ForeignTables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckAction {
    EnableTcpTimestamps,
    RemoveWinDivert,
    RemoveConflicts,
    /// Linux: `nfqws2` made executable and given its network capabilities (`setcap`, as root).
    GrantCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SystemCheck {
    pub id: CheckId,
    pub level: CheckLevel,
    /// What the message names: the proxy address, service names, the folder. Empty when passed.
    pub detail: String,
    /// The fix offered next to a failed check.
    pub action: Option<CheckAction>,
}

/// Other bypasses `service.bat` offers to remove, and Flowseal's own `zapret`: the hub runs
/// zapret2 alone, and two WinDivert bypasses get in each other's way.
#[cfg(windows)]
pub const CONFLICTING: &[&str] = &[
    "GoodbyeDPI",
    "discordfix_zapret",
    "winws1",
    "winws2",
    service::SERVICE,
];
/// bol-van/zapret's own unit: its nfqws fights the hub's for the packets.
#[cfg(target_os = "linux")]
pub const CONFLICTING: &[&str] = &[service::SERVICE];

/// The installed services «Удалить службы» removes.
pub fn conflicts(installed: impl Fn(&str) -> bool) -> Vec<&'static str> {
    CONFLICTING
        .iter()
        .copied()
        .filter(|name| installed(name))
        .collect()
}

fn installed(name: &str) -> bool {
    service::state(name) != SvcState::NotInstalled
}

fn check(
    id: CheckId,
    failed: bool,
    level: CheckLevel,
    detail: String,
    action: Option<CheckAction>,
) -> SystemCheck {
    if failed {
        SystemCheck {
            id,
            level,
            detail,
            action,
        }
    } else {
        SystemCheck {
            id,
            level: CheckLevel::Ok,
            detail: String::new(),
            action: None,
        }
    }
}

fn found(id: CheckId, names: String, level: CheckLevel) -> SystemCheck {
    check(id, !names.is_empty(), level, names, None)
}

/// TCP timestamps off: strategies with `ts` fooling need them.
fn tcp_timestamps() -> SystemCheck {
    check(
        CheckId::TcpTimestamps,
        service::tcp_timestamps() != Some(true),
        CheckLevel::Warn,
        String::new(),
        Some(CheckAction::EnableTcpTimestamps),
    )
}

fn vpn(services: &[(String, String)]) -> SystemCheck {
    found(
        CheckId::Vpn,
        matching_services(services, &["vpn"]).join(", "),
        CheckLevel::Warn,
    )
}

fn hosts_youtube() -> SystemCheck {
    check(
        CheckId::HostsYoutube,
        youtube_in_hosts_file(),
        CheckLevel::Warn,
        String::new(),
        None,
    )
}

fn conflicting_services() -> SystemCheck {
    let conflicts = conflicts(installed);
    check(
        CheckId::Conflicts,
        !conflicts.is_empty(),
        CheckLevel::Problem,
        conflicts.join(", "),
        Some(CheckAction::RemoveConflicts),
    )
}

/// The button next to a failed check; `zapret2` is the zapret2 folder setting. As in `service.bat`, a delete that fails is reported and
/// the ones after it are still tried.
pub fn fix(action: CheckAction, zapret2: Option<&std::path::Path>) -> anyhow::Result<()> {
    #[cfg(windows)]
    let _ = zapret2; // every Windows fix is system-wide
    match action {
        CheckAction::EnableTcpTimestamps => service::enable_tcp_timestamps(),
        CheckAction::RemoveConflicts => remove_conflicts(),
        #[cfg(windows)]
        CheckAction::RemoveWinDivert => windows::remove_windivert(),
        #[cfg(target_os = "linux")]
        CheckAction::GrantCapabilities => linux::grant_capabilities(zapret2),
        #[allow(unreachable_patterns)]
        other => anyhow::bail!("{other:?} does not apply to this system"),
    }
}

fn remove_conflicts() -> anyhow::Result<()> {
    let mut failed = Vec::new();
    for name in conflicts(installed) {
        if let Err(err) = service::stop_and_delete(name) {
            failed.push(format!("{name}: {err:#}"));
        }
    }
    // As in `service.bat`: a running winws keeps WinDivert until it stops.
    #[cfg(windows)]
    {
        service::request_delete("WinDivert");
        service::request_delete("WinDivert14");
    }
    anyhow::ensure!(failed.is_empty(), "{}", failed.join("; "));
    Ok(())
}

/// Names of the services whose name or display name holds every word: `sc query | findstr`
/// matches one line at a time.
pub fn matching_services(services: &[(String, String)], words: &[&str]) -> Vec<String> {
    services
        .iter()
        .filter(|(name, display)| {
            [name, display].iter().any(|text| {
                let text = text.to_lowercase();
                words.iter().all(|word| text.contains(word))
            })
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// `[а-яА-ЯёЁ]`
pub fn has_cyrillic(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// `echo %~dp0\ | findstr /I /C:"%OneDrive%\\"`
pub fn in_onedrive(root_dp0: &str, onedrive: Option<&str>) -> bool {
    onedrive.is_some_and(|dir| {
        let dir = dir.trim_end_matches('\\');
        !dir.is_empty()
            && root_dp0
                .to_lowercase()
                .contains(&format!("{dir}\\").to_lowercase())
    })
}

/// `findstr /I "youtube.com"` and `"youtu.be"` over the raw bytes of a hosts file: the names are
/// ASCII, so they are found whatever encoding the rest of the file is in.
pub fn hosts_mentions_youtube(hosts: &[u8]) -> bool {
    let text = String::from_utf8_lossy(hosts).to_ascii_lowercase();
    text.contains("youtube.com") || text.contains("youtu.be")
}

/// As `service.bat`: `if exist` first, so a missing file has no entries; a file that cannot be
/// read has none either (`findstr` fails and the check stays quiet), but the journal says why.
fn youtube_in_hosts_file() -> bool {
    let path = crate::hosts::path();
    match std::fs::read(&path) {
        Ok(bytes) => hosts_mentions_youtube(&bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            log::warn!("hosts file {} was not read: {err}", path.display());
            false
        }
    }
}

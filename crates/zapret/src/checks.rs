//! The checks of `service.bat :service_diagnostics` (spec 5.7). Each gets a level; what the
//! original fixes on its own becomes an action behind a button.

use crate::folder::Folder;
use crate::service::{self, SvcState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckLevel {
    Ok,
    Warn,
    Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckId {
    Bfe,
    Proxy,
    TcpTimestamps,
    Adguard,
    Killer,
    IntelConnectivity,
    CheckPoint,
    SmartByte,
    CyrillicPath,
    OneDrive,
    WinDivertSys,
    Vpn,
    SecureDns,
    HostsYoutube,
    WinDivertOrphan,
    Conflicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckAction {
    EnableTcpTimestamps,
    RemoveWinDivert,
    RemoveConflicts,
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

/// Other bypasses `service.bat` offers to remove.
pub const CONFLICTING: [&str; 4] = ["GoodbyeDPI", "discordfix_zapret", "winws1", "winws2"];

pub fn run_all(folder: &Folder) -> Vec<SystemCheck> {
    use CheckLevel::{Problem, Warn};
    let services = service::active_services();
    let find = |words: &[&str]| matching_services(&services, words).join(", ");
    let root = folder.root_dp0();
    let proxy = proxy_server();
    let hosts = crate::hosts::read().unwrap_or_default().to_lowercase();
    let windivert = service::state("WinDivert");
    let orphan = matches!(windivert, SvcState::Running | SvcState::StopPending)
        && !crate::procs::is_running("winws.exe");
    let conflicts: Vec<&str> = CONFLICTING
        .into_iter()
        .filter(|name| service::state(name) != SvcState::NotInstalled)
        .collect();
    let checkpoint: Vec<String> = [find(&["tracsrvwrapper"]), find(&["epwd"])]
        .into_iter()
        .filter(|names| !names.is_empty())
        .collect();
    vec![
        check(
            CheckId::Bfe,
            service::state("BFE") != SvcState::Running,
            Problem,
            String::new(),
            None,
        ),
        check(
            CheckId::Proxy,
            proxy.is_some(),
            Warn,
            proxy.unwrap_or_default(),
            None,
        ),
        check(
            CheckId::TcpTimestamps,
            service::tcp_timestamps() != Some(true),
            Warn,
            String::new(),
            Some(CheckAction::EnableTcpTimestamps),
        ),
        check(
            CheckId::Adguard,
            crate::procs::is_running("AdguardSvc.exe"),
            Problem,
            String::new(),
            None,
        ),
        found(CheckId::Killer, find(&["killer"]), Problem),
        found(
            CheckId::IntelConnectivity,
            find(&["intel", "connectivity", "network"]),
            Problem,
        ),
        found(CheckId::CheckPoint, checkpoint.join(", "), Problem),
        found(CheckId::SmartByte, find(&["smartbyte"]), Problem),
        check(
            CheckId::CyrillicPath,
            has_cyrillic(&root),
            Warn,
            root.clone(),
            None,
        ),
        check(
            CheckId::OneDrive,
            in_onedrive(&root, std::env::var("OneDrive").ok().as_deref()),
            Problem,
            root.clone(),
            None,
        ),
        check(
            CheckId::WinDivertSys,
            !has_driver(folder),
            Problem,
            folder.bin().display().to_string(),
            None,
        ),
        found(CheckId::Vpn, find(&["vpn"]), Warn),
        check(
            CheckId::SecureDns,
            !doh_configured(),
            Warn,
            String::new(),
            None,
        ),
        check(
            CheckId::HostsYoutube,
            hosts.contains("youtube.com") || hosts.contains("youtu.be"),
            Warn,
            String::new(),
            None,
        ),
        check(
            CheckId::WinDivertOrphan,
            orphan,
            Warn,
            format!("{windivert:?}"),
            Some(CheckAction::RemoveWinDivert),
        ),
        check(
            CheckId::Conflicts,
            !conflicts.is_empty(),
            Problem,
            conflicts.join(", "),
            Some(CheckAction::RemoveConflicts),
        ),
    ]
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

/// The button next to a failed check: what `service.bat` does for it.
pub fn fix(action: CheckAction) -> anyhow::Result<()> {
    match action {
        CheckAction::EnableTcpTimestamps => service::enable_tcp_timestamps(),
        CheckAction::RemoveWinDivert => service::stop_and_delete("WinDivert").or_else(|first| {
            // A GoodbyeDPI service keeps WinDivert loaded: it goes, then WinDivert once more.
            if service::state("GoodbyeDPI") == SvcState::NotInstalled {
                return Err(first);
            }
            service::stop_and_delete("GoodbyeDPI")?;
            service::stop_and_delete("WinDivert")
        }),
        CheckAction::RemoveConflicts => {
            for name in CONFLICTING {
                service::stop_and_delete(name)?;
            }
            // As in `service.bat`: a running winws keeps WinDivert until it stops.
            service::request_delete("WinDivert");
            service::request_delete("WinDivert14");
            Ok(())
        }
    }
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

fn has_driver(folder: &Folder) -> bool {
    std::fs::read_dir(folder.bin()).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sys"))
        })
    })
}

/// `ProxyServer` when `ProxyEnable` is 1 in the user's Internet Settings.
fn proxy_server() -> Option<String> {
    let key = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    (key.get_u32("ProxyEnable").ok()? == 1)
        .then(|| key.get_string("ProxyServer").unwrap_or_default())
}

/// A `DohFlags` above zero anywhere under the DNS client's per-interface settings.
fn doh_configured() -> bool {
    fn search(key: &windows_registry::Key, depth: u8) -> bool {
        key.get_u64("DohFlags").is_ok_and(|flags| flags > 0)
            || (depth > 0
                && key.keys().is_ok_and(|mut names| {
                    names.any(|name| key.open(&name).is_ok_and(|sub| search(&sub, depth - 1)))
                }))
    }
    windows_registry::LOCAL_MACHINE
        .open(r"SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters")
        .is_ok_and(|key| search(&key, 6))
}

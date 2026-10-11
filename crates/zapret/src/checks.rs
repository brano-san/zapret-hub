//! The checks of `service.bat :service_diagnostics` (spec 5.7). Each gets a level; what the
//! original fixes on its own becomes an action behind a button.

use crate::folder::Folder;
use crate::service::{self, SERVICE, SERVICE2, SvcState, WINWS2};

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

/// The installed services «Удалить службы» removes: the other bypasses, and `zapret2` while `zapret`
/// is installed too (zapret2 spec 9). `service.bat` knows nothing of zapret2 and may have put
/// `zapret` next to it: the user's newer step outside the app wins.
pub fn conflicts(installed: impl Fn(&str) -> bool) -> Vec<&'static str> {
    let zapret2 = installed(SERVICE).then_some(SERVICE2);
    CONFLICTING
        .into_iter()
        .chain(zapret2)
        .filter(|name| installed(name))
        .collect()
}

fn installed(name: &str) -> bool {
    service::state(name) != SvcState::NotInstalled
}

pub fn run_all(folder: &Folder) -> Vec<SystemCheck> {
    use CheckLevel::{Problem, Warn};
    let services = service::active_services();
    let find = |words: &[&str]| matching_services(&services, words).join(", ");
    let root = folder.root_dp0();
    let proxy = proxy_server();
    let windivert = service::state("WinDivert");
    // winws2 holds WinDivert too: removing the driver under it would break zapret2 (spec 9).
    let orphan = matches!(windivert, SvcState::Running | SvcState::StopPending)
        && !crate::procs::is_running("winws.exe")
        && !crate::procs::is_running(WINWS2);
    let conflicts = conflicts(installed);
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
            youtube_in_hosts_file(),
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

/// The button next to a failed check: what `service.bat` does for it. As there, a delete that
/// fails is reported and the ones after it are still tried.
pub fn fix(action: CheckAction) -> anyhow::Result<()> {
    match action {
        CheckAction::EnableTcpTimestamps => service::enable_tcp_timestamps(),
        CheckAction::RemoveWinDivert => remove_windivert(),
        CheckAction::RemoveConflicts => remove_conflicts(),
    }
}

fn remove_windivert() -> anyhow::Result<()> {
    let Err(first) = service::stop_and_delete("WinDivert") else {
        return Ok(());
    };
    // A GoodbyeDPI service keeps WinDivert loaded: it goes, then WinDivert once more, whether or
    // not GoodbyeDPI went.
    if service::state("GoodbyeDPI") == SvcState::NotInstalled {
        return Err(first);
    }
    let goodbye = service::stop_and_delete("GoodbyeDPI").err();
    if let Some(err) = &goodbye {
        log::warn!(target: "exec", "GoodbyeDPI was left in place: {err:#}");
    }
    service::stop_and_delete("WinDivert").map_err(|err| match goodbye {
        Some(goodbye) => anyhow::anyhow!("{err:#}; GoodbyeDPI: {goodbye:#}"),
        None => err,
    })
}

fn remove_conflicts() -> anyhow::Result<()> {
    let mut failed = Vec::new();
    for name in conflicts(installed) {
        if let Err(err) = service::stop_and_delete(name) {
            failed.push(format!("{name}: {err:#}"));
        }
    }
    // As in `service.bat`: a running winws keeps WinDivert until it stops.
    service::request_delete("WinDivert");
    service::request_delete("WinDivert14");
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

//! The checks of Flowseal's `service.bat :service_diagnostics` (spec 5.7), on the zapret2 folder.
//! Each gets a level; what the original fixes on its own becomes an action behind a button.

use std::path::Path;

use crate::service::{self, SERVICE, SvcState, WINWS2};

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

/// Other bypasses `service.bat` offers to remove, and Flowseal's own `zapret`: the hub runs
/// zapret2 alone, and two WinDivert bypasses get in each other's way.
pub const CONFLICTING: [&str; 5] = [
    "GoodbyeDPI",
    "discordfix_zapret",
    "winws1",
    "winws2",
    SERVICE,
];

/// The installed services «Удалить службы» removes.
pub fn conflicts(installed: impl Fn(&str) -> bool) -> Vec<&'static str> {
    CONFLICTING
        .into_iter()
        .filter(|name| installed(name))
        .collect()
}

fn installed(name: &str) -> bool {
    service::state(name) != SvcState::NotInstalled
}

/// `zapret2` is the zapret2 folder setting: the checks of its path and its driver pass without one.
pub fn run_all(zapret2: Option<&Path>) -> Vec<SystemCheck> {
    use CheckLevel::{Problem, Warn};
    let services = service::active_services();
    let find = |words: &[&str]| matching_services(&services, words).join(", ");
    // `%~dp0`: the folder with a trailing backslash.
    let root = zapret2.map(|dir| format!("{}\\", dir.display().to_string().trim_end_matches('\\')));
    let root = root.as_deref();
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
            root.is_some_and(has_cyrillic),
            Warn,
            root.unwrap_or_default().to_owned(),
            None,
        ),
        check(
            CheckId::OneDrive,
            root.is_some_and(|root| in_onedrive(root, std::env::var("OneDrive").ok().as_deref())),
            Problem,
            root.unwrap_or_default().to_owned(),
            None,
        ),
        check(
            CheckId::WinDivertSys,
            zapret2.is_some_and(|dir| !has_driver(dir, 3)),
            Problem,
            zapret2
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
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

/// A `WinDivert64.sys` in `dir` or a folder up to `depth` levels below it: next to `winws2.exe`,
/// wherever the release keeps it (`binaries\windows-x86_64`, `zapret-winws`, `bin`).
fn has_driver(dir: &Path, depth: u8) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                depth > 0 && has_driver(&path, depth - 1)
            } else {
                path.file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("WinDivert64.sys"))
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_driver_is_found_where_a_release_keeps_it() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("binaries").join("windows-x86_64");
        std::fs::create_dir_all(&deep).unwrap();
        assert!(!has_driver(tmp.path(), 3));
        std::fs::write(deep.join("WinDivert64.sys"), "").unwrap();
        assert!(has_driver(tmp.path(), 3));
        assert!(!has_driver(tmp.path(), 0), "not deeper than asked");
    }
}

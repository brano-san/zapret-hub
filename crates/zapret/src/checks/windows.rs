//! Windows: the checks of Flowseal's `service.bat :service_diagnostics`, on the zapret2 folder.

use std::path::Path;

use super::{
    CheckAction, CheckId, CheckLevel, SystemCheck, check, conflicting_services, found,
    has_cyrillic, hosts_youtube, in_onedrive, matching_services, tcp_timestamps, vpn,
};
use crate::service::{self, SvcState, WINWS2};

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
        tcp_timestamps(),
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
        vpn(&services),
        check(
            CheckId::SecureDns,
            !doh_configured(),
            Warn,
            String::new(),
            None,
        ),
        hosts_youtube(),
        check(
            CheckId::WinDivertOrphan,
            orphan,
            Warn,
            format!("{windivert:?}"),
            Some(CheckAction::RemoveWinDivert),
        ),
        conflicting_services(),
    ]
}

pub(super) fn remove_windivert() -> anyhow::Result<()> {
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

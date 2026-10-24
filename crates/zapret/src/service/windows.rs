//! Windows: state through the Service Control Manager, changes through `sc`/`net` as
//! `service.bat` makes them.

use std::process::Command;
use std::time::Duration;

use super::{SERVICE, SERVICE2, SvcConfig, SvcState, WINWS2, state, stop_and_delete};

use windows::Win32::System::Services::{
    CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW, OpenSCManagerW,
    OpenServiceW, QueryServiceStatusEx, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_ACTIVE, SERVICE_AUTO_START,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS_PROCESS,
    SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_WIN32,
};
use windows::core::{HSTRING, PCWSTR};

const STRATEGY_VALUE: &str = "zapret-discord-youtube";

struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW/OpenServiceW and is closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn manager() -> Option<Handle> {
    // SAFETY: plain call; the handle is wrapped for closing.
    unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
        .ok()
        .map(Handle)
    }
}

fn open(name: &str, access: u32) -> Option<Handle> {
    let scm = manager()?;
    // SAFETY: `scm` is open; the service handle is wrapped for closing.
    unsafe {
        OpenServiceW(scm.0, &HSTRING::from(name), access)
            .ok()
            .map(Handle)
    }
}

/// State and process id; `None` when the service does not exist.
pub fn status(name: &str) -> Option<(SvcState, u32)> {
    let service = open(name, SERVICE_QUERY_STATUS)?;
    let mut buf = vec![0u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: the buffer has the size of the structure asked for.
    unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            Some(&mut buf),
            &mut needed,
        )
    }
    .ok()?;
    // SAFETY: the call above filled one SERVICE_STATUS_PROCESS.
    let status = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<SERVICE_STATUS_PROCESS>()) };
    let state = match status.dwCurrentState {
        SERVICE_STOPPED => SvcState::Stopped,
        SERVICE_START_PENDING => SvcState::StartPending,
        SERVICE_STOP_PENDING => SvcState::StopPending,
        SERVICE_RUNNING => SvcState::Running,
        _ => SvcState::Other,
    };
    Some((state, status.dwProcessId))
}

fn key(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}

/// `ImagePath` and `Start` from the service's registry key (5.1): what `sc qc` shows. Not from
/// `QueryServiceConfigW`: its RPC reply is capped at 8 KB, and the `ImagePath` of `zapret`
/// carries a strategy's whole argument list, which can pass 4000 characters. `None` when the key
/// or its `ImagePath` is missing.
pub fn config(name: &str) -> Option<SvcConfig> {
    let key = windows_registry::LOCAL_MACHINE.open(key(name)).ok()?;
    Some(SvcConfig {
        image_path: key.get_string("ImagePath").ok()?,
        auto_start: key
            .get_u32("Start")
            .is_ok_and(|start| start == SERVICE_AUTO_START.0),
    })
}

/// Name and display name of every running Win32 service: what a bare `sc query` lists.
pub fn active_services() -> Vec<(String, String)> {
    let Some(scm) = manager() else {
        return Vec::new();
    };
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: a size query with no buffer.
    let _ = unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_ACTIVE,
            None,
            &mut needed,
            &mut count,
            None,
            PCWSTR::null(),
        )
    };
    let mut buf = vec![0u8; needed as usize + 4096];
    // SAFETY: `buf` is larger than the size asked for.
    let listed = unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_ACTIVE,
            Some(&mut buf),
            &mut needed,
            &mut count,
            None,
            PCWSTR::null(),
        )
    };
    if listed.is_err() {
        return Vec::new();
    }
    let item = size_of::<ENUM_SERVICE_STATUS_PROCESSW>();
    (0..count as usize)
        .map(|i| {
            // SAFETY: the call wrote `count` entries at the start of `buf`; their strings point into it.
            unsafe {
                let entry = std::ptr::read_unaligned(
                    buf.as_ptr()
                        .add(i * item)
                        .cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                );
                (
                    entry.lpServiceName.to_string().unwrap_or_default(),
                    entry.lpDisplayName.to_string().unwrap_or_default(),
                )
            }
        })
        .collect()
}

/// The strategy name recorded next to service `name`: `service.bat` writes it for `zapret`, the app
/// writes `zapret2:<name>` for `zapret2` (zapret2 spec 6).
pub fn strategy_of(name: &str) -> Option<String> {
    windows_registry::LOCAL_MACHINE
        .open(key(name))
        .ok()?
        .get_string(STRATEGY_VALUE)
        .ok()
}

pub fn set_strategy_of(service: &str, name: &str) -> anyhow::Result<()> {
    let key = key(service);
    windows_registry::LOCAL_MACHINE
        .create(&key)?
        .set_string(STRATEGY_VALUE, name)?;
    log::info!(target: "exec", r"registry: HKLM\{key} {STRATEGY_VALUE} = {name}");
    Ok(())
}

fn cmd(program: &str, args: &[&str]) -> anyhow::Result<exec::Output> {
    Ok(exec::run(
        Command::new(program).args(args),
        Duration::from_secs(60),
        None,
    )?)
}

/// `sc create <name> binPath= ... DisplayName= <name> start= auto|demand` and the description, as
/// `service.bat` makes `zapret` (zapret2 spec 6).
pub fn create(name: &str, bin_path: &str, auto_start: bool) -> anyhow::Result<()> {
    let start = if auto_start { "auto" } else { "demand" };
    cmd(
        "sc",
        &[
            "create",
            name,
            "binPath=",
            bin_path,
            "DisplayName=",
            name,
            "start=",
            start,
        ],
    )?
    .check()?;
    let _ = cmd("sc", &["description", name, "zapret2 DPI bypass software"]);
    Ok(())
}

/// `net stop` + `sc delete` with errors ignored, as `service.bat` does where it does not care.
pub fn request_delete(name: &str) {
    let _ = cmd("net", &["stop", name]);
    let _ = cmd("sc", &["delete", name]);
}

/// Why `stop_and_delete` gave up.
pub(super) fn still_there(name: &str) -> String {
    format!(
        "service {name} is still there after sc delete (marked for deletion?): close services.msc and try again"
    )
}

/// `:service_remove`: zapret and zapret2, every winws and winws2, WinDivert, WinDivert14. Only the
/// services have to go. As in `service.bat` the rest is best effort: a process may end by itself,
/// and a WinDivert driver that another program holds stays; their errors go to the journal as
/// warnings.
pub fn remove() -> anyhow::Result<()> {
    stop_and_delete(SERVICE)?;
    stop_and_delete(SERVICE2)?;
    for image in ["winws.exe", WINWS2] {
        if let Err(err) = crate::procs::kill_all(image) {
            log::warn!(target: "exec", "{image} was not ended: {err:#}");
        }
    }
    for driver in ["WinDivert", "WinDivert14"] {
        if let Err(err) = stop_and_delete(driver) {
            log::warn!(target: "exec", "{driver} was left in place: {err:#}");
        }
    }
    Ok(())
}

pub fn start(name: &str) -> anyhow::Result<()> {
    if state(name) != SvcState::Running {
        cmd("net", &["start", name])?.check()?;
    }
    Ok(())
}

pub fn stop(name: &str) -> anyhow::Result<()> {
    if matches!(
        state(name),
        SvcState::Running | SvcState::StartPending | SvcState::Other
    ) {
        cmd("net", &["stop", name])?.check()?;
    }
    Ok(())
}

/// `netsh interface tcp show global`, the `RFC 1323 Timestamps` line. Labels are translated, the
/// RFC number and the value are not. `None` when the line is missing.
pub fn tcp_timestamps() -> Option<bool> {
    parse_timestamps(
        &cmd("netsh", &["interface", "tcp", "show", "global"])
            .ok()?
            .stdout,
    )
}

pub fn parse_timestamps(text: &str) -> Option<bool> {
    let line = text.lines().find(|line| line.contains("1323"))?;
    Some(
        line.rsplit(':')
            .next()?
            .trim()
            .eq_ignore_ascii_case("enabled"),
    )
}

pub fn enable_tcp_timestamps() -> anyhow::Result<()> {
    cmd(
        "netsh",
        &["interface", "tcp", "set", "global", "timestamps=enabled"],
    )?
    .check()?;
    Ok(())
}

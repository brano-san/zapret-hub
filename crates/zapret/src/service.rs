//! The `zapret` Windows service: state through the Service Control Manager, changes through
//! `sc`/`net` exactly as `service.bat` makes them, and the quick check after an install (5.4).

use std::process::Command;
use std::time::{Duration, Instant};

use probe::curl::{CheckKind, CheckStatus};
use windows::Win32::System::Services::{
    CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW, OpenSCManagerW,
    OpenServiceW, QueryServiceStatusEx, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_ACTIVE, SERVICE_AUTO_START,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS_PROCESS,
    SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_WIN32,
};
use windows::core::{HSTRING, PCWSTR};

use crate::batparse::ParsedArgs;
use crate::folder::Folder;

pub const SERVICE: &str = "zapret";
/// zapret2's service and process (zapret2 spec 3.1, 6): this crate knows the second engine by
/// these two names only.
pub const SERVICE2: &str = "zapret2";
pub const WINWS2: &str = "winws2.exe";
const STRATEGY_VALUE: &str = "zapret-discord-youtube";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum SvcState {
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    Other,
}

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

pub fn state(name: &str) -> SvcState {
    status(name).map_or(SvcState::NotInstalled, |(state, _)| state)
}

/// The installed engine's service and process (zapret2 spec 3.1): `zapret2` and `winws2.exe` while
/// the `zapret2` service exists, else Flowseal's. The app installs one engine at a time; when
/// `service.bat` puts `zapret` next to `zapret2`, the system checks name the conflict (spec 9).
pub fn active() -> (&'static str, &'static str) {
    if state(SERVICE2) == SvcState::NotInstalled {
        (SERVICE, "winws.exe")
    } else {
        (SERVICE2, WINWS2)
    }
}

fn key(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SvcConfig {
    pub image_path: String,
    pub auto_start: bool,
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

/// The active engine's strategy: a Flowseal name or `zapret2:<name>`.
pub fn strategy() -> Option<String> {
    strategy_of(active().0)
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

/// `sc create <name> binPath= ... DisplayName= <name> start= auto|demand` and the description, as
/// `service.bat` makes `zapret`; `zapret2` the same way (zapret2 spec 6).
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
    let description = if name == SERVICE {
        "Zapret DPI bypass software"
    } else {
        "zapret2 DPI bypass software"
    };
    let _ = cmd("sc", &["description", name, description]);
    Ok(())
}

/// `:service_install` after the menu: timestamps on, the old service gone, the new one created
/// and started, the strategy name recorded. One engine at a time (zapret2 spec 6): zapret2 goes
/// too, so every path that installs a Flowseal strategy takes it down.
pub fn install(folder: &Folder, name: &str, parsed: &ParsedArgs) -> anyhow::Result<()> {
    let _ = enable_tcp_timestamps();
    stop_and_delete(SERVICE)?;
    stop_and_delete(SERVICE2)?;
    crate::procs::kill_all(WINWS2)?;
    create(SERVICE, &parsed.bin_path(&folder.winws()), true)?;
    cmd("sc", &["start", SERVICE])?.check()?;
    set_strategy_of(SERVICE, name)
}

/// `net stop` + `sc delete` with errors ignored, as `service.bat` does where it does not care.
pub fn request_delete(name: &str) {
    let _ = cmd("net", &["stop", name]);
    let _ = cmd("sc", &["delete", name]);
}

/// `request_delete`, then waits until the SCM forgets the service.
pub fn stop_and_delete(name: &str) -> anyhow::Result<()> {
    if state(name) == SvcState::NotInstalled {
        return Ok(());
    }
    request_delete(name);
    anyhow::ensure!(
        wait_until(Duration::from_secs(10), || state(name)
            == SvcState::NotInstalled),
        "service {name} is still there after sc delete (marked for deletion?): close services.msc and try again"
    );
    Ok(())
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

pub fn restart(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        state(name) != SvcState::NotInstalled,
        "service {name} is not installed"
    );
    stop(name)?;
    start(name)
}

/// After a list or ipset change: winws reads its files at start.
pub fn restart_if_running(name: &str) -> anyhow::Result<()> {
    if state(name) == SvcState::Running {
        restart(name)
    } else {
        Ok(())
    }
}

/// The arguments live in the service: after a game filter change the active strategy is
/// installed again. A service without a recorded strategy cannot be: that is an error, so the
/// change is undone instead of reported as done.
pub fn reinstall_active(folder: &Folder) -> anyhow::Result<()> {
    if state(SERVICE) == SvcState::NotInstalled {
        return Ok(());
    }
    let Some(name) = strategy_of(SERVICE) else {
        anyhow::bail!(
            "the zapret service has no strategy name in the registry, so it cannot be installed again with the new game filter: apply a strategy first"
        );
    };
    let file = crate::strategies::find(folder, &name)?;
    install(
        folder,
        &file.name,
        &crate::batparse::parse_file(folder, &file.path)?,
    )
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

/// 5.4: the active engine's process runs and the first two URL targets answer `HTTP:OK`.
/// The first two targets of the Flowseal folder's `targets.txt`, or of the built-in list without
/// one (zapret2 alone).
pub fn quick_check(folder: Option<&Folder>) -> QuickCheck {
    let (service, image) = active();
    let service = state(service);
    let winws_running = crate::procs::is_running(image);
    let checks: Vec<QuickItem> = folder
        .map_or_else(crate::targets::defaults, crate::targets::load)
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

/// «Применить»: install `name` as the service, wait 2 s, run the quick check.
pub fn apply(folder: &Folder, name: &str) -> anyhow::Result<QuickCheck> {
    let file = crate::strategies::find(folder, name)?;
    let parsed = crate::batparse::parse_file(folder, &file.path)?;
    install(folder, &file.name, &parsed)?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(quick_check(Some(folder)))
}

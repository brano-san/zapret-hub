//! The router in the app (router spec 3, 7): its setup from the settings, the switch, the pauses
//! for what changes the WinDivert driver under it, and on Linux the capabilities and the cleanup.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use router::divert::pick_dll;
use router::{RouterState, RouterStatus, Setup, parse_proxy};
use tauri::AppHandle;
use zapret::checks::{CheckId, CheckLevel, SystemCheck};

use crate::actions::{self, state};
use crate::settings::Settings;
use crate::state::{AppState, lock};

/// zapret2's DLL (spec 3.1; Windows, Linux routes with nftables) and the proxy from the settings.
pub fn setup(state: &AppState) -> anyhow::Result<Setup> {
    let proxy = parse_proxy(state.settings().socks5.as_deref())?;
    let dll = if cfg!(windows) {
        dll(state)?
    } else {
        PathBuf::new()
    };
    Ok(Setup { dll, proxy })
}

/// Linux: gives the hub's exe the capabilities routing needs (one root step); they take effect
/// on its next start.
pub fn grant() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let exe = std::env::current_exe()?;
        router::linux::caps::grant(&exe)?;
        log::info!("routing capabilities granted to {}", exe.display());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("only Linux routing needs capabilities")
}

/// Before anything routes: what a crashed session left (Linux) goes, so rule traffic is never
/// sent to a dead acceptor.
pub fn clear_stale() {
    if router::clear_stale() {
        log::info!("cleared the routing a previous session left");
    }
}

/// On the way out (`RunEvent::Exit`, a signal): Linux routing is the system's table and rules,
/// which outlive the process; Windows' handles close with it.
pub fn stop_on_exit(state: &AppState) {
    if cfg!(target_os = "linux") {
        // The app is ending: nothing to tell the window or the tray.
        state.router.on_change(|| {});
        state.router.stop();
        log::info!("routing stopped on exit");
    }
}

/// zapret2's WinDivert DLL; a zapret2 folder that does not open counts as none.
pub fn dll(state: &AppState) -> anyhow::Result<PathBuf> {
    let engine2 = state.engine2().ok().flatten();
    pick_dll(
        engine2.as_ref().and_then(|engine| engine.exe().parent()),
        Path::is_file,
    )
}

/// The router as `router_enabled` says, with the DLL and the proxy in force now. A failed start
/// is already in the router's status.
pub fn sync(app: &AppHandle) {
    let state = state(app);
    if !state.settings().router_enabled {
        state.router.stop();
        return;
    }
    match setup(&state) {
        Ok(setup) => {
            let _ = state.router.start(setup);
        }
        Err(err) => state.router.fail(format!("{err:#}")),
    }
}

/// The only writer of `router_enabled`. Turning on starts the router first: when that fails the
/// setting stays off (spec 6) and so does the router.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> anyhow::Result<RouterStatus> {
    let state = state(app);
    // Diagnostics drive the router themselves and put it back as they found it.
    state.not_diagnosing()?;
    let saved = {
        let _saving = state.saving();
        let switched = if enabled {
            setup(&state).and_then(|setup| state.router.start(setup))
        } else {
            state.router.stop();
            Ok(())
        };
        let settings = Settings {
            router_enabled: enabled,
            ..state.settings()
        };
        let saved =
            switched.and_then(|()| crate::settings::save(&state.paths.settings(), &settings));
        if saved.is_err() && enabled {
            state.router.stop();
        }
        // In memory the setting follows the router even when the file write failed: a stopped
        // router left `true` there would come back on the next `sync`.
        state.set_settings(Settings {
            router_enabled: enabled && saved.is_ok(),
            ..settings
        });
        saved
    };
    actions::changed(app);
    saved.map(|()| state.router.status())
}

/// Pauses going at once: a test run and an engine switch may overlap, and routing comes back only
/// when the last one ends.
static PAUSES: Mutex<usize> = Mutex::new(0);

/// `f` with routing paused (spec 7): its handles are closed, so the driver and the engines can
/// change under it. On every way out of `f`, a panic included, the router takes the DLL of the
/// engine running then and, once the last pause ends, resumes.
pub fn paused<T>(app: &AppHandle, f: impl FnOnce() -> T) -> T {
    struct Resume<'a>(&'a AppHandle);
    impl Drop for Resume<'_> {
        fn drop(&mut self) {
            let mut pauses = lock(&PAUSES);
            *pauses -= 1;
            sync(self.0);
            if *pauses == 0 {
                state(self.0).router.resume();
            }
        }
    }
    {
        let mut pauses = lock(&PAUSES);
        *pauses += 1;
        state(app).router.pause();
    }
    let _resume = Resume(app);
    f()
}

/// While routing runs, the hub's own WinDivert handles keep the driver loaded: that is no orphan
/// driver, and removing it would break routing (spec 11).
pub fn hide_own_driver(checks: &mut [SystemCheck], router: &RouterState) {
    if *router != RouterState::Running {
        return;
    }
    for check in checks
        .iter_mut()
        .filter(|check| check.id == CheckId::WinDivertOrphan)
    {
        check.level = CheckLevel::Ok;
        check.detail.clear();
        check.action = None;
    }
}

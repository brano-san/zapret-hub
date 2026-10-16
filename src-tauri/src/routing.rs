//! The router in the app (router spec 3, 7): its setup from the settings, the switch, and the
//! pauses for what changes the WinDivert driver under it.

use std::path::Path;
use std::sync::Mutex;

use router::divert::{Engine, pick_dll};
use router::{RouterState, RouterStatus, Setup, parse_proxy};
use tauri::AppHandle;
use zapret::checks::{CheckId, CheckLevel, SystemCheck};
use zapret::service::{SERVICE2, WINWS2};

use crate::actions::{self, state};
use crate::settings::Settings;
use crate::state::{AppState, lock};

/// The DLL of the engine running now (spec 3.1) and the proxy from the settings. A zapret2 folder
/// that does not open counts as none: it must not block Flowseal's DLL.
pub fn setup(state: &AppState) -> anyhow::Result<Setup> {
    let settings = state.settings();
    let proxy = parse_proxy(settings.socks5.as_deref())?;
    let running = if zapret::procs::is_running(WINWS2) {
        Some(Engine::Zapret2)
    } else if zapret::procs::is_running("winws.exe") {
        Some(Engine::Flowseal)
    } else {
        None
    };
    let engine2 = state.engine2().ok().flatten();
    let dll = pick_dll(
        running,
        settings.zapret_dir.as_deref().map(Path::new),
        engine2.as_ref().and_then(|engine| engine.exe().parent()),
        Path::is_file,
    )?;
    Ok(Setup { dll, proxy })
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

/// A strategy of the other engine than the installed service's: the driver may change, and with
/// it the DLL the router needs (spec 3.1).
pub fn switches_engine(name: &str, service: &str) -> bool {
    zapret2::strip_prefix(name).is_some() != (service == SERVICE2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_strategy_of_the_other_engine_switches_it() {
        assert!(switches_engine("zapret2:mine", "zapret"));
        assert!(switches_engine("general (ALT11)", "zapret2"));
        assert!(!switches_engine("general (ALT11)", "zapret"));
        assert!(!switches_engine("zapret2:general", "zapret2"));
    }
}

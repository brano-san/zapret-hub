//! The main window: made when needed, destroyed by its close button, while the app stays in the
//! tray (spec 4.2).

use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";

/// Brings the window up, making it first if it is gone. A new one starts hidden: the page shows
/// it once it has drawn, so it never flashes white.
pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let shown = window
            .unminimize()
            .and_then(|()| window.show())
            .and_then(|()| window.set_focus());
        if let Err(err) = shown {
            log::warn!("cannot show the window: {err}");
        }
        return;
    }
    // Off the main thread: building a webview from a main-thread handler deadlocks on Windows,
    // and tray clicks and the second-instance callback run there.
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(err) = build(&app) {
            log::error!("cannot create the window: {err:#}");
        }
    });
}

fn build(app: &AppHandle) -> anyhow::Result<()> {
    let profile = crate::state::Paths::current()?.data().join("webview");
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title(crate::texts::APP)
        .inner_size(1100.0, 720.0)
        .min_inner_size(900.0, 560.0)
        .center()
        .visible(false)
        .data_directory(profile)
        .build()?;
    Ok(())
}

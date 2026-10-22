//! The main window: made when needed, destroyed by its close button, while the app stays in the
//! tray (spec 4.2).

use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindowBuilder};
use tauri_specta::Event as _;

use crate::dto::OpenTab;

pub const MAIN: &str = "main";

/// Brings the window up, making it first if it is gone. A new one starts hidden: the page shows
/// it once it has drawn, so it never flashes white.
pub fn show(app: &AppHandle) {
    open(app, None);
}

/// `show` on the page's tab `tab`: an open page gets `OpenTab`, a new one the URL `#<tab>`.
pub fn show_tab(app: &AppHandle, tab: &str) {
    open(app, Some(tab));
}

fn open(app: &AppHandle, tab: Option<&str>) {
    if let Some(window) = app.get_webview_window(MAIN) {
        if let Some(tab) = tab
            && let Err(err) = (OpenTab {
                tab: tab.to_owned(),
            })
            .emit(app)
        {
            log::warn!("cannot open the {tab} tab: {err}");
        }
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
    let url = tab.map_or_else(WebviewUrl::default, |tab| {
        WebviewUrl::App(format!("#{tab}").into())
    });
    std::thread::spawn(move || {
        if let Err(err) = build(&app, url) {
            log::error!("cannot create the window: {err:#}");
        }
    });
}

fn build(app: &AppHandle, url: WebviewUrl) -> anyhow::Result<()> {
    let profile = crate::state::Paths::current()?.data().join("webview");
    WebviewWindowBuilder::new(app, MAIN, url)
        .title(crate::texts::APP)
        .inner_size(1100.0, 720.0)
        .min_inner_size(900.0, 560.0)
        .center()
        .visible(false)
        .data_directory(profile)
        .build()?;
    Ok(())
}

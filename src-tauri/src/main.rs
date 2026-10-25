#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        use router::linux::caps;
        // Routing's capabilities come through the launcher (`zapret-hub-cli`, caps.rs): a hub
        // started without them goes through it once, when it holds them.
        let launched = std::env::var_os(caps::LAUNCHED_ENV).is_some();
        if let Some(err) = caps::relaunch(launched) {
            eprintln!("zapret-hub: {} did not run: {err}", caps::LAUNCHER);
        }
        if caps::present() {
            caps::keep_from_children();
        }
        if launched {
            // The launcher ran this file as `/proc/self/fd/<n>`: the name `ps` and `pgrep` show
            // is the hub's again.
            // SAFETY: a NUL-terminated name that outlives the call.
            unsafe { libc::prctl(libc::PR_SET_NAME, c"zapret-hub".as_ptr()) };
        }
        // WebKitGTK's DMA-BUF renderer kills the window on some Mesa/Nvidia Wayland setups ("Error
        // 71 dispatching to Wayland"); the path without it always paints. The file dialogs go
        // through the desktop portal (KDE, GNOME) unless the user chose otherwise.
        // SAFETY: the very first step of `main`, before any thread or GTK starts.
        unsafe {
            // What the hub starts (itself again included) goes through the launcher anew.
            std::env::remove_var(caps::LAUNCHED_ENV);
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            if std::env::var_os("GTK_USE_PORTAL").is_none() {
                std::env::set_var("GTK_USE_PORTAL", "1");
            }
        }
    }
    if let Err(err) = zapret_hub_lib::run() {
        zapret_hub_lib::report(&err);
    }
}

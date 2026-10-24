#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // WebKitGTK's DMA-BUF renderer kills the window on some Mesa/Nvidia Wayland setups ("Error
        // 71 dispatching to Wayland"); the path without it always paints. The file dialogs go
        // through the desktop portal (KDE, GNOME) unless the user chose otherwise.
        // SAFETY: the very first step of `main`, before any thread or GTK starts.
        unsafe {
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

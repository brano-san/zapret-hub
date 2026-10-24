//! Desktop notifications (spec 9.3): Windows toasts under the AppUserModelID `ZapretHub`, Linux
//! `notify-send`.

use tauri::AppHandle;

#[cfg(windows)]
const AUMID: &str = "ZapretHub";

/// An unpackaged exe has no identity for notifications: this names one. No `IconUri`: the shell
/// draws a registered icon inverted and caches it.
#[cfg(windows)]
pub fn register() {
    let key =
        windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    if let Err(err) = key.and_then(|key| key.set_string("DisplayName", crate::texts::APP)) {
        log::warn!("cannot register the notification sender: {err}");
    }
}

/// Nothing to register: the notification daemon takes the name `notify-send -a` gives.
#[cfg(not(windows))]
pub fn register() {}

#[cfg(windows)]
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(windows)]
pub fn show(app: &AppHandle, title: &str, body: &str) {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;
    log::info!("notification: {title}: {body}");
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    // WinRT wants an initialized apartment: the main thread has one.
    let dispatched = app.run_on_main_thread(move || {
        let shown = (|| -> windows::core::Result<()> {
            let document = XmlDocument::new()?;
            document.LoadXml(&HSTRING::from(xml))?;
            let toast = ToastNotification::CreateToastNotification(&document)?;
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
        })();
        if let Err(err) = shown {
            log::warn!("cannot show a notification: {err}");
        }
    });
    if let Err(err) = dispatched {
        log::warn!("cannot show a notification: {err}");
    }
}

#[cfg(not(windows))]
pub fn show(_app: &AppHandle, title: &str, body: &str) {
    log::info!("notification: {title}: {body}");
    send(title, body);
}

/// `notify-send`, not waited for; also what `lib::fatal` says with, before there is an app.
#[cfg(not(windows))]
pub fn send(title: &str, body: &str) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg(crate::texts::APP).arg(title).arg(body);
    crate::shell::launch(cmd);
}

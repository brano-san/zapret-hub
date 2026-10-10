//! Windows notifications (spec 9.3) under the AppUserModelID `ZapretHub`.

use tauri::AppHandle;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

const AUMID: &str = "ZapretHub";

/// An unpackaged exe has no identity for notifications: this names one. No `IconUri`: the shell
/// draws a registered icon inverted and caches it.
pub fn register() {
    let key =
        windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    if let Err(err) = key.and_then(|key| key.set_string("DisplayName", crate::texts::APP)) {
        log::warn!("cannot register the notification sender: {err}");
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn show(app: &AppHandle, title: &str, body: &str) {
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

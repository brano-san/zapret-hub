//! The tray icon (spec 9.3): its color says whether zapret works, its menu switches strategies.

use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};
use zapret::service::{QuickCheck, SERVICE, SvcState};

use crate::dto::NoticeLevel;
use crate::{actions, notify, texts, window};

const TRAY: &str = "main";
const SIZE: usize = 32;
const RADIUS: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Green,
    Yellow,
    Gray,
}

/// Green: the service runs and the last quick check passed. Yellow: it runs, but the check failed
/// or has not run yet. Gray: stopped or not installed.
pub fn light(service: SvcState, quick: Option<&QuickCheck>) -> Light {
    match (service, quick) {
        (SvcState::Running, Some(check)) if check.ok => Light::Green,
        (SvcState::Running, _) => Light::Yellow,
        _ => Light::Gray,
    }
}

/// A 32×32 RGBA disc with a soft edge: drawn here, so there are no image files to ship.
pub fn pixels(light: Light) -> Vec<u8> {
    let [r, g, b] = match light {
        Light::Green => [0x2E, 0xB8, 0x5C],
        Light::Yellow => [0xF2, 0xB7, 0x05],
        Light::Gray => [0x8A, 0x8F, 0x98],
    };
    let center = SIZE as f32 / 2.0;
    let mut out = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - center, y as f32 + 0.5 - center);
            let coverage = (RADIUS + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            out.extend([r, g, b, (coverage * 255.0).round() as u8]);
        }
    }
    out
}

fn icon(light: Light) -> Image<'static> {
    Image::new_owned(pixels(light), SIZE as u32, SIZE as u32)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY)
        .icon(icon(Light::Gray))
        .tooltip(texts::APP)
        .menu(&menu(app, None, &[])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, &event.id().0))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// After every `actions::changed`. Cheap: the SCM and the registry only.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    let state = actions::state(app);
    let strategy = zapret::service::strategy();
    let light = light(zapret::service::state(SERVICE), state.quick().as_ref());
    let updated = menu(app, strategy.as_deref(), &state.best(5))
        .and_then(|menu| tray.set_menu(Some(menu)))
        .and_then(|()| tray.set_icon(Some(icon(light))))
        .and_then(|()| tray.set_tooltip(Some(texts::tooltip(light, strategy.as_deref()))));
    if let Err(err) = updated {
        log::warn!("cannot update the tray: {err}");
    }
}

/// «Открыть»; «Стратегия: <имя>» → «Переключить на» and the top five; «Перезапустить zapret»;
/// «Быстрая проверка»; «Выход» (spec 9.3).
fn menu(app: &AppHandle, strategy: Option<&str>, best: &[String]) -> tauri::Result<Menu<Wry>> {
    let switch = MenuItem::with_id(app, "switch", texts::SWITCH_TO, false, None::<&str>)?;
    let empty = MenuItem::with_id(app, "empty", texts::NO_RANKING, false, None::<&str>)?;
    let line = PredefinedMenuItem::separator(app)?;
    let names = best
        .iter()
        .map(|name| MenuItem::with_id(app, format!("apply:{name}"), name, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let mut inner: Vec<&dyn IsMenuItem<Wry>> = vec![&switch, &line];
    if names.is_empty() {
        inner.push(&empty);
    }
    inner.extend(names.iter().map(|item| item as &dyn IsMenuItem<Wry>));
    let current = Submenu::with_items(app, texts::strategy(strategy), true, &inner)?;
    let open = MenuItem::with_id(app, "open", texts::OPEN, true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", texts::RESTART, true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", texts::QUICK_CHECK, true, None::<&str>)?;
    let gap = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", texts::QUIT, true, None::<&str>)?;
    let items: [&dyn IsMenuItem<Wry>; 6] = [&open, &current, &restart, &quick, &gap, &quit];
    Menu::with_items(app, &items)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => window::show(app),
        "quit" => quit(app),
        "restart" => background(app, actions::restart),
        "quick" => background(app, actions::quick_check),
        _ => {
            if let Some(name) = id.strip_prefix("apply:") {
                let name = name.to_owned();
                background(app, move |app| actions::apply(app, &name));
            }
        }
    }
}

/// Menu events come on the main thread: the work goes to another one, and its outcome to a
/// notification, as the window is usually closed.
fn background(
    app: &AppHandle,
    work: impl FnOnce(&AppHandle) -> anyhow::Result<QuickCheck> + Send + 'static,
) {
    let app = app.clone();
    std::thread::spawn(move || match work(&app) {
        Ok(check) => {
            let title = if check.ok {
                texts::QUICK_OK
            } else {
                texts::QUICK_FAILED
            };
            notify::show(&app, title, &texts::quick_result(&check));
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(&app, texts::TRAY_FAILED, &text);
            actions::notice(&app, NoticeLevel::Error, texts::TRAY_FAILED, text);
        }
    });
}

/// «Выход». A test run is canceled and puts zapret back; any other change finishes first. The wait
/// is on another thread (menu events come on the event-loop one).
fn quit(app: &AppHandle) {
    actions::state(app).cancel_tests();
    let app = app.clone();
    std::thread::spawn(move || {
        // A change or a whole test run holds the turn: once it is ours, zapret is settled and
        // `pending.json` is cleared. Forgotten, not dropped: `exit` only asks the event loop to
        // stop, and no change may start in between.
        std::mem::forget(actions::state(&app).turn());
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(ok: bool) -> QuickCheck {
        QuickCheck {
            ok,
            service: SvcState::Running,
            winws_running: true,
            checks: Vec::new(),
        }
    }

    #[test]
    fn the_light_follows_the_service_and_the_last_check() {
        assert_eq!(light(SvcState::Running, Some(&check(true))), Light::Green);
        assert_eq!(light(SvcState::Running, Some(&check(false))), Light::Yellow);
        assert_eq!(light(SvcState::Running, None), Light::Yellow);
        assert_eq!(light(SvcState::Stopped, Some(&check(true))), Light::Gray);
        assert_eq!(light(SvcState::NotInstalled, None), Light::Gray);
    }

    #[test]
    fn the_icon_is_a_disc_on_transparency() {
        let rgba = pixels(Light::Green);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| rgba[(y * 32 + x) * 4..][..4].to_vec();
        assert_eq!(at(16, 16), [0x2E, 0xB8, 0x5C, 255]);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(31, 31)[3], 0);
        assert_eq!(
            pixels(Light::Gray)[(16 * 32 + 16) * 4..][..3],
            [0x8A, 0x8F, 0x98]
        );
    }
}

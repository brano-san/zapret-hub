//! The Russian text the backend shows itself: message boxes, the tray, notifications, notices.
//! The page's text lives in `frontend/src/lib/strings.ts`.

use std::path::Path;

use probe::curl::CheckStatus;
use tester::model::{Suite, TestRun};
use zapret::service::{QuickCheck, SvcState};

pub const APP: &str = "Zapret Hub";

pub const FAILED_TO_START: &str = "Zapret Hub не запустился.";

pub const WEBVIEW2_MISSING: &str = "Для окна Zapret Hub нужен Microsoft Edge WebView2 Runtime, а на этом \
компьютере его нет.\n\nСкачайте его: https://go.microsoft.com/fwlink/p/?LinkId=2124703\n\n\
Ctrl+C копирует этот текст.";

pub const PICK_FOLDER: &str = "Папка zapret2";

pub const SETTINGS_RESET: &str = "Настройки сброшены";

pub fn settings_kept(broken: &Path) -> String {
    format!(
        "Файл настроек повреждён, он сохранён как {}.",
        broken.display()
    )
}

pub const RECOVERED: &str = "Прерванная операция отменена";

pub fn recovered(operation: &str) -> String {
    format!(
        "Прошлый запуск не довёл до конца операцию «{operation}». Служба zapret2 и запущенные вручную winws2 возвращены как были."
    )
}

pub const RECOVERY_FAILED: &str = "Состояние после прерванной операции не вернулось";

/// The way out when that state never comes back: every operation that takes a snapshot tries it
/// first and does not start without it.
pub fn recovery_failed(pending: &Path) -> String {
    format!(
        "Состояние до прерванной операции записано в {}: следующий запуск и следующая операция снова \
         попробуют его вернуть. Если удалить этот файл, операции пойдут дальше без возврата этого состояния.",
        pending.display()
    )
}

/// The operation ids `pending.json` keeps, as the user knows them.
pub fn operation(id: &str) -> &str {
    match id {
        "apply" => "смена стратегии",
        "custom" => "сохранение своей стратегии",
        "game_filter" => "смена игрового фильтра",
        "ipset" => "смена режима ipset",
        "remove_service" => "удаление службы",
        "tests" => "тесты стратегий",
        "diag" => "диагностика",
        other => other,
    }
}

pub const AUTOSTART_FAILED: &str = "Автозапуск не обновлён";
pub const HISTORY_FAILED: &str = "История тестов не прочитана";
pub const RESTORE_AFTER_TESTS: &str = "После тестов не всё вернулось на место";

pub const CLI_PIPE_BUSY: &str = "Канал CLI занят другой программой";
pub const CLI_PIPE_FAILED: &str = "Канал CLI не открыт";
pub const CLI_PIPE_STOPPED: &str = "Канал CLI остановлен";

pub const TESTS_DONE: &str = "Тесты завершены";
pub const TESTS_CANCELED: &str = "Тесты отменены";
pub const TESTS_FAILED: &str = "Тесты не выполнены";

pub fn tests_done(run: &TestRun) -> String {
    let Some(best) = run.strategies.first() else {
        return "Ни одна стратегия не проверена".to_owned();
    };
    if best.launch_error.is_some() {
        return "Ни одна стратегия не запустилась: подробности на вкладке «Тесты»".to_owned();
    }
    let score = &best.score;
    let tail = match run.suite {
        Suite::Standard => format!(
            "не прошло {} из {} проверок, задержка {}",
            score.not_ok(),
            score.total,
            score
                .median_ms
                .map_or_else(|| "—".to_owned(), |ms| format!("{ms} мс"))
        ),
        Suite::Dpi => format!(
            "похоже на блокировку {}, сбоев {}",
            score.likely_blocked, score.fail
        ),
    };
    format!("Лучшая: {} — {tail}", best.name)
}

pub const DIAG_DONE: &str = "Диагностика завершена";
pub const DIAG_CANCELED: &str = "Диагностика отменена";
pub const DIAG_FAILED: &str = "Диагностика не выполнена";
pub const RESTORE_AFTER_DIAG: &str = "После диагностики не всё вернулось на место";
pub const DIAG_QUESTION: &str = "Диагностика ждёт ответа";
pub const DIAG_CHOOSE: &str = "Диагностика: выберите адреса";
pub const DIAG_CHOOSE_TEXT: &str =
    "Снимите галочки с лишних адресов; через 2 минуты будут проверены все.";
pub const MANUAL: &str = "это делается вручную";
pub const CIRCULAR_NO_BASE: &str = "hub-circular не собрать: у активной стратегии нет основы в \
     zapret2, и hub-circular заменил бы её целиком; примените стратегию zapret2 (или перенесите эту \
     в zapret2) и повторите";

pub fn diag_question(hint: &str, path: &str) -> String {
    format!("{path}. Проверьте: {hint}. Работает?")
}

pub const QUICK_OK: &str = "Быстрая проверка пройдена";
pub const QUICK_FAILED: &str = "Быстрая проверка не прошла";

pub fn quick_result(check: &QuickCheck) -> String {
    if check.service != SvcState::Running {
        return "Служба не запущена".to_owned();
    }
    if !check.winws_running {
        return "Служба запущена, но её процесс не работает".to_owned();
    }
    check
        .checks
        .iter()
        .map(|item| match (item.status, item.time_ms) {
            (CheckStatus::Ok, Some(ms)) => format!("{} — {ms} мс", item.name),
            (CheckStatus::Ok, None) => format!("{} — OK", item.name),
            (CheckStatus::Ssl, _) => format!("{} — ошибка TLS", item.name),
            (CheckStatus::Unsup, _) => format!("{} — сброс соединения", item.name),
            (CheckStatus::Error, _) => format!("{} — нет ответа", item.name),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub const FLOWSEAL_MOVED: &str = "Flowseal заменён на zapret2";

/// What the move off Flowseal did to the services (`actions::move_step`).
pub enum Moved<'a> {
    NotTried,
    /// Flowseal's service gave way to its zapret2 copy, or did not.
    Switched(Result<(), &'a String>),
    /// The zapret2 service installed again with its strategy, to read the zapret2 folder's lists.
    Reinstalled(&'a str, Result<(), &'a String>),
}

/// The one notice of the move off Flowseal (`actions::migrate_flowseal`): `old` is the strategy of
/// Flowseal's service, `target` its zapret2 copy, `installed` whether Flowseal's service is there.
pub fn flowseal_moved(
    old: Option<&str>,
    target: &str,
    zapret2: bool,
    installed: bool,
    moved: Moved<'_>,
) -> String {
    let mut text = String::from(
        "Zapret Hub больше не работает с Flowseal: его стратегии встроены в zapret2 под тем же именем.",
    );
    if let Some(old) = old {
        text.push_str(&format!(" Стратегия «{old}» — это {target}."));
    }
    text.push(' ');
    text.push_str(&match (zapret2, installed, moved) {
        (_, _, Moved::Switched(Ok(()))) => {
            format!("Служба Flowseal удалена, вместо неё работает zapret2 со стратегией {target}.")
        }
        (_, _, Moved::Switched(Err(err))) => format!(
            "Переключить службу на {target} не удалось. Служба Flowseal восстановлена, но тесты и диагностика её останавливают: удалите её в «Системных проверках» и примените {target}.\n{err}"
        ),
        (_, installed, Moved::Reinstalled(active, Ok(()))) => {
            let mut done = format!(
                "Служба zapret2 установлена заново со стратегией {active}: списки и фейки она берёт из папки zapret2, папка Flowseal больше не нужна."
            );
            if installed {
                done.push_str(" Служба Flowseal осталась: удалите её в «Системных проверках».");
            }
            done
        }
        (_, _, Moved::Reinstalled(active, Err(err))) => format!(
            "Установить службу zapret2 заново со стратегией {active} не удалось: она ещё берёт списки и фейки из папки Flowseal. Не удаляйте эту папку, пока не примените стратегию снова.\n{err}"
        ),
        (false, true, Moved::NotTried) => format!(
            "Укажите папку zapret2 (мастер настройки), удалите службу Flowseal в «Системных проверках» и примените {target}. Свои списки перенесите из папки lists Flowseal в папку lists zapret2."
        ),
        (false, false, Moved::NotTried) => "Укажите папку zapret2 в мастере настройки. Свои списки перенесите из папки lists Flowseal в папку lists zapret2.".to_owned(),
        (true, _, Moved::NotTried) => "Списки и фейки Flowseal перенесены в папку zapret2.".to_owned(),
    });
    text
}

pub const OPEN: &str = "Открыть";
pub const SWITCH_TO: &str = "Переключить на";
pub const NO_RANKING: &str = "Нет результатов тестов";
pub const RESTART: &str = "Перезапустить zapret";
pub const QUICK_CHECK: &str = "Быстрая проверка";
pub const DIAG: &str = "Диагностика…";
pub const DIAGNOSING: &str = "идёт диагностика";
pub const QUIT: &str = "Выход";
pub const TRAY_FAILED: &str = "Действие из трея не выполнено";

pub fn strategy(name: Option<&str>) -> String {
    format!("Стратегия: {}", name.unwrap_or("не выбрана"))
}

pub const ROUTER_ON: &str = "Маршрутизация: вкл";
pub const ROUTER_OFF: &str = "Маршрутизация: выкл";
pub const ROUTER_FAILED: &str = "Маршрутизация не включилась";
pub const PROXY_DOWN: &str = "Прокси недоступен, трафик идёт напрямую";

/// Windows cuts a tray tooltip at 127 characters: short lines, the strategy's name last, so a long
/// one is what gets cut. Diagnostics stop and start zapret as they go: while they run, the tooltip
/// says so instead of the service's state.
pub fn tooltip(
    light: crate::tray::Light,
    strategy: Option<&str>,
    proxy_down: bool,
    diagnosing: bool,
) -> String {
    use crate::tray::Light;
    let status = match light {
        _ if diagnosing => DIAGNOSING,
        Light::Green => "работает",
        Light::Yellow => "работает, проверка не пройдена",
        Light::Gray => "служба не запущена",
    };
    let mut text = format!("{APP}: {status}");
    if proxy_down {
        text = format!("{text}\n{PROXY_DOWN}");
    }
    // Diagnostics run strategies of their own: the installed one's name would mislead.
    if let Some(name) = strategy.filter(|_| light != Light::Gray && !diagnosing) {
        text = format!("{text}\n{name}");
    }
    text.chars().take(127).collect()
}

#[cfg(test)]
mod tests {
    use tester::model::{Score, StrategyResult};

    use super::*;

    fn result(name: &str, launch_error: Option<&str>) -> StrategyResult {
        StrategyResult {
            name: name.into(),
            args_sha256: String::new(),
            launch_error: launch_error.map(str::to_owned),
            targets: Vec::new(),
            dpi: Vec::new(),
            score: Score {
                ok: 35,
                total: 36,
                median_ms: Some(120),
                ..Score::default()
            },
            passes: Vec::new(),
        }
    }

    fn run(strategies: Vec<StrategyResult>) -> TestRun {
        TestRun {
            id: "2026-10-10_05-03-34".into(),
            suite: Suite::Standard,
            zapret2_version: None,
            canceled: false,
            restore_error: None,
            strategies,
        }
    }

    #[test]
    fn the_tests_notification_names_the_best_strategy() {
        assert_eq!(
            tests_done(&run(vec![result("general (ALT11)", None)])),
            "Лучшая: general (ALT11) — не прошло 1 из 36 проверок, задержка 120 мс"
        );
        assert_eq!(
            tests_done(&run(vec![result("general", Some("exit 1"))])),
            "Ни одна стратегия не запустилась: подробности на вкладке «Тесты»"
        );
        assert_eq!(
            tests_done(&run(Vec::new())),
            "Ни одна стратегия не проверена"
        );
    }

    #[test]
    fn the_flowseal_notice_says_where_the_strategy_went() {
        let target = "zapret2:general (ALT11)";
        let done = flowseal_moved(
            Some("general (ALT11)"),
            target,
            true,
            true,
            Moved::Switched(Ok(())),
        );
        assert!(
            done.contains("«general (ALT11)» — это zapret2:general (ALT11)"),
            "{done}"
        );
        assert!(done.contains("вместо неё работает zapret2"), "{done}");
        let err = "winws2 --dry-run failed".to_owned();
        let failed = flowseal_moved(
            Some("general (ALT11)"),
            target,
            true,
            true,
            Moved::Switched(Err(&err)),
        );
        assert!(failed.ends_with(&err), "{failed}");
        let no_zapret2 = flowseal_moved(
            Some("general (ALT11)"),
            target,
            false,
            true,
            Moved::NotTried,
        );
        assert!(no_zapret2.contains("Системных проверках"), "{no_zapret2}");
        let quiet = flowseal_moved(None, "zapret2:general", true, false, Moved::NotTried);
        assert!(!quiet.contains("Стратегия"), "{quiet}");
        let both = flowseal_moved(
            None,
            "zapret2:general",
            true,
            true,
            Moved::Reinstalled("zapret2:mine", Ok(())),
        );
        assert!(both.contains("заново со стратегией zapret2:mine"), "{both}");
        assert!(both.contains("Служба Flowseal осталась"), "{both}");
        let kept = flowseal_moved(
            None,
            "zapret2:general",
            true,
            false,
            Moved::Reinstalled("zapret2:mine", Err(&err)),
        );
        assert!(
            kept.contains("Не удаляйте эту папку") && kept.ends_with(&err),
            "{kept}"
        );
    }

    #[test]
    fn operation_diag_named() {
        assert_eq!(operation("diag"), "диагностика");
        assert_eq!(operation("tests"), "тесты стратегий");
    }

    #[test]
    fn the_tooltip_names_a_proxy_that_is_down_and_fits_windows() {
        use crate::tray::Light;
        assert_eq!(
            tooltip(Light::Green, Some("general"), true, false),
            format!("Zapret Hub: работает\n{PROXY_DOWN}\ngeneral")
        );
        assert_eq!(
            tooltip(Light::Gray, Some("general"), false, false),
            "Zapret Hub: служба не запущена"
        );
        assert_eq!(
            tooltip(Light::Green, Some("general"), false, true),
            "Zapret Hub: идёт диагностика"
        );
        let long = "x".repeat(200);
        let cut = tooltip(Light::Yellow, Some(&long), true, false);
        assert_eq!(cut.chars().count(), 127);
        assert!(cut.contains(PROXY_DOWN));
    }
}

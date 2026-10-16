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

pub const PICK_FOLDER: &str = "Папка zapret";

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
        "Прошлый запуск не довёл до конца операцию «{operation}». Служба, ipset, игровой фильтр и запущенные вручную winws возвращены как были."
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
        other => other,
    }
}

pub const PREPARE_FAILED: &str = "Папка zapret не подготовлена";
pub const FILES_RESTORED: &str = "Файлы своих стратегий выложены заново";
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

pub const UPDATE: &str = "Вышла новая версия Flowseal";

pub fn update(version: &str) -> String {
    format!("Версия {version}. Ссылка на релиз — на вкладке «Обзор».")
}

pub const OPEN: &str = "Открыть";
pub const SWITCH_TO: &str = "Переключить на";
pub const NO_RANKING: &str = "Нет результатов тестов";
pub const RESTART: &str = "Перезапустить zapret";
pub const QUICK_CHECK: &str = "Быстрая проверка";
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
/// one is what gets cut.
pub fn tooltip(light: crate::tray::Light, strategy: Option<&str>, proxy_down: bool) -> String {
    use crate::tray::Light;
    let status = match light {
        Light::Green => "работает",
        Light::Yellow => "работает, проверка не пройдена",
        Light::Gray => "служба не запущена",
    };
    let mut text = format!("{APP}: {status}");
    if proxy_down {
        text = format!("{text}\n{PROXY_DOWN}");
    }
    if let Some(name) = strategy.filter(|_| light != Light::Gray) {
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
            flowseal_version: None,
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
    fn the_tooltip_names_a_proxy_that_is_down_and_fits_windows() {
        use crate::tray::Light;
        assert_eq!(
            tooltip(Light::Green, Some("general"), true),
            format!("Zapret Hub: работает\n{PROXY_DOWN}\ngeneral")
        );
        assert_eq!(
            tooltip(Light::Gray, Some("general"), false),
            "Zapret Hub: служба не запущена"
        );
        let long = "x".repeat(200);
        let cut = tooltip(Light::Yellow, Some(&long), true);
        assert_eq!(cut.chars().count(), 127);
        assert!(cut.contains(PROXY_DOWN));
    }
}

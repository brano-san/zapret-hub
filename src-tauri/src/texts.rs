//! The Russian text the backend shows itself: message boxes, the tray, notifications, notices.
//! The page's text lives in `frontend/src/lib/strings.ts`.

use std::path::Path;

use tester::model::{Suite, TestRun};

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
        "Прошлый запуск не довёл до конца операцию «{operation}». Служба, ipset и запущенные вручную winws возвращены как были."
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
        }
    }

    fn run(strategies: Vec<StrategyResult>) -> TestRun {
        TestRun {
            id: "2026-10-10_05-03-34".into(),
            suite: Suite::Standard,
            flowseal_version: None,
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
}

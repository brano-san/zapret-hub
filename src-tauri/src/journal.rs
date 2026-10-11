//! `log` records into `data/logs/YYYY-MM-DD.log` (spec 10): ours from info up, the rest from warn.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::state::lock;

/// Targets logged from info: the crates and the app (`zapret` also covers `zapret_hub_lib`).
/// Tauri's and the webview's chatter is not.
const OURS: [&str; 4] = ["exec", "probe", "zapret", "tester"];

pub struct Journal {
    dir: PathBuf,
    /// The day's file, opened on first use and again when the date changes.
    file: Mutex<Option<(String, File)>>,
}

impl Journal {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            file: Mutex::new(None),
        }
    }
}

impl log::Log for Journal {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() <= log::Level::Info
                && OURS.iter().any(|ours| metadata.target().starts_with(ours)))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = exec::LocalTime::now();
        let date = now.date();
        let line = format!(
            "{} {:<5} [{}] {}\n",
            now.clock(),
            record.level(),
            record.target(),
            record.args()
        );
        let mut file = lock(&self.file);
        if file.as_ref().is_none_or(|(day, _)| *day != date) {
            *file = open(&self.dir, &date).map(|opened| (date, opened));
        }
        if let Some((_, opened)) = file.as_mut() {
            let _ = opened.write_all(line.as_bytes());
        }
        if cfg!(debug_assertions) && !cfg!(test) {
            // Not `eprint!`: it panics when stderr is gone, and the panic hook logs here again.
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

fn open(dir: &Path, date: &str) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{date}.log")))
        .ok()
}

/// First thing in `run`: everything after it is journaled, panics included.
pub fn init(dir: PathBuf) {
    if log::set_logger(Box::leak(Box::new(Journal::new(dir)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
        // The message and where it came from, then wherever a panic went before (stderr).
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("{info}");
            previous(info);
        }));
    }
}

#[cfg(test)]
mod tests {
    use log::Log as _;

    use super::*;

    #[test]
    fn records_go_to_the_days_file_without_foreign_chatter() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("logs"));
        let record = |target: &str, level: log::Level, text: &str| {
            journal.log(
                &log::Record::builder()
                    .args(format_args!("{text}"))
                    .level(level)
                    .target(target)
                    .build(),
            );
        };
        record("zapret::service", log::Level::Info, "sc start zapret");
        record("wry::webview", log::Level::Info, "webview chatter");
        record("tao", log::Level::Warn, "tao trouble");
        let day = dir
            .path()
            .join("logs")
            .join(format!("{}.log", exec::LocalTime::now().date()));
        let text = std::fs::read_to_string(day).unwrap();
        assert!(
            text.contains("INFO  [zapret::service] sc start zapret"),
            "{text}"
        );
        assert!(text.contains("WARN  [tao] tao trouble"), "{text}");
        assert!(!text.contains("chatter"), "{text}");
    }
}

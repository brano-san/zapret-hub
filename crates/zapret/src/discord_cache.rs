//! `:clear_discord_cache`: close each Discord build, delete `Cache`, `Code Cache`, `GPUCache`.

use std::path::Path;
use std::time::Duration;

/// Process image, the name the report shows, the folder under `%APPDATA%`.
pub const INSTALLS: [(&str, &str, &str); 4] = [
    ("Discord.exe", "Discord", "discord"),
    ("DiscordPTB.exe", "Discord PTB", "discordptb"),
    ("DiscordCanary.exe", "Discord Canary", "discordcanary"),
    (
        "DiscordDevelopment.exe",
        "Discord Development",
        "discorddevelopment",
    ),
];

const CACHE_DIRS: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CacheOutcome {
    Deleted,
    Missing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CacheDir {
    pub path: String,
    pub outcome: CacheOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CacheReport {
    pub install: String,
    /// It was running and was closed.
    pub closed: bool,
    pub dirs: Vec<CacheDir>,
}

/// Every build installed under `%APPDATA%`; empty when none is.
pub fn clear() -> anyhow::Result<Vec<CacheReport>> {
    let appdata =
        std::env::var_os("APPDATA").ok_or_else(|| anyhow::anyhow!("APPDATA is not set"))?;
    Ok(clear_in(Path::new(&appdata), |image| {
        let closed = crate::procs::is_running(image) && crate::procs::kill_all(image).is_ok();
        if closed {
            std::thread::sleep(Duration::from_secs(1)); // let it release its files
        }
        closed
    }))
}

/// `close(image)` stops a running build and says whether it did.
pub fn clear_in(appdata: &Path, mut close: impl FnMut(&str) -> bool) -> Vec<CacheReport> {
    INSTALLS
        .iter()
        .filter(|(_, _, dir)| appdata.join(dir).is_dir())
        .map(|&(image, name, dir)| {
            let closed = close(image);
            let dirs = CACHE_DIRS
                .iter()
                .map(|sub| {
                    let path = appdata.join(dir).join(sub);
                    let (outcome, error) = if !path.is_dir() {
                        (CacheOutcome::Missing, None)
                    } else {
                        match std::fs::remove_dir_all(&path) {
                            Ok(()) => (CacheOutcome::Deleted, None),
                            Err(err) => (CacheOutcome::Failed, Some(err.to_string())),
                        }
                    };
                    CacheDir {
                        path: path.display().to_string(),
                        outcome,
                        error,
                    }
                })
                .collect();
            log::info!("discord cache: {name} cleared");
            CacheReport {
                install: name.to_owned(),
                closed,
                dirs,
            }
        })
        .collect()
}

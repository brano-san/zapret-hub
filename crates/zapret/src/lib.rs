//! The Flowseal `zapret-discord-youtube` folder as `service.bat` sees it.

pub mod batparse;
pub mod checks;
pub mod custom;
pub mod discord_cache;
pub mod fakes;
pub mod flags;
pub mod folder;
pub mod game_filter;
pub mod hosts;
pub mod ipset;
pub mod procs;
pub mod service;
pub mod snapshot;
pub mod strategies;
pub mod targets;
pub mod updates;
pub mod user_lists;

use std::path::Path;

/// A text file Flowseal ships (ASCII or UTF-8); broken bytes become U+FFFD.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

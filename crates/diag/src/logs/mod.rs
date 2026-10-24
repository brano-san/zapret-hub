//! Read-only readers of the client logs (Discord, Steam).

pub mod discord;
pub mod discord_client;
pub mod steam;
pub mod webrtc;

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Lines of the last `max_bytes` of the file for which `keep` holds; `Ok(None)` if there is no file.
pub fn tail_since(
    path: &Path,
    max_bytes: u64,
    keep: impl Fn(&str) -> bool,
) -> std::io::Result<Option<Vec<String>>> {
    Ok(tail_from(path, 0, max_bytes)?.map(|l| l.into_iter().filter(|s| keep(s)).collect()))
}

/// Size of the file now (the offset to pass to `tail_from` later); `None` if there is no file.
pub fn size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Lines written after `offset`, at most the last `max_bytes` of that range; a file shorter than
/// `offset` was rotated and is read from 0. `Ok(None)` if there is no file. The file is opened
/// with full sharing: its owner keeps writing to it.
pub fn tail_from(path: &Path, offset: u64, max_bytes: u64) -> std::io::Result<Option<Vec<String>>> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut opts, 0x7); // read | write | delete
    let mut f = match opts.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let len = f.metadata()?.len();
    let from = if len < offset { 0 } else { offset };
    let start = if len - from > max_bytes {
        len - max_bytes
    } else {
        from
    };
    // The byte before the start too: a start mid-line (a cut, or a mark taken between two writes
    // of one line) leaves the rest of that line out.
    let back = u64::from(start > 0);
    f.seek(SeekFrom::Start(start - back))?;
    let mut buf = Vec::new();
    f.take(max_bytes + back).read_to_end(&mut buf)?;
    let mid_line = back == 1 && buf.first() != Some(&b'\n');
    let text = String::from_utf8_lossy(buf.get(back as usize..).unwrap_or_default());
    let mut lines = text.lines();
    if mid_line {
        lines.next();
    }
    Ok(Some(lines.map(str::to_owned).collect()))
}

/// Existing `renderer_js.log` files of Discord, PTB and Canary.
pub fn discord_log_paths() -> Vec<PathBuf> {
    discord_log_dirs()
        .into_iter()
        .map(|dir| dir.join("renderer_js.log"))
        .filter(|p| p.is_file())
        .collect()
}

/// The `logs` folders of Discord, PTB and Canary that exist, under `%APPDATA%` (Windows) or
/// the XDG config folder (Linux).
pub fn discord_log_dirs() -> Vec<PathBuf> {
    let Ok(config) = zapret::discord_cache::config_dir() else {
        return Vec::new();
    };
    ["discord", "discordptb", "discordcanary"]
        .iter()
        .map(|d| config.join(d).join("logs"))
        .filter(|p| p.is_dir())
        .collect()
}

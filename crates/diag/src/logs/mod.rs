//! Read-only readers of the proxy and Discord logs.

pub mod discord;
pub mod neko;
pub mod neko_routes;

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Lines of the last `max_bytes` of the file for which `keep` holds; `Ok(None)` if there is no file.
/// The file is opened with full sharing: its owner keeps writing to it.
pub fn tail_since(
    path: &Path,
    max_bytes: u64,
    keep: impl Fn(&str) -> bool,
) -> std::io::Result<Option<Vec<String>>> {
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
    let cut = len > max_bytes;
    if cut {
        f.seek(SeekFrom::Start(len - max_bytes))?;
    }
    let mut buf = Vec::new();
    f.take(max_bytes).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if cut {
        lines.next(); // the first line is most likely cut in the middle
    }
    Ok(Some(lines.filter(|l| keep(l)).map(str::to_owned).collect()))
}

/// Existing `renderer_js.log` files of Discord, PTB and Canary.
pub fn discord_log_paths() -> Vec<PathBuf> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Vec::new();
    };
    ["discord", "discordptb", "discordcanary"]
        .iter()
        .map(|d| {
            Path::new(&appdata)
                .join(d)
                .join("logs")
                .join("renderer_js.log")
        })
        .filter(|p| p.is_file())
        .collect()
}

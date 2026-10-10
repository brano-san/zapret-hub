//! The `zapret-hub` block in the hosts file and when it needs refreshing (`:hosts_update`).

use std::path::PathBuf;

pub const BEGIN: &str = "# zapret-hub begin";
pub const END: &str = "# zapret-hub end";

pub fn path() -> PathBuf {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windows
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

/// Up to date when the first and the last non-empty line of Flowseal's file are in hosts.
pub fn needs_update(current: &str, flowseal: &str) -> bool {
    let lines: Vec<&str> = flowseal
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
        return false;
    };
    !current.contains(first) || !current.contains(last)
}

/// `current` with its zapret-hub block replaced by `block`, or the block appended.
pub fn merge(current: &str, block: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in current.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if !inside => kept.push(line),
            _ => {}
        }
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n\r\n");
    }
    out.push_str(BEGIN);
    out.push_str("\r\n");
    for line in block.lines() {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(END);
    out.push_str("\r\n");
    out
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HostsPlan {
    pub needs_update: bool,
    pub block: String,
    /// Lines of Flowseal's file that hosts does not have yet.
    pub added: Vec<String>,
    /// Lines of the current zapret-hub block that Flowseal no longer has.
    pub removed: Vec<String>,
}

pub fn plan(current: &str, flowseal: &str) -> HostsPlan {
    let mut inside = false;
    let mut old_block: Vec<&str> = Vec::new();
    for line in current.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => old_block.push(line),
            _ => {}
        }
    }
    let added = flowseal
        .lines()
        .filter(|line| !line.trim().is_empty() && !current.lines().any(|c| c == *line))
        .map(str::to_owned)
        .collect();
    let removed = old_block
        .into_iter()
        .filter(|line| !line.trim().is_empty() && !flowseal.lines().any(|f| f == *line))
        .map(str::to_owned)
        .collect();
    HostsPlan {
        needs_update: needs_update(current, flowseal),
        block: flowseal.to_owned(),
        added,
        removed,
    }
}

/// The hosts file; a missing one is empty. A file that is not UTF-8 is refused, not rewritten:
/// a lossy round trip would mangle its comments.
pub fn read() -> anyhow::Result<String> {
    let path = path();
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            anyhow::anyhow!(
                "{} is not UTF-8 text: edit it by hand, nothing was changed",
                path.display()
            )
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
    }
}

/// Writes `block` into hosts; the previous file is kept as `hosts.zapret-hub.bak`.
pub fn apply(block: &str) -> anyhow::Result<()> {
    let path = path();
    let current = read()?;
    if path.is_file() {
        std::fs::copy(&path, path.with_file_name("hosts.zapret-hub.bak"))?;
    }
    std::fs::write(&path, merge(&current, block))?;
    log::info!(target: "exec", "hosts: zapret-hub block written, {} lines", block.lines().count());
    let _ = exec::run(
        std::process::Command::new("ipconfig").arg("/flushdns"),
        std::time::Duration::from_secs(15),
        None,
    );
    Ok(())
}

//! `lists\ipset-all.txt` modes as `:ipset_switch_status` / `:ipset_switch` keep them, the
//! test-time "any" switch, and installing a downloaded list.

use std::path::{Path, PathBuf};

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum IpsetMode {
    Loaded,
    None,
    Any,
}

/// The address `service.bat` writes to mean "match nothing".
pub const NONE_MARKER: &str = "203.0.113.113/32";

fn list(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.txt")
}

fn backup(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.txt.backup")
}

fn test_backup(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.hub-test-backup.txt")
}

/// A missing file counts as `any`, as in `service.bat` (zero lines); any other read error is an
/// error.
pub fn try_mode(folder: &Folder) -> anyhow::Result<IpsetMode> {
    let path = list(folder);
    match crate::read_text(&path) {
        Ok(content) => Ok(mode_of(&content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(IpsetMode::Any),
        Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
    }
}

/// For display: an unreadable list reads as `any`.
pub fn mode(folder: &Folder) -> IpsetMode {
    try_mode(folder).unwrap_or(IpsetMode::Any)
}

/// Blank → any; the marker → none; anything else → loaded.
pub fn mode_of(content: &str) -> IpsetMode {
    let body = content.trim_start_matches('\u{feff}');
    if body.trim().is_empty() {
        IpsetMode::Any
    } else if body.contains(NONE_MARKER) {
        IpsetMode::None
    } else {
        IpsetMode::Loaded
    }
}

pub fn has_backup(folder: &Folder) -> bool {
    backup(folder).is_file()
}

/// Switches to `target`. Leaving `loaded` moves the list to `.backup`; going back needs it.
pub fn set_mode(folder: &Folder, target: IpsetMode) -> anyhow::Result<()> {
    let current = try_mode(folder)?;
    if current == target {
        return Ok(());
    }
    let (list, backup) = (list(folder), backup(folder));
    if target == IpsetMode::Loaded {
        anyhow::ensure!(
            backup.is_file(),
            "no ipset backup to restore: update the ipset list first"
        );
    }
    if current == IpsetMode::Loaded {
        std::fs::rename(&list, &backup)?;
    }
    match target {
        IpsetMode::None => std::fs::write(&list, format!("{NONE_MARKER}\r\n"))?,
        IpsetMode::Any => std::fs::write(&list, "")?,
        IpsetMode::Loaded => std::fs::rename(&backup, &list)?,
    }
    log::info!("ipset: {current:?} -> {target:?}");
    Ok(())
}

/// DPI tests need every address matched: the list is emptied and its content kept aside. An aside
/// copy that already exists holds the real list of an interrupted DPI run (the list on disk is the
/// emptied one), so it is kept as it is.
pub fn enter_test_any(folder: &Folder) -> anyhow::Result<()> {
    let (list, saved) = (list(folder), test_backup(folder));
    if saved.is_file() {
        log::info!(
            "ipset: kept the aside copy {} of an earlier DPI run",
            saved.display()
        );
    } else if list.is_file() {
        // Moved, not copied: a copy cut short would later be put back as the real list.
        std::fs::rename(&list, &saved)?;
    } else {
        std::fs::write(&saved, "")?;
    }
    std::fs::write(&list, "")?;
    log::info!("ipset: emptied for DPI tests, kept in {}", saved.display());
    Ok(())
}

/// Undoes `enter_test_any`; nothing to do when it was not entered.
pub fn leave_test_any(folder: &Folder) -> anyhow::Result<()> {
    let (list, saved) = (list(folder), test_backup(folder));
    if saved.is_file() {
        std::fs::rename(&saved, &list)?;
        log::info!("ipset: restored after DPI tests");
    }
    Ok(())
}

/// Swaps a downloaded list in (`downloaded` sits next to it, so the rename is atomic); the old
/// list stays as `ipset-all.txt.hub-prev`.
pub fn install(folder: &Folder, downloaded: &Path) -> anyhow::Result<()> {
    let list = list(folder);
    anyhow::ensure!(
        std::fs::metadata(downloaded)?.len() > 0,
        "the downloaded ipset list is empty"
    );
    if list.is_file() {
        std::fs::copy(&list, folder.lists().join("ipset-all.txt.hub-prev"))?;
    }
    std::fs::rename(downloaded, &list)?;
    log::info!("ipset: installed a new ipset-all.txt");
    Ok(())
}

//! `utils\check_updates.enabled`: Flowseal's own "check for updates" switch.

use crate::folder::Folder;

fn path(folder: &Folder) -> std::path::PathBuf {
    folder.utils().join("check_updates.enabled")
}

pub fn check_updates(folder: &Folder) -> bool {
    path(folder).is_file()
}

pub fn set_check_updates(folder: &Folder, on: bool) -> std::io::Result<()> {
    let path = path(folder);
    if on {
        std::fs::write(&path, "ENABLED\r\n")
    } else if path.exists() {
        std::fs::remove_file(&path)
    } else {
        Ok(())
    }
}

//! Where the Flowseal folder is, whether it is complete, which version it is.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What every operation that needs the Flowseal folder says without one.
pub const NOT_SET: &str = "the zapret folder is not set: choose it in Settings";

/// A Flowseal folder that has what `service.bat` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    /// Fails with the list of what is missing.
    pub fn open(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = root.into();
        let missing = missing_parts(&root);
        anyhow::ensure!(
            missing.is_empty(),
            "{} is not a zapret folder: missing {}",
            root.display(),
            missing.join(", ")
        );
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn lists(&self) -> PathBuf {
        self.root.join("lists")
    }

    pub fn utils(&self) -> PathBuf {
        self.root.join("utils")
    }

    pub fn winws(&self) -> PathBuf {
        self.bin().join("winws.exe")
    }

    /// `%~dp0` of `service.bat`: the root with a trailing backslash.
    pub fn root_dp0(&self) -> String {
        let mut root = self.root.to_string_lossy().into_owned();
        if !root.ends_with('\\') {
            root.push('\\');
        }
        root
    }

    /// `LOCAL_VERSION` from `service.bat`.
    pub fn version(&self) -> Option<String> {
        local_version(&crate::read_text(&self.root.join("service.bat")).ok()?)
    }
}

/// What `Folder::open` checks: `service.bat`, `bin\winws.exe`, `bin\WinDivert64.sys`, `lists\`.
pub fn missing_parts(root: &Path) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !root.join("service.bat").is_file() {
        missing.push("service.bat");
    }
    if !root.join("bin").join("winws.exe").is_file() {
        missing.push("bin\\winws.exe");
    }
    if !root.join("bin").join("WinDivert64.sys").is_file() {
        missing.push("bin\\WinDivert64.sys");
    }
    if !root.join("lists").is_dir() {
        missing.push("lists\\");
    }
    missing
}

/// `set "LOCAL_VERSION=1.10.3"` → `1.10.3`.
pub fn local_version(service_bat: &str) -> Option<String> {
    let line = service_bat
        .lines()
        .find(|line| line.contains("LOCAL_VERSION="))?;
    let value = line
        .split_once("LOCAL_VERSION=")?
        .1
        .trim()
        .trim_end_matches('"')
        .trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// `"E:\Zapret\bin\winws.exe"   --wf-tcp ...` → `E:\Zapret`, the folder two levels above winws.
pub fn root_from_image_path(image_path: &str) -> Option<PathBuf> {
    let image_path = image_path.trim();
    let exe = match image_path.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => image_path.split(' ').next()?,
    };
    Some(Path::new(exe).parent()?.parent()?.to_path_buf())
}

/// Where a Flowseal folder probably is: the `zapret` service's binary, then any running winws.
pub fn candidates() -> Vec<PathBuf> {
    let from_service = crate::service::config(crate::service::SERVICE)
        .and_then(|c| root_from_image_path(&c.image_path));
    let from_processes = crate::procs::find("winws.exe").into_iter().filter_map(|p| {
        p.exe
            .as_deref()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    });
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in from_service.into_iter().chain(from_processes) {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// The first of `dirs` that holds a usable Flowseal folder.
pub fn find_flowseal(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .find(|dir| dir.join("service.bat").is_file() && missing_parts(dir).is_empty())
        .cloned()
}

/// The cheap places a release is unpacked to, each with its direct subfolders and nothing
/// deeper: next to the app, Downloads, Desktop, Documents, Program Files, then drive roots.
/// Stops listing when `budget` is spent; the order is the order of preference.
pub fn nearby_dirs(budget: Duration) -> Vec<PathBuf> {
    let deadline = Instant::now() + budget;
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(app) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        roots.extend(app.parent().map(Path::to_path_buf));
        roots.push(app);
    }
    if let Some(home) = var("USERPROFILE") {
        roots.extend(["Downloads", "Desktop", "Documents"].map(|name| home.join(name)));
    }
    roots.extend(
        ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|name| var(name)),
    );
    // SAFETY: no arguments, no pointers: a bit mask of the drives that exist.
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    roots.extend(
        (0..26u8)
            .filter(|bit| mask >> bit & 1 == 1)
            .map(|bit| PathBuf::from(format!("{}:\\", char::from(b'A' + bit)))),
    );
    expand(&roots, deadline)
}

/// Each root, then its subfolders; a root that cannot be listed counts for itself only.
fn expand(roots: &[PathBuf], deadline: Instant) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for root in roots {
        if Instant::now() > deadline {
            break;
        }
        if !root.is_dir() || dirs.contains(root) {
            continue;
        }
        dirs.push(root.clone());
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if Instant::now() > deadline {
                break;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(entry.path());
            }
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flowseal(root: &Path) {
        for file in ["service.bat", r"bin\winws.exe", r"bin\WinDivert64.sys"] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        std::fs::create_dir_all(root.join("lists")).unwrap();
    }

    #[test]
    fn a_subfolder_of_a_root_is_a_candidate_but_not_deeper() {
        let tmp = tempfile::tempdir().unwrap();
        let near = tmp.path().join("zapret-1.9");
        let deep = tmp.path().join("a").join("b");
        flowseal(&near);
        flowseal(&deep);
        let dirs = expand(
            &[tmp.path().to_path_buf()],
            Instant::now() + Duration::from_secs(5),
        );
        assert!(dirs.contains(&near) && !dirs.contains(&deep));
        assert_eq!(find_flowseal(&dirs), Some(near));
        assert_eq!(
            find_flowseal(&[tmp.path().join("a")]),
            None,
            "an incomplete folder is skipped"
        );
    }

    #[test]
    fn a_spent_budget_lists_nothing_more() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("x")).unwrap();
        assert!(
            expand(
                &[tmp.path().to_path_buf()],
                Instant::now() - Duration::from_secs(1)
            )
            .is_empty()
        );
    }
}

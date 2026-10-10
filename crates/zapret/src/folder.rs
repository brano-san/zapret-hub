//! Where the Flowseal folder is, whether it is complete, which version it is.

use std::path::{Path, PathBuf};

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

//! Where the app keeps its files (spec 4.3).

use std::path::PathBuf;

use anyhow::Context as _;

/// Everything lives next to the exe: the app is portable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub exe_dir: PathBuf,
}

impl Paths {
    pub fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("cannot find the executable's path")?;
        let exe_dir = exe
            .parent()
            .context("the executable has no folder")?
            .to_path_buf();
        Ok(Self { exe_dir })
    }

    pub fn data(&self) -> PathBuf {
        self.exe_dir.join("data")
    }
}

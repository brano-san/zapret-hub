//! `bin\ACTIVE_DISCORD_UDP.bin` and `bin\ACTIVE_GAME_UDP.bin`: which fake each one is a copy of,
//! and swapping it (`:replace_active_fakes`).

use std::path::Path;

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FakeKind {
    Discord,
    Game,
}

impl FakeKind {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Discord => "ACTIVE_DISCORD_UDP.bin",
            Self::Game => "ACTIVE_GAME_UDP.bin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Fakes {
    /// Base names of `bin\*.bin` except `ACTIVE_*`, sorted.
    pub files: Vec<String>,
    /// The file each active fake is a copy of (same SHA-256), when one matches.
    pub discord: Option<String>,
    pub game: Option<String>,
}

pub fn state(folder: &Folder) -> anyhow::Result<Fakes> {
    let bin = folder.bin();
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&bin)? {
        let path = entry?.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("ACTIVE_"))
        {
            continue;
        }
        files.push((stem.to_owned(), hash(&path)?));
    }
    files.sort();
    let current = |kind: FakeKind| -> Option<String> {
        let active = hash(&bin.join(kind.file_name())).ok()?;
        files
            .iter()
            .find(|(_, h)| *h == active)
            .map(|(name, _)| name.clone())
    };
    Ok(Fakes {
        discord: current(FakeKind::Discord),
        game: current(FakeKind::Game),
        files: files.iter().map(|(name, _)| name.clone()).collect(),
    })
}

/// Copies `bin\<source>.bin` over the active fake; `source` must be one of `state().files`. The
/// copy is written next to it and renamed over it, so a failed copy leaves the active fake as it
/// was; written from the bytes, it takes no read-only attribute along.
pub fn replace(folder: &Folder, kind: FakeKind, source: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        state(folder)?.files.iter().any(|f| f == source),
        "no fake named {source} in bin"
    );
    let bin = folder.bin();
    let active = bin.join(kind.file_name());
    let tmp = active.with_extension("bin.hub-tmp");
    let source_path = bin.join(format!("{source}.bin"));
    let bytes = std::fs::read(&source_path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", source_path.display()))?;
    std::fs::write(&tmp, bytes).map_err(|e| anyhow::anyhow!("{}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &active) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("{}: {e}", active.display());
    }
    log::info!("{} := {source}.bin", kind.file_name());
    Ok(())
}

fn hash(path: &Path) -> std::io::Result<String> {
    Ok(crate::sha256_hex(&std::fs::read(path)?))
}

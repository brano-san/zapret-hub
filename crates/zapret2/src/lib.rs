//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// Every zapret2 strategy's name starts with it: `zapret2:general` (zapret2 spec 3.1). A `.bat`
/// file name has no colon, so the names never meet Flowseal's.
pub const PREFIX: &str = "zapret2:";

/// Where `winws2.exe` may be under the root: zapret-win-bundle, a `bin\`, the release zip.
const EXE_DIRS: [&str; 3] = ["", "bin", r"binaries\windows-x86_64"];
/// What `winws2.exe` needs next to it.
const BESIDE_EXE: [&str; 3] = ["cygwin1.dll", "WinDivert.dll", "WinDivert64.sys"];
/// What every strategy loads from `lua\`.
const LUA_FILES: [&str; 2] = ["zapret-lib.lua", "zapret-antidpi.lua"];

/// A zapret2 folder with what `winws2.exe` needs (zapret2 spec 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    root: PathBuf,
    exe: PathBuf,
}

/// What `winws2.exe --version` says, checked against the Lua files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// `v1.0.5.2 (6b6c63e…)`, or `self-built version <date> <time>`.
    pub text: String,
    /// `lua_compat_ver`.
    pub compat: u32,
}

impl Engine {
    /// The root is `dir` or its `zapret-winws` (zapret-win-bundle), whichever holds
    /// `lua\zapret-lib.lua`. Fails naming every missing file, and on a path with Cyrillic letters:
    /// Cygwin does not carry them.
    pub fn open(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let root: PathBuf = [dir.to_path_buf(), dir.join("zapret-winws")]
            .into_iter()
            .find(|root| root.join("lua").join("zapret-lib.lua").is_file())
            .unwrap_or_else(|| dir.to_path_buf())
            .components()
            .collect();
        anyhow::ensure!(
            !zapret::checks::has_cyrillic(&root.to_string_lossy()),
            "{} has Cyrillic letters, which winws2 (a Cygwin program) cannot use: move zapret2 to a folder with a Latin path",
            root.display()
        );
        let exe_dir = EXE_DIRS
            .iter()
            .map(|sub| root.join(sub))
            .find(|dir| dir.join("winws2.exe").is_file());
        let mut missing = Vec::new();
        match &exe_dir {
            Some(exe_dir) => {
                let sub = exe_dir.strip_prefix(&root).unwrap_or(exe_dir);
                for file in BESIDE_EXE {
                    if !exe_dir.join(file).is_file() {
                        missing.push(sub.join(file).display().to_string());
                    }
                }
            }
            None => missing.push("winws2.exe".to_owned()),
        }
        for file in LUA_FILES {
            let path = Path::new("lua").join(file);
            if !root.join(&path).is_file() {
                missing.push(path.display().to_string());
            }
        }
        let exe = match exe_dir {
            Some(exe_dir) if missing.is_empty() => exe_dir.join("winws2.exe"),
            _ => anyhow::bail!(
                "{} is not a zapret2 folder: missing {}",
                root.display(),
                missing.join(", ")
            ),
        };
        Ok(Self { root, exe })
    }

    /// Where `lua\` is; `--chdir` makes it the working directory of every run.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    pub fn lua(&self) -> PathBuf {
        self.root.join("lua")
    }

    /// `winws2.exe --version`, run again only when `winws2.exe` or `lua\zapret-lib.lua` changes:
    /// the overview asks on every change. Needs admin rights (winws2's manifest asks for them). A
    /// `lua_compat_ver` other than the one `lua\zapret-lib.lua` requires is an error: the engine
    /// and the Lua files come from different releases.
    // ponytail: an error stays cached until a file changes; a restart reads it again.
    pub fn version(&self) -> anyhow::Result<Version> {
        type Key = (PathBuf, Option<SystemTime>, Option<SystemTime>);
        static CACHE: Mutex<Option<(Key, Result<Version, String>)>> = Mutex::new(None);
        let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let lib = self.lua().join("zapret-lib.lua");
        let key = (self.exe.clone(), modified(&self.exe), modified(&lib));
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, result)) = &*cache
            && *cached == key
        {
            return result.clone().map_err(anyhow::Error::msg);
        }
        let result = self.read_version(&lib).map_err(|err| format!("{err:#}"));
        *cache = Some((key, result.clone()));
        result.map_err(anyhow::Error::msg)
    }

    fn read_version(&self, lib: &Path) -> anyhow::Result<Version> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--version").current_dir(&self.root);
        let output = exec::run(&mut cmd, Duration::from_secs(15), None)?.check()?;
        let (text, compat) = parse_banner(&output.stdout).ok_or_else(|| {
            anyhow::anyhow!("winws2.exe printed no version:\n{}", output.describe())
        })?;
        let required = required_compat(&zapret::read_text(lib)?).ok_or_else(|| {
            anyhow::anyhow!("{} has no NFQWS2_COMPAT_VER_REQUIRED line", lib.display())
        })?;
        anyhow::ensure!(
            compat == required,
            "winws2.exe {text} has lua_compat_ver {compat}, but {} requires {required}: the engine and the Lua files come from different zapret2 releases",
            lib.display()
        );
        Ok(Version { text, compat })
    }
}

/// The banner every winws2 run prints first: `github version v1.0.5.2 (6b6c63e) lua_compat_ver 6`
/// gives `v1.0.5.2 (6b6c63e)` and 6; a self-built one keeps its whole text.
pub fn parse_banner(output: &str) -> Option<(String, u32)> {
    output.lines().find_map(|line| {
        let (version, compat) = line.trim().rsplit_once(" lua_compat_ver ")?;
        let compat = compat.trim().parse().ok()?;
        let version = version.strip_prefix("github version ").unwrap_or(version);
        Some((version.to_owned(), compat))
    })
}

/// `NFQWS2_COMPAT_VER_REQUIRED=6`, the first line of `lua\zapret-lib.lua`.
pub fn required_compat(lib_lua: &str) -> Option<u32> {
    lib_lua.lines().find_map(|line| {
        line.trim()
            .strip_prefix("NFQWS2_COMPAT_VER_REQUIRED")?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .parse()
            .ok()
    })
}

/// The zapret2 folder as «Настройки», «Обзор» and `zapret2_info` show it.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct Info {
    /// The `zapret2_dir` setting; `None` while zapret2 is off.
    pub dir: Option<String>,
    /// `v1.0.5.2 (6b6c63e…)`.
    pub version: Option<String>,
    /// `lua_compat_ver`, the same in the engine and its Lua files.
    pub compat: Option<u32>,
    /// Why the folder cannot be used.
    pub error: Option<String>,
}

pub fn info(dir: Option<&str>) -> Info {
    let Some(dir) = dir else {
        return Info::default();
    };
    let version = Engine::open(dir).and_then(|engine| engine.version());
    Info {
        dir: Some(dir.to_owned()),
        version: version.as_ref().ok().map(|v| v.text.clone()),
        compat: version.as_ref().ok().map(|v| v.compat),
        error: version.err().map(|err| format!("{err:#}")),
    }
}

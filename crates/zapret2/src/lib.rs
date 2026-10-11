//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version here, its strategies in `strategies`, the check that runs winws2
//! and the `zapret2` service in `service`.

pub mod circular;
pub mod service;
pub mod strategies;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// Every zapret2 strategy's name starts with it: `zapret2:general` (zapret2 spec 3.1). A `.bat`
/// file name has no colon, so the names never meet Flowseal's.
pub const PREFIX: &str = "zapret2:";

/// `name` without the `zapret2:` prefix, whatever its case (`Zapret2:General` from a CLI call
/// reaches the same strategy as `zapret2:General`); `None` for a Flowseal name.
pub fn strip_prefix(name: &str) -> Option<&str> {
    let head = name.get(..PREFIX.len())?;
    head.eq_ignore_ascii_case(PREFIX)
        .then(|| &name[PREFIX.len()..])
}

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

/// What `version` is cached for: the exe, and when it and `lua\zapret-lib.lua` last changed.
type Key = (PathBuf, Option<SystemTime>, Option<SystemTime>);
type Cache = Mutex<Option<(Key, Result<Version, String>)>>;

impl Engine {
    /// The root is `dir` or its `zapret-winws` (zapret-win-bundle), whichever holds
    /// `lua\zapret-lib.lua`. Fails naming every missing file, on a path with Cyrillic letters
    /// (Cygwin does not carry them) and on a drive root (`--chdir="E:\"` ends in `\"`, which
    /// winws2 reads as an escaped quote).
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
        anyhow::ensure!(
            !(root.has_root() && root.parent().is_none()),
            "{} is the root of a drive, which winws2 cannot use (its path would end in \\ right before a quote): put zapret2 in a folder",
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

    /// `winws2.exe --version`, run again only when `winws2.exe` or `lua\zapret-lib.lua` changes
    /// (or when it could not be started): the overview asks on every change. Needs admin rights
    /// (winws2's manifest asks for them). A `lua_compat_ver` other than the one
    /// `lua\zapret-lib.lua` requires is an error: the engine and the Lua files come from
    /// different releases.
    // ponytail: a bad answer stays cached until a file changes (a restart reads it again); a start
    // that failed is retried on every call, with no back-off.
    pub fn version(&self) -> anyhow::Result<Version> {
        static CACHE: Cache = Mutex::new(None);
        let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let lib = self.lua().join("zapret-lib.lua");
        let key = (self.exe.clone(), modified(&self.exe), modified(&lib));
        cached(&CACHE, key, || self.read_version(&lib))
    }

    fn read_version(&self, lib: &Path) -> anyhow::Result<Version> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--version").current_dir(&self.root);
        let output = exec::run(&mut cmd, Duration::from_secs(15), None)
            .map_err(|err| anyhow::Error::new(err).context(self.exe.display().to_string()))?
            .check()?;
        verdict(&output, lib, &zapret::read_text(lib)?)
    }
}

/// `read`'s answer for `key`, kept while `key` stays the same: a broken exe is not started again
/// until a file changes. Except an `io::Error` as the root cause: a start the system refused (an
/// antivirus, os error 225; a lock, 32; no access, 5) or a Lua file that cannot be read. The user
/// can fix that without touching a file, so it is read again. The error is one text either way,
/// so `{err}` shows the file and the cause as `{err:#}` does.
fn cached(
    cache: &Cache,
    key: Key,
    read: impl FnOnce() -> anyhow::Result<Version>,
) -> anyhow::Result<Version> {
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((cached, result)) = &*cache
        && *cached == key
    {
        return result.clone().map_err(anyhow::Error::msg);
    }
    let result = read();
    let retry = matches!(&result, Err(err) if err.root_cause().is::<std::io::Error>());
    let result = result.map_err(|err| format!("{err:#}"));
    if !retry {
        *cache = Some((key, result.clone()));
    }
    result.map_err(anyhow::Error::msg)
}

/// `output` of `winws2.exe --version` against `lib_text`, the text of `lib` (`lua\zapret-lib.lua`):
/// no banner, no required level and two different levels are errors.
fn verdict(output: &exec::Output, lib: &Path, lib_text: &str) -> anyhow::Result<Version> {
    let (text, compat) = parse_banner(&output.stdout)
        .ok_or_else(|| anyhow::anyhow!("winws2.exe printed no version:\n{}", output.describe()))?;
    let required = required_compat(lib_text).ok_or_else(|| {
        anyhow::anyhow!("{} has no NFQWS2_COMPAT_VER_REQUIRED line", lib.display())
    })?;
    anyhow::ensure!(
        compat == required,
        "winws2.exe {text} has lua_compat_ver {compat}, but {} requires {required}: the engine and the Lua files come from different zapret2 releases",
        lib.display()
    );
    Ok(Version { text, compat })
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

/// Whether `dir` (or its `zapret-winws`) has `winws2.exe` and `lua\zapret-lib.lua`: the cheap
/// half of `Engine::open`, for searching.
pub fn looks_like_zapret2(dir: &Path) -> bool {
    [dir.to_path_buf(), dir.join("zapret-winws")]
        .iter()
        .any(|root| {
            root.join("lua").join("zapret-lib.lua").is_file()
                && EXE_DIRS
                    .iter()
                    .any(|sub| root.join(sub).join("winws2.exe").is_file())
        })
}

/// The first of `dirs` that looks like an unpacked zapret2 release.
pub fn find_zapret2(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find(|dir| looks_like_zapret2(dir)).cloned()
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_release_is_found_by_its_exe_and_lua() {
        let tmp = tempfile::tempdir().unwrap();
        let touch = |path: &Path| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        };
        let release = tmp.path().join("zapret2");
        let bundle = tmp.path().join("bundle");
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        touch(&release.join(r"binaries\windows-x86_64\winws2.exe"));
        touch(&release.join(r"lua\zapret-lib.lua"));
        touch(&bundle.join(r"zapret-winws\winws2.exe"));
        touch(&bundle.join(r"zapret-winws\lua\zapret-lib.lua"));
        assert!(looks_like_zapret2(&release) && looks_like_zapret2(&bundle));
        assert!(!looks_like_zapret2(&empty));
        assert_eq!(
            find_zapret2(&[empty, bundle.clone(), release]),
            Some(bundle)
        );
    }

    const LIB: &str = r"D:\zapret2\lua\zapret-lib.lua";
    const REQUIRES_6: &str = "NFQWS2_COMPAT_VER_REQUIRED=6\n";

    /// What `winws2.exe --version` left behind: it printed `stdout` and exited with 0.
    fn printed(stdout: &str) -> exec::Output {
        exec::Output {
            command: "winws2.exe --version".into(),
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
            timed_out: false,
            canceled: false,
            elapsed: Duration::ZERO,
        }
    }

    fn verdict_on(stdout: &str, lib_text: &str) -> anyhow::Result<Version> {
        verdict(&printed(stdout), Path::new(LIB), lib_text)
    }

    #[test]
    fn a_banner_and_lua_files_of_one_release_give_the_version() {
        let banner = "github version v1.0.5.2 (6b6c63e) lua_compat_ver 6\n";
        let version = verdict_on(banner, REQUIRES_6).unwrap();
        assert_eq!(version.text, "v1.0.5.2 (6b6c63e)");
        assert_eq!(version.compat, 6);
    }

    #[test]
    fn no_banner_is_an_error_with_everything_winws2_printed() {
        let output = printed("winws2: unknown option\n");
        let err = verdict(&output, Path::new(LIB), REQUIRES_6).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("winws2.exe printed no version:\n{}", output.describe())
        );
    }

    #[test]
    fn lua_files_without_a_required_level_are_an_error() {
        let banner = "github version v1.0.5.2 (6b6c63e) lua_compat_ver 6\n";
        let err = verdict_on(banner, "-- no level\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("{LIB} has no NFQWS2_COMPAT_VER_REQUIRED line")
        );
    }

    #[test]
    fn an_engine_and_lua_files_of_two_releases_are_an_error() {
        let banner = "github version v1.0.5.2 (6b6c63e) lua_compat_ver 5\n";
        let err = verdict_on(banner, REQUIRES_6).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "winws2.exe v1.0.5.2 (6b6c63e) has lua_compat_ver 5, but {LIB} requires 6: the engine and the Lua files come from different zapret2 releases"
            )
        );
    }

    #[test]
    fn a_start_that_failed_is_asked_again_but_a_bad_answer_is_not() {
        let cache = Cache::new(None);
        let runs = Cell::new(0);
        let ask = |answer: anyhow::Result<Version>| {
            let key = (PathBuf::from("winws2.exe"), None, None);
            cached(&cache, key, || {
                runs.set(runs.get() + 1);
                answer
            })
        };
        // The system refuses to start winws2.exe (os error 225: an antivirus). The user can fix
        // that without touching a file, so every call tries again.
        let refused = || -> anyhow::Result<Version> {
            let io = std::io::Error::from_raw_os_error(225);
            Err(anyhow::Error::new(io).context(r"D:\zapret2\winws2.exe"))
        };
        let err = ask(refused()).unwrap_err();
        assert!(ask(refused()).is_err());
        assert_eq!(runs.get(), 2);
        // The file and the cause show in `{err}` as well as in `{err:#}`.
        assert_eq!(format!("{err}"), format!("{err:#}"));
        assert!(
            err.to_string().starts_with(r"D:\zapret2\winws2.exe: "),
            "{err}"
        );
        assert!(err.to_string().ends_with("(os error 225)"), "{err}");
        // winws2 ran and its answer was bad: kept until a file changes.
        let bad = || -> anyhow::Result<Version> { Err(anyhow::anyhow!("exit code 1")) };
        assert_eq!(ask(bad()).unwrap_err().to_string(), "exit code 1");
        assert_eq!(ask(refused()).unwrap_err().to_string(), "exit code 1");
        assert_eq!(runs.get(), 3);
    }
}

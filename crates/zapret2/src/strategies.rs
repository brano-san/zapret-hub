//! zapret2 strategies (zapret2 spec 5): the built-in texts and the user's `<zapret2 folder>\strategies\<name>.txt`,
//! the command line a text becomes, and the static part of the check.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use zapret::batparse::replace_ci;
use zapret::custom::quoted_tokens;
use zapret::folder::Folder;

use crate::{Engine, PREFIX};

/// Read-only. Not checked on live traffic: the tests rank them (spec 5.2).
const BUILTIN: [(&str, &str); 2] = [
    ("general", include_str!("builtin/general.txt")),
    ("general-sni", include_str!("builtin/general-sni.txt")),
];

/// Flowseal's strategies ported to zapret2 (names without the prefix): read-only like `BUILTIN`, but
/// left out of the tests that run without names.
const FLOWSEAL: [(&str, &str); 22] = [
    (
        "general (ALT)",
        include_str!("builtin/flowseal/general (ALT).txt"),
    ),
    (
        "general (ALT2)",
        include_str!("builtin/flowseal/general (ALT2).txt"),
    ),
    (
        "general (ALT3)",
        include_str!("builtin/flowseal/general (ALT3).txt"),
    ),
    (
        "general (ALT4)",
        include_str!("builtin/flowseal/general (ALT4).txt"),
    ),
    (
        "general (ALT5)",
        include_str!("builtin/flowseal/general (ALT5).txt"),
    ),
    (
        "general (ALT6)",
        include_str!("builtin/flowseal/general (ALT6).txt"),
    ),
    (
        "general (ALT7)",
        include_str!("builtin/flowseal/general (ALT7).txt"),
    ),
    (
        "general (ALT8)",
        include_str!("builtin/flowseal/general (ALT8).txt"),
    ),
    (
        "general (ALT9)",
        include_str!("builtin/flowseal/general (ALT9).txt"),
    ),
    (
        "general (ALT10)",
        include_str!("builtin/flowseal/general (ALT10).txt"),
    ),
    (
        "general (ALT11)",
        include_str!("builtin/flowseal/general (ALT11).txt"),
    ),
    (
        "general (ALT12)",
        include_str!("builtin/flowseal/general (ALT12).txt"),
    ),
    (
        "general (ALT13)",
        include_str!("builtin/flowseal/general (ALT13).txt"),
    ),
    (
        "general (EXP)",
        include_str!("builtin/flowseal/general (EXP).txt"),
    ),
    (
        "general (FAKE TLS AUTO ALT)",
        include_str!("builtin/flowseal/general (FAKE TLS AUTO ALT).txt"),
    ),
    (
        "general (FAKE TLS AUTO ALT2)",
        include_str!("builtin/flowseal/general (FAKE TLS AUTO ALT2).txt"),
    ),
    (
        "general (FAKE TLS AUTO ALT3)",
        include_str!("builtin/flowseal/general (FAKE TLS AUTO ALT3).txt"),
    ),
    (
        "general (FAKE TLS AUTO)",
        include_str!("builtin/flowseal/general (FAKE TLS AUTO).txt"),
    ),
    (
        "general (Flowseal)",
        include_str!("builtin/flowseal/general (Flowseal).txt"),
    ),
    (
        "general (SIMPLE FAKE ALT)",
        include_str!("builtin/flowseal/general (SIMPLE FAKE ALT).txt"),
    ),
    (
        "general (SIMPLE FAKE ALT2)",
        include_str!("builtin/flowseal/general (SIMPLE FAKE ALT2).txt"),
    ),
    (
        "general (SIMPLE FAKE)",
        include_str!("builtin/flowseal/general (SIMPLE FAKE).txt"),
    ),
];

/// The placeholders that come from the Flowseal folder.
const FLOWSEAL_VARS: [&str; 5] = [
    "%LISTS%",
    "%BIN%",
    "%GameFilter%",
    "%GameFilterTCP%",
    "%GameFilterUDP%",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    /// Without the prefix: `general`.
    pub name: String,
    /// As written: comment lines and placeholders kept.
    pub text: String,
    pub builtin: bool,
    /// A built-in copy of a Flowseal strategy (`FLOWSEAL`): not in the default test run.
    pub flowseal_copy: bool,
}

impl Strategy {
    /// `zapret2:<name>`: what lists, tests, the registry and the CLI call it.
    pub fn full_name(&self) -> String {
        format!("{PREFIX}{}", self.name)
    }
}

/// `<zapret2 folder>\strategies`: with the engine, so that a new app version keeps them.
pub fn dir(home: &Path) -> PathBuf {
    home.join("strategies")
}

/// Moves `*.txt` of an older `data\zapret2` into the strategies folder; a name already there
/// stays as it is and its old file is left where it was. Returns the names moved.
pub fn migrate(old: &Path, home: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(old) else {
        return Vec::new();
    };
    let dir = dir(home);
    let mut moved = Vec::new();
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(file) = path.file_name().map(|file| file.to_owned()) else {
            continue;
        };
        let txt = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        let target = dir.join(&file);
        if !txt || !path.is_file() || target.exists() {
            continue;
        }
        let done = std::fs::create_dir_all(&dir).and_then(|()| {
            std::fs::rename(&path, &target).or_else(|_| {
                // Another drive: copy, then remove.
                std::fs::copy(&path, &target).and_then(|_| std::fs::remove_file(&path))
            })
        });
        match done {
            Ok(()) => {
                log::info!("{} moved to {}", path.display(), target.display());
                moved.push(file.to_string_lossy().into_owned());
            }
            Err(err) => log::warn!(
                "{} not moved to {}: {err}",
                path.display(),
                target.display()
            ),
        }
    }
    moved
}

/// The files of the folder that are not strategies, with the reason: not `.txt`, named like a
/// built-in one, a name `save` would refuse, or unreadable.
pub fn skipped(home: &Path) -> Vec<(String, String)> {
    scan(home).map_or_else(|_| Vec::new(), |(_, skipped)| skipped)
}

/// The built-in ones, then the user's in natural order. A file that is not a strategy (see
/// `skipped`) is left out: logged, so that it does not stop the others from being listed, applied
/// or deleted.
pub fn list(home: &Path) -> anyhow::Result<Vec<Strategy>> {
    Ok(builtin().chain(scan(home)?.0).collect())
}

type Scan = (Vec<Strategy>, Vec<(String, String)>);

fn scan(home: &Path) -> anyhow::Result<Scan> {
    let dir = dir(home);
    let mut own = Vec::new();
    let mut skipped = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((own, skipped)),
        Err(err) => return Err(anyhow::Error::new(err).context(dir.display().to_string())),
    };
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let txt = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        let problem = if !txt {
            Some("only .txt files are strategies".to_owned())
        } else if is_builtin(stem) {
            Some("named like a built-in strategy".to_owned())
        } else if let Err(err) = zapret::custom::validate_name(stem) {
            Some(format!("{err:#}"))
        } else {
            None
        };
        if let Some(problem) = problem {
            log::warn!(
                "{} is not a strategy and is skipped: {problem}",
                path.display()
            );
            skipped.push((file, problem));
            continue;
        }
        match zapret::read_text(&path) {
            Ok(text) => own.push(Strategy {
                name: stem.to_owned(),
                text: text.trim_start_matches('\u{feff}').to_owned(),
                builtin: false,
                flowseal_copy: false,
            }),
            Err(err) => {
                log::warn!("{} cannot be read and is skipped: {err}", path.display());
                skipped.push((file, format!("cannot be read: {err}")));
            }
        }
    }
    own.sort_by_cached_key(|s| zapret::strategies::natural_key(&s.name));
    skipped.sort();
    Ok((own, skipped))
}

fn builtin() -> impl Iterator<Item = Strategy> {
    let table = |table: &'static [(&str, &str)], flowseal_copy| {
        table.iter().map(move |(name, text)| Strategy {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
            builtin: true,
            flowseal_copy,
        })
    };
    // Natural order: the copies are added in it by hand.
    table(&BUILTIN, false).chain(table(&FLOWSEAL, true))
}

fn is_builtin(name: &str) -> bool {
    BUILTIN
        .iter()
        .chain(&FLOWSEAL)
        .any(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
}

/// `name` without the prefix.
pub fn find(home: &Path, name: &str) -> anyhow::Result<Strategy> {
    list(home)?
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("no zapret2 strategy named {name}"))
}

/// The name a save writes under (spec 5.2): a valid one, not a built-in one; new for `create`,
/// one of the user's otherwise.
pub fn claim(home: &Path, name: &str, create: bool) -> anyhow::Result<String> {
    let name = zapret::custom::validate_name(name)?;
    anyhow::ensure!(
        !is_builtin(&name),
        "{PREFIX}{name} is built in: save a copy under another name"
    );
    let exists = list(home)?
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name));
    if create {
        anyhow::ensure!(!exists, "the name {name} is taken");
    } else {
        anyhow::ensure!(exists, "no zapret2 strategy of yours named {name}");
    }
    Ok(name)
}

/// `<zapret2 folder>\strategies\<name>.txt`, the text as it came.
pub fn write(home: &Path, name: &str, text: &str) -> anyhow::Result<()> {
    let dir = dir(home);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let path = dir.join(format!("{name}.txt"));
    std::fs::write(&path, text).with_context(|| path.display().to_string())
}

/// The user's own, and not `active` (the registry's strategy name).
pub fn delete(home: &Path, name: &str, active: Option<&str>) -> anyhow::Result<()> {
    let strategy = find(home, name)?;
    let full = strategy.full_name();
    anyhow::ensure!(!strategy.builtin, "{full} is built in: it stays");
    anyhow::ensure!(
        !active.is_some_and(|active| active.eq_ignore_ascii_case(&full)),
        "{full} is the active strategy: apply another one first"
    );
    let path = dir(home).join(format!("{}.txt", strategy.name));
    std::fs::remove_file(&path).with_context(|| path.display().to_string())
}

/// The text as `winws2` arguments: lines that start with `#` dropped, the rest joined by spaces. A
/// `#` inside a line is zapret2's own (`#var`) and stays (spec 5.1). A BOM before the first line
/// goes too: `trim` keeps it, and it would hide that line's `#`.
pub fn args(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The one command line of a text, for the test, the check and the service alike (spec 5.1):
/// `--chdir="<root>"`, then the arguments with the placeholders filled in. Every path placeholder
/// ends in `\`. `%LISTS%`, `%BIN%` and the game filter come from the Flowseal folder: without one
/// they are an error.
pub fn tail(engine: &Engine, folder: Option<&Folder>, text: &str) -> anyhow::Result<String> {
    let dir = |path: PathBuf| format!("{}\\", path.display());
    let mut args = args(text);
    for (var, value) in [
        ("%LUA%", dir(engine.lua())),
        ("%FILES%", dir(engine.root().join("files"))),
    ] {
        args = replace_ci(&args, var, &value);
    }
    match folder {
        Some(folder) => {
            let filter = zapret::game_filter::try_read(folder)?.vars();
            for (var, value) in [
                ("%LISTS%", dir(folder.lists())),
                ("%BIN%", dir(folder.bin())),
                ("%GameFilterTCP%", filter.tcp),
                ("%GameFilterUDP%", filter.udp),
                ("%GameFilter%", filter.all),
            ] {
                args = replace_ci(&args, var, &value);
            }
        }
        None => {
            let lower = args.to_ascii_lowercase();
            if let Some(var) = FLOWSEAL_VARS
                .iter()
                .find(|var| lower.contains(&var.to_ascii_lowercase()))
            {
                anyhow::bail!("the strategy uses {var}: choose the zapret folder in Settings");
            }
        }
    }
    Ok(format!("--chdir=\"{}\" {args}", engine.root().display()))
}

/// A lenient first pass over what `--dry-run` cannot see (spec 5.3), not winws2's own option
/// parsing: every `--lua-desync=<function>` must be defined as `function <name>(` in some
/// `lua\*.lua` (one this strategy loads or not), in `--lua-init=<code>` or in a
/// `--lua-init=@<file>`; and the line has no `\"`, which a placeholder right before a quote makes.
/// Only the `--option=value` spelling is read, and quotes just group (`"--lua-desync=x"` and
/// `--lua-init="@C:\My Lua\x.lua"` read as without them): single-dash, abbreviated and
/// space-separated options are not seen. A circular profile must number its variants 1, 2, ...
/// (`circular::analyze`'s errors). `--intercept=0` (step 3) is the real check.
pub fn check(engine: &Engine, tail: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !tail.contains("\\\""),
        "the command line has \\\" (a path ending in \\ right before a quote), which winws2 reads as a quote inside the value: {tail}"
    );
    let lua = engine.lua();
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&lua).with_context(|| lua.display().to_string())? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "lua") {
            sources.push(zapret::read_text(&path).with_context(|| path.display().to_string())?);
        }
    }
    let tokens: Vec<String> = quoted_tokens(tail)
        .into_iter()
        .map(|token| token.replace('"', ""))
        .collect();
    for init in tokens.iter().filter_map(|t| t.strip_prefix("--lua-init=")) {
        match init.strip_prefix('@') {
            Some(file) => {
                let path = engine.root().join(file);
                sources.push(
                    zapret::read_text(&path)
                        .with_context(|| format!("--lua-init file {}", path.display()))?,
                );
            }
            None => sources.push(init.to_owned()),
        }
    }
    let mut missing: Vec<&str> = tokens
        .iter()
        .filter_map(|t| t.strip_prefix("--lua-desync="))
        .map(|call| call.split(':').next().unwrap_or_default())
        .filter(|function| {
            let definition = format!("function {function}(");
            !sources.iter().any(|source| source.contains(&definition))
        })
        .collect();
    missing.sort_unstable();
    missing.dedup();
    anyhow::ensure!(
        missing.is_empty(),
        "--lua-desync calls {}, which no lua\\*.lua file and no --lua-init defines",
        missing.join(", ")
    );
    let circular = crate::circular::analyze(tail);
    anyhow::ensure!(circular.errors.is_empty(), "{}", circular.errors.join("\n"));
    Ok(())
}

/// The global part (`--wf-*`, `--lua-init`, `--blob`) and the profiles between `--new`s, for the
/// card (spec 8).
pub fn split(args: &str) -> (String, Vec<String>) {
    let tokens = quoted_tokens(args);
    let global = |token: &String| {
        ["--wf-", "--lua-init", "--blob"]
            .iter()
            .any(|prefix| token.starts_with(prefix))
    };
    let profiles = tokens
        .split(|token| token == "--new" || token.starts_with("--new="))
        .map(|profile| {
            profile
                .iter()
                .filter(|token| !global(token))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|profile| !profile.is_empty())
        .collect();
    let global = tokens
        .iter()
        .filter(|token| global(token))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (global, profiles)
}

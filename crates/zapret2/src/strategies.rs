//! zapret2 strategies (zapret2 spec 5): the built-in texts and the user's `data/zapret2/<name>.txt`,
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
}

impl Strategy {
    /// `zapret2:<name>`: what lists, tests, the registry and the CLI call it.
    pub fn full_name(&self) -> String {
        format!("{PREFIX}{}", self.name)
    }
}

/// `data/zapret2`
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("zapret2")
}

/// The built-in ones, then the user's in natural order. A file named like a built-in one is left
/// out: the built-in one would hide it anyway. So is a file that cannot be read (a lock, no
/// access): logged, so that it does not stop the others from being listed, applied or deleted.
pub fn list(data_dir: &Path) -> anyhow::Result<Vec<Strategy>> {
    let dir = dir(data_dir);
    let mut own = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(builtin().collect()),
        Err(err) => return Err(anyhow::Error::new(err).context(dir.display().to_string())),
    };
    for entry in entries {
        let path = entry?.path();
        let txt = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !txt || !path.is_file() {
            continue;
        }
        if is_builtin(name) {
            log::warn!(
                "{} is named like a built-in strategy: skipped",
                path.display()
            );
            continue;
        }
        let text = match zapret::read_text(&path) {
            Ok(text) => text,
            Err(err) => {
                log::warn!("{} cannot be read and is skipped: {err}", path.display());
                continue;
            }
        };
        own.push(Strategy {
            name: name.to_owned(),
            text: text.trim_start_matches('\u{feff}').to_owned(),
            builtin: false,
        });
    }
    own.sort_by_cached_key(|s| zapret::strategies::natural_key(&s.name));
    Ok(builtin().chain(own).collect())
}

fn builtin() -> impl Iterator<Item = Strategy> {
    BUILTIN.into_iter().map(|(name, text)| Strategy {
        name: name.to_owned(),
        text: text.to_owned(),
        builtin: true,
    })
}

fn is_builtin(name: &str) -> bool {
    BUILTIN
        .iter()
        .any(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
}

/// `name` without the prefix.
pub fn find(data_dir: &Path, name: &str) -> anyhow::Result<Strategy> {
    list(data_dir)?
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("no zapret2 strategy named {name}"))
}

/// The name a save writes under (spec 5.2): a valid one, not a built-in one; new for `create`,
/// one of the user's otherwise.
pub fn claim(data_dir: &Path, name: &str, create: bool) -> anyhow::Result<String> {
    let name = zapret::custom::validate_name(name)?;
    anyhow::ensure!(
        !is_builtin(&name),
        "{PREFIX}{name} is built in: save a copy under another name"
    );
    let exists = list(data_dir)?
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name));
    if create {
        anyhow::ensure!(!exists, "the name {name} is taken");
    } else {
        anyhow::ensure!(exists, "no zapret2 strategy of yours named {name}");
    }
    Ok(name)
}

/// `data/zapret2/<name>.txt`, the text as it came.
pub fn write(data_dir: &Path, name: &str, text: &str) -> anyhow::Result<()> {
    let dir = dir(data_dir);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let path = dir.join(format!("{name}.txt"));
    std::fs::write(&path, text).with_context(|| path.display().to_string())
}

/// The user's own, and not `active` (the registry's strategy name).
pub fn delete(data_dir: &Path, name: &str, active: Option<&str>) -> anyhow::Result<()> {
    let strategy = find(data_dir, name)?;
    let full = strategy.full_name();
    anyhow::ensure!(!strategy.builtin, "{full} is built in: it stays");
    anyhow::ensure!(
        !active.is_some_and(|active| active.eq_ignore_ascii_case(&full)),
        "{full} is the active strategy: apply another one first"
    );
    let path = dir(data_dir).join(format!("{}.txt", strategy.name));
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
/// space-separated options are not seen. `--intercept=0` (step 3) is the real check.
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

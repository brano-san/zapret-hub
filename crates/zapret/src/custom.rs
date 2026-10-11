//! Custom strategies (spec 6). A TOML record in `data/strategies/` is the source of truth; a
//! `general (HUB <name>).bat` in Flowseal's format lets `service.bat` and `test zapret.ps1` use it.

use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::batparse::ParsedArgs;
use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CustomStrategy {
    /// What follows `HUB ` in the file name.
    pub name: String,
    /// The strategy it was cloned from.
    pub cloned_from: String,
    /// `--wf-tcp=... --wf-udp=...`: what winws intercepts at all.
    pub wf: String,
    /// One entry per `--new` block, as the `.bat` has it (`%BIN%`, `%LISTS%`, `%GameFilter…%` kept).
    pub blocks: Vec<String>,
}

/// The lines before the winws command in every Flowseal 1.10.3 strategy.
const HEADER: &str = concat!(
    "@echo off\r\n",
    "chcp 65001 > nul\r\n",
    ":: 65001 - UTF-8\r\n",
    "\r\n",
    "cd /d \"%~dp0\"\r\n",
    "call service.bat status_zapret\r\n",
    "call service.bat check_updates\r\n",
    "call service.bat load_game_filter\r\n",
    "call service.bat load_user_lists\r\n",
    "echo:\r\n",
    "\r\n",
    "set \"BIN=%~dp0bin\\\"\r\n",
    "set \"LISTS=%~dp0lists\\\"\r\n",
    "cd /d %BIN%\r\n",
    "\r\n",
);

impl CustomStrategy {
    /// `general (HUB <name>)`: the strategy name `service.bat` and the registry see.
    pub fn file_stem(&self) -> String {
        format!("general (HUB {})", self.name)
    }

    /// Flowseal's header, then `start ... winws.exe` with one block per line.
    pub fn to_bat(&self) -> String {
        let mut bat = String::from(HEADER);
        bat.push_str("start \"zapret: %~n0\" /min \"%BIN%winws.exe\" ");
        bat.push_str(&self.wf);
        for (i, block) in self.blocks.iter().enumerate() {
            bat.push_str(" ^\r\n");
            bat.push_str(block);
            if i + 1 < self.blocks.len() {
                bat.push_str(" --new");
            }
        }
        bat.push_str("\r\n");
        bat
    }

    /// Editor text made fit for one `.bat` line: line breaks become spaces, empty blocks go,
    /// cmd's special characters and a typed `--new` are refused.
    pub fn normalized(&self) -> anyhow::Result<Self> {
        let wf = one_line(&self.wf)?;
        anyhow::ensure!(
            !wf.is_empty()
                && wf
                    .split_whitespace()
                    .all(|token| token.starts_with("--wf-")),
            "the first line holds only --wf-* options: {wf}"
        );
        let mut blocks = Vec::new();
        for block in &self.blocks {
            let block = one_line(block)?;
            anyhow::ensure!(
                !block.split_whitespace().any(|token| token == "--new"),
                "split blocks with the block buttons, not with --new"
            );
            if !block.is_empty() {
                blocks.push(block);
            }
        }
        anyhow::ensure!(!blocks.is_empty(), "a strategy needs at least one block");
        Ok(Self {
            name: validate_name(&self.name)?,
            cloned_from: self.cloned_from.clone(),
            wf,
            blocks,
        })
    }
}

fn one_line(text: &str) -> anyhow::Result<String> {
    // `^!` is Flowseal's literal `!` (`--dpi-desync-fake-tls=^!`); any other `^` is refused.
    let checked = text.replace("^!", "");
    if let Some(c) = checked.chars().find(|c| "&|<>^".contains(*c)) {
        anyhow::bail!("the character {c} is not allowed in winws arguments");
    }
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" "))
}

/// 1-40 characters: Latin letters, digits, space, `-`, `_`, `.`; a letter or digit first; not a
/// Windows device name (the record is `<name>.toml`).
pub fn validate_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    let ok = (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    anyhow::ensure!(
        ok,
        "a name is 1-40 Latin letters, digits, spaces, '-', '_' or '.', starting with a letter or digit: {name}"
    );
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    anyhow::ensure!(
        !device,
        "{name} is a Windows device name: choose another name"
    );
    Ok(name.to_owned())
}

/// The `--wf-*` options and the `--new` blocks of a strategy file's winws command.
pub fn split_bat(bat: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut lines = bat.lines();
    let mut line = lines
        .by_ref()
        .find_map(|line| crate::batparse::find_ci(line, "winws.exe\"").map(|end| &line[end..]))
        .ok_or_else(|| anyhow::anyhow!("no winws.exe command in the strategy file"))?;
    let mut command = String::new();
    loop {
        let trimmed = line.trim_end();
        let Some(rest) = trimmed.strip_suffix('^') else {
            command.push_str(trimmed);
            break;
        };
        command.push_str(rest);
        command.push(' ');
        match lines.next() {
            Some(next) => line = next,
            None => break,
        }
    }
    let tokens = quoted_tokens(&command);
    let wf_len = tokens
        .iter()
        .take_while(|token| token.starts_with("--wf-"))
        .count();
    let blocks = tokens[wf_len..]
        .split(|token| token == "--new")
        .map(|block| block.join(" "))
        .filter(|block| !block.is_empty())
        .collect();
    Ok((tokens[..wf_len].join(" "), blocks))
}

/// Whitespace-separated; whitespace inside quotes does not split.
pub fn quoted_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// `winws --dry-run <args>` when this winws knows the option; otherwise winws runs for 2 s and
/// must not exit (spec 6). An error carries winws's whole output.
pub fn dry_run(folder: &Folder, parsed: &ParsedArgs) -> anyhow::Result<String> {
    let tail = parsed.command_tail();
    if std::fs::read(folder.winws())?
        .windows(9)
        .any(|bytes| bytes == b"--dry-run")
    {
        let mut cmd = Command::new(folder.winws());
        cmd.raw_arg(format!("--dry-run{tail}"))
            .current_dir(folder.bin());
        let out = exec::run(&mut cmd, Duration::from_secs(15), None)?.check()?;
        return Ok(format!("{}{}", out.stdout, out.stderr));
    }
    let mut winws = crate::procs::Winws::start(&folder.winws(), &folder.bin(), &tail)?;
    let ready = winws.wait_ready(Duration::from_secs(2));
    let output = winws.stop();
    ready.map_err(anyhow::Error::msg)?;
    Ok(output)
}

/// Normalized, parsed as `service.bat` would, accepted by winws.
pub fn validate(
    folder: &Folder,
    s: &CustomStrategy,
) -> anyhow::Result<(CustomStrategy, ParsedArgs)> {
    let s = s.normalized()?;
    let parsed = crate::batparse::parse(
        &s.to_bat(),
        &folder.root_dp0(),
        &crate::game_filter::try_read(folder)?.vars(),
    )?;
    dry_run(folder, &parsed)?;
    Ok((s, parsed))
}

/// `data/strategies/<name>.toml`.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Every record by name. A record that does not parse, or whose name is not one `validate_name`
    /// gives, is an error naming its file: the name becomes file names in the Flowseal folder and
    /// here, so a record written by hand must not reach outside them.
    pub fn list(&self) -> anyhow::Result<Vec<CustomStrategy>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut records = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            let record: CustomStrategy =
                toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            anyhow::ensure!(
                validate_name(&record.name).is_ok_and(|name| name == record.name),
                "{}: {:?} is not a valid strategy name",
                path.display(),
                record.name
            );
            records.push(record);
        }
        records.sort_by_key(|s| s.name.to_lowercase());
        Ok(records)
    }

    pub fn get(&self, name: &str) -> anyhow::Result<CustomStrategy> {
        self.list()?
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("no custom strategy named {name}"))
    }

    pub fn save(&self, s: &CustomStrategy) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.path(&s.name), toml::to_string_pretty(s)?)?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> anyhow::Result<()> {
        let s = self.get(name)?;
        std::fs::remove_file(self.path(&s.name))?;
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.toml"))
    }
}

fn bat_path(folder: &Folder, s: &CustomStrategy) -> PathBuf {
    folder.root().join(format!("{}.bat", s.file_stem()))
}

pub fn write_bat(folder: &Folder, s: &CustomStrategy) -> anyhow::Result<()> {
    let path = bat_path(folder, s);
    std::fs::write(&path, s.to_bat())?;
    log::info!("custom strategy written: {}", path.display());
    Ok(())
}

/// Checks `s`'s name against the store, validates it, then writes its record and its `.bat`. `new`
/// refuses a name already in use (a record or a file); otherwise the record must exist. A name that
/// is taken or missing, or a record that does not parse, is reported before winws runs.
pub fn save(
    folder: &Folder,
    store: &Store,
    s: &CustomStrategy,
    new: bool,
) -> anyhow::Result<CustomStrategy> {
    let s = s.normalized()?;
    // `list`, not `get`: a record that cannot be read is its own error, naming the file.
    let exists = store
        .list()?
        .iter()
        .any(|record| record.name.eq_ignore_ascii_case(&s.name));
    if new {
        anyhow::ensure!(
            !exists && !bat_path(folder, &s).exists(),
            "the name {} is taken",
            s.name
        );
    } else {
        anyhow::ensure!(exists, "no custom strategy named {}", s.name);
    }
    let (s, _) = validate(folder, &s)?;
    store.save(&s)?;
    write_bat(folder, &s)?;
    Ok(s)
}

/// «Клонировать»: a new custom strategy with `source`'s filter and blocks.
pub fn clone_from(
    folder: &Folder,
    store: &Store,
    source: &str,
    name: &str,
) -> anyhow::Result<CustomStrategy> {
    let file = crate::strategies::find(folder, source)?;
    let (wf, blocks) = split_bat(&crate::read_text(&file.path)?)?;
    save(
        folder,
        store,
        &CustomStrategy {
            name: name.to_owned(),
            cloned_from: file.name,
            wf,
            blocks,
        },
        true,
    )
}

/// Record and file. `active`, the registry's strategy name, cannot be deleted (spec 6).
pub fn delete(
    folder: &Folder,
    store: &Store,
    name: &str,
    active: Option<&str>,
) -> anyhow::Result<()> {
    let s = store.get(name)?;
    anyhow::ensure!(
        !active.is_some_and(|active| active.eq_ignore_ascii_case(&s.file_stem())),
        "{} is the active strategy: apply another one first",
        s.file_stem()
    );
    let bat = bat_path(folder, &s);
    if bat.exists() {
        std::fs::remove_file(&bat)?;
    }
    store.remove(&s.name)
}

/// A Flowseal update replaces the folder: the missing `.bat` files are written again (spec 6).
pub fn materialize_all(folder: &Folder, store: &Store) -> anyhow::Result<Vec<String>> {
    let mut restored = Vec::new();
    for s in store.list()? {
        if !bat_path(folder, &s).exists() {
            write_bat(folder, &s)?;
            restored.push(s.name);
        }
    }
    Ok(restored)
}

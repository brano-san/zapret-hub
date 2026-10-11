//! Port of the argument parser in `service.bat :service_install`: a strategy `.bat` becomes the
//! exact `winws` arguments the service is created with. Golden files (tests/golden.rs) hold it to
//! byte equality with the batch original.

use std::path::Path;

/// `%GameFilter%`, `%GameFilterTCP%`, `%GameFilterUDP%`, as `:game_switch_status` sets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFilterVars {
    pub all: String,
    pub tcp: String,
    pub udp: String,
}

impl GameFilterVars {
    /// Game filter off: the placeholder port `12` everywhere.
    pub fn disabled() -> Self {
        Self {
            all: "12".into(),
            tcp: "12".into(),
            udp: "12".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    /// What `service.bat` prints after `Final args: `, quotes still escaped as `\"`.
    pub final_args: String,
}

impl ParsedArgs {
    /// The arguments as the service process gets them: `sc` turns every `\"` into `"`.
    pub fn command_tail(&self) -> String {
        self.final_args.replace("\\\"", "\"")
    }

    /// The service `binPath`: `"<bin>\winws.exe"` and the tail, spacing kept.
    pub fn bin_path(&self, winws: &Path) -> String {
        format!("\"{}\" {}", winws.display(), self.command_tail())
    }

    /// Identifies these exact arguments in test results.
    pub fn sha256(&self) -> String {
        crate::sha256_hex(self.final_args.as_bytes())
    }
}

/// Parses `bat` the way `service.bat` does for a folder whose `%~dp0` is `root_dp0`.
pub fn parse(bat: &str, root_dp0: &str, vars: &GameFilterVars) -> anyhow::Result<ParsedArgs> {
    let bin = format!("{root_dp0}bin\\");
    let lists = format!("{root_dp0}lists\\");
    let mut args = String::new();
    let mut capture = false;
    // 2 = an option was just written, 1 = its values follow (joined by `,`),
    // 3 = the next value joins with `=` (after `sni`, `host`, `altorder`), 0 = plain.
    let mut merge = 0u8;
    for raw in bat.lines() {
        // `for /f "tokens=*"`: leading blanks go; empty lines and `;` lines are skipped.
        let line = raw.trim_start_matches([' ', '\t']);
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let line = if line.contains('!') {
            unescape_carets(line)
        } else {
            line.to_owned()
        };
        let mut line = expand(&line, root_dp0, &bin, &lists, vars);
        if !capture && line.to_ascii_lowercase().contains("winws.exe") {
            capture = true;
        }
        if !capture {
            continue;
        }
        if args.is_empty() {
            // `set "line=!line:*winws.exe"=!"`
            if let Some(end) = find_ci(&line, "winws.exe\"") {
                line = line[end..].to_owned();
            }
        }
        let mut temp = String::new();
        for token in for_tokens(&line) {
            if token == "^" || token == "^^" {
                continue;
            }
            let mut arg = token;
            if arg.starts_with("--") && merge != 0 {
                merge = 0;
            }
            if arg.starts_with('"') {
                arg = quote_arg(&arg, root_dp0);
            }
            match merge {
                1 => {
                    temp.push(',');
                    temp.push_str(&arg);
                }
                3 => {
                    temp.push('=');
                    temp.push_str(&arg);
                    merge = 1;
                }
                _ => {
                    temp.push(' ');
                    temp.push_str(&arg);
                }
            }
            if arg.starts_with("--") {
                merge = 2;
            } else if merge >= 1 {
                if merge == 2 {
                    merge = 1;
                }
                if ["sni", "host", "altorder"]
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(&arg))
                {
                    merge = 3;
                }
            }
        }
        if !temp.is_empty() {
            args.push(' ');
            args.push_str(&temp);
        }
    }
    anyhow::ensure!(capture, "no winws.exe command in the strategy file");
    Ok(ParsedArgs { final_args: args })
}

/// `set "line=%%a"` under delayed expansion: in a line that has a `!`, cmd reads every `^` as an
/// escape and keeps the character after it (`--dpi-desync-fake-tls=^!` → `=!`); a trailing `^` goes.
fn unescape_carets(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '^' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    out
}

/// Byte offset right after the first case-insensitive occurrence of an ASCII `needle`.
pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
        .map(|i| i + needle.len())
}

/// The `%` pass of `call set "line=!line!"`, for the variables strategies use.
fn expand(line: &str, root_dp0: &str, bin: &str, lists: &str, vars: &GameFilterVars) -> String {
    let mut out = replace_ci(line, "%~dp0", root_dp0);
    for (name, value) in [
        ("%BIN%", bin),
        ("%LISTS%", lists),
        ("%GameFilterTCP%", vars.tcp.as_str()),
        ("%GameFilterUDP%", vars.udp.as_str()),
        ("%GameFilter%", vars.all.as_str()),
    ] {
        out = replace_ci(&out, name, value);
    }
    out
}

/// Every case-insensitive occurrence of an ASCII `needle` replaced, as cmd expands `%VAR%`.
pub fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let lower = hay.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(hay.len());
    let mut last = 0;
    for (start, _) in lower.match_indices(&needle_lower) {
        out.push_str(&hay[last..start]);
        out.push_str(with);
        last = start + needle.len();
    }
    out.push_str(&hay[last..]);
    out
}

/// Tokens of `for %%i in (!line!)`: split on space, tab, `,`, `;`, `=` outside quotes, quotes kept.
fn for_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
        } else if !quoted && matches!(c, ' ' | '\t' | ',' | ';' | '=') {
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

/// A quoted token: `"X"` with a `:` stays absolute, `"@X"` and `"X"` become root-relative;
/// the quotes come back escaped as `\"` (`set "arg=\!QUOTE!...\!QUOTE!"`).
fn quote_arg(arg: &str, root_dp0: &str) -> String {
    // `!arg:~1,-1!`: drop the first and the last character.
    let mut chars = arg.chars();
    chars.next();
    chars.next_back();
    let inner = chars.as_str();
    if inner.contains(':') {
        format!("\\\"{inner}\\\"")
    } else if let Some(rest) = inner.strip_prefix('@') {
        format!("\\\"@{root_dp0}{rest}\\\"")
    } else {
        format!("\\\"{root_dp0}{inner}\\\"")
    }
}

/// A strategy file with the folder's current game filter, as «Применить» and the tests see it.
pub fn parse_file(folder: &crate::folder::Folder, path: &Path) -> anyhow::Result<ParsedArgs> {
    let bat = crate::read_text(path)?;
    parse(
        &bat,
        &folder.root_dp0(),
        &crate::game_filter::try_read(folder)?.vars(),
    )
    .map_err(|err| err.context(path.display().to_string()))
}

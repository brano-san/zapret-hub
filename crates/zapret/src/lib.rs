//! Platform utilities of the hub: the services, root rights (Linux), the processes, hosts,
//! releases, the snapshot and the system checks.

pub mod checks;
pub mod discord_cache;
pub mod hosts;
pub mod procs;
pub mod release;
#[cfg(target_os = "linux")]
pub mod root;
pub mod service;
pub mod snapshot;
pub mod targets;

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

/// `$XDG_DATA_HOME` when set and absolute, else `~/.local/share` (empty when `HOME` is unset).
#[cfg(target_os = "linux")]
pub fn data_home() -> PathBuf {
    data_home_of(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

#[cfg(target_os = "linux")]
fn data_home_of(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> PathBuf {
    xdg.map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_default()
}

/// A text file: UTF-8, where broken bytes become U+FFFD, or UTF-16LE when it starts with the BOM
/// `FF FE`, as PowerShell 5.1's `>` and Notepad's "Unicode" write it (decoded without the BOM).
/// A UTF-8 BOM stays in the text: the caller strips it where it matters.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(match bytes.strip_prefix(&[0xFF, 0xFE]) {
        Some(utf16) => String::from_utf16_lossy(
            &utf16
                .chunks(2)
                .map(|pair| match *pair {
                    [low, high] => u16::from_le_bytes([low, high]),
                    // A byte left over after the last pair.
                    _ => char::REPLACEMENT_CHARACTER as u16,
                })
                .collect::<Vec<_>>(),
        ),
        None => String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

/// The arguments of a command line for `Command::args`: tokens as `quoted_tokens` splits them,
/// their quotes dropped (Linux has no `raw_arg`).
#[cfg(target_os = "linux")]
pub fn spawn_tokens(text: &str) -> Vec<String> {
    quoted_tokens(text)
        .into_iter()
        .map(|token| token.replace('"', ""))
        .filter(|token| !token.is_empty())
        .collect()
}

/// 1-40 characters: Latin letters, digits, space, `-`, `_`, `.`; a letter or digit first; not a
/// Windows device name (the name becomes a file).
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

/// `[Regex]::Replace($_.Name, '(d+)', { $args[0].Value.PadLeft(8, '0') })`, lower-cased: `2`
/// sorts before `10`.
// ponytail: ordinal compare of the padded name; PowerShell compares by culture, which orders
// ASCII names the same way.
pub fn natural_key(file_name: &str) -> String {
    let mut key = String::with_capacity(file_name.len() + 16);
    let mut digits = String::new();
    for c in file_name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        flush_digits(&mut key, &mut digits);
        key.extend(c.to_lowercase());
    }
    flush_digits(&mut key, &mut digits);
    key
}

fn flush_digits(key: &mut String, digits: &mut String) {
    if digits.is_empty() {
        return;
    }
    for _ in digits.len()..8 {
        key.push('0');
    }
    key.push_str(digits);
    digits.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn spawn_tokens_drop_the_quotes_of_a_command_line() {
        assert_eq!(
            spawn_tokens(r#"--chdir="/a b" --x """#),
            ["--chdir=/a b", "--x"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn data_home_prefers_an_absolute_xdg_then_home() {
        let some = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(
            data_home_of(some("/data"), some("/home/me")),
            PathBuf::from("/data")
        );
        assert_eq!(
            data_home_of(some("rel"), some("/home/me")),
            PathBuf::from("/home/me/.local/share")
        );
        assert_eq!(data_home_of(None, None), PathBuf::new());
    }

    fn read(bytes: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.txt");
        std::fs::write(&path, bytes).unwrap();
        read_text(&path).unwrap()
    }

    /// `text` as PowerShell 5.1's `>` and Notepad's "Unicode" write it: the BOM `FF FE`, UTF-16LE.
    fn utf16le(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn a_utf16le_file_with_a_bom_is_decoded_without_the_bom() {
        let text = "# Привет\r\n--wf-tcp-out=443\r\n--emoji=😀\r\n";
        assert_eq!(read(&utf16le(text)), text);
        assert_eq!(read(&[0xFF, 0xFE]), "");
        // A byte left over after the last pair is a broken one, like any other.
        assert_eq!(read(&[0xFF, 0xFE, b'A', 0, b'B']), "A\u{FFFD}");
        // An unpaired surrogate too.
        assert_eq!(read(&[0xFF, 0xFE, b'A', 0, 0x3D, 0xD8]), "A\u{FFFD}");
    }

    #[test]
    fn everything_else_is_read_as_utf8() {
        assert_eq!(read("Привет\r\n".as_bytes()), "Привет\r\n");
        // A UTF-8 BOM stays for the caller to strip.
        assert_eq!(read(b"\xEF\xBB\xBFx"), "\u{feff}x");
        // Broken bytes become U+FFFD; FF without FE is no BOM.
        assert_eq!(read(b"a\xFFb"), "a\u{FFFD}b");
        assert_eq!(read(b"\xFFA"), "\u{FFFD}A");
    }

    #[test]
    fn replace_ci_mimics_how_cmd_expands_a_variable() {
        assert_eq!(replace_ci("%List%-%list%", "%List%", "x"), "x-x");
        assert_eq!(
            replace_ci("a%LIST%s %LIST2%", "%List%", "bc"),
            "abcs %LIST2%"
        );
    }

    #[test]
    fn quoted_tokens_keep_quoted_spaces() {
        assert_eq!(
            quoted_tokens(r#"-a "one two" --b"#),
            vec!["-a", "\"one two\"", "--b"]
        );
    }

    #[test]
    fn validate_names_accept_a_plain_name_and_refuse_the_rest() {
        assert_eq!(validate_name(" ALT-2.bat ").unwrap(), "ALT-2.bat");
        assert!(validate_name("").is_err());
        assert!(validate_name("конский").is_err());
        assert!(validate_name("CON").is_err());
        assert!(validate_name("COM9").is_err());
        assert!(validate_name(&"x".repeat(41)).is_err());
    }

    #[test]
    fn a_run_of_digits_sorts_smaller_numbers_first() {
        assert!(natural_key("general (ALT2)") < natural_key("general (ALT10)"));
        assert_eq!(natural_key("General.txt"), "general.txt");
    }
}

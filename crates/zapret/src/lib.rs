//! The Flowseal `zapret-discord-youtube` folder as `service.bat` sees it.

pub mod batparse;
pub mod checks;
pub mod custom;
pub mod discord_cache;
pub mod fakes;
pub mod flags;
pub mod folder;
pub mod game_filter;
pub mod hosts;
pub mod ipset;
pub mod procs;
pub mod release;
pub mod service;
pub mod snapshot;
pub mod strategies;
pub mod targets;
pub mod updates;
pub mod user_lists;

use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

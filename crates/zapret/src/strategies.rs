//! Strategy `.bat` files in the folder root, in the order of `service.bat`'s menu.

use std::path::PathBuf;

use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyFile {
    /// File stem: the name `service.bat` writes to the registry, `general (ALT11)`.
    pub name: String,
    pub path: PathBuf,
}

/// Every `*.bat` except `service*`, sorted as `service.bat` sorts them.
pub fn list(folder: &Folder) -> std::io::Result<Vec<StrategyFile>> {
    let mut files: Vec<(String, StrategyFile)> = Vec::new();
    for entry in std::fs::read_dir(folder.root())? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = file_name.to_lowercase();
        if !lower.ends_with(".bat") || lower.starts_with("service") || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        files.push((natural_key(file_name), StrategyFile { name, path }));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files.into_iter().map(|(_, file)| file).collect())
}

pub fn find(folder: &Folder, name: &str) -> anyhow::Result<StrategyFile> {
    list(folder)?
        .into_iter()
        .find(|file| file.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("strategy {name} not found in {}", folder.root().display()))
}

/// `[Regex]::Replace($_.Name, '(\d+)', { $args[0].Value.PadLeft(8, '0') })`, lower-cased.
// ponytail: ordinal compare of the padded name; PowerShell compares by culture, which orders
// Flowseal's ASCII names the same way.
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

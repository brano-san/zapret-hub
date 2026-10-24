//! The XDG autostart entry `~/.config/autostart/zapret-hub.desktop`: at login the desktop runs
//! `zapret-hub --autostart` from this exe's path.

use std::path::{Path, PathBuf};

fn entry_path() -> PathBuf {
    zapret::config_home()
        .join("autostart")
        .join("zapret-hub.desktop")
}

/// `exe` as one quoted argument of `Exec=`: the quoting rule escapes `"`, `` ` ``, `$` and `\`,
/// the string rule then doubles every `\`, and `%` (a field code) is doubled.
fn exec_arg(exe: &Path) -> String {
    let mut quoted = String::new();
    for c in exe.display().to_string().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    format!("\"{}\"", quoted.replace('\\', "\\\\").replace('%', "%%"))
}

/// The `[Desktop Entry]` that starts `exe`.
pub fn entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment=Zapret Hub at login\n\
         Exec={} --autostart\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        crate::texts::APP,
        exec_arg(exe)
    )
}

/// The program an entry's `Exec=` starts: its first, quoted argument, unescaped. `None` for an
/// entry the desktop is told to skip (`Hidden=true`, GNOME's switch off), as the system's own
/// settings leave it.
pub fn command_of(entry: &str) -> Option<String> {
    let value = |key: &str| {
        entry
            .lines()
            .find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))
            .map(str::trim)
    };
    if value("Hidden") == Some("true") || value("X-GNOME-Autostart-enabled") == Some("false") {
        return None;
    }
    // The string rule first: `\\` is one `\`.
    let exec = value("Exec")?.replace("\\\\", "\\");
    let mut chars = exec.strip_prefix('"')?.chars();
    let mut command = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(command),
            '\\' => command.push(chars.next()?),
            '%' => {
                chars.next().filter(|&c| c == '%')?;
                command.push('%');
            }
            c => command.push(c),
        }
    }
}

/// The autostart as it is on the disk: the entry is there, on, and runs this exe. The user may
/// switch it off in the desktop's settings, so the file, not the saved flag, is the truth.
pub fn is_enabled(exe: &Path) -> bool {
    std::fs::read_to_string(entry_path())
        .ok()
        .and_then(|text| command_of(&text))
        .is_some_and(|command| Path::new(&command) == exe)
}

pub fn enable(exe: &Path) -> anyhow::Result<()> {
    let path = entry_path();
    anyhow::ensure!(
        path.is_absolute(),
        "HOME is not set: there is no autostart folder"
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, entry(exe))?;
    Ok(())
}

pub fn disable() -> anyhow::Result<()> {
    match std::fs::remove_file(entry_path()) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// The exe moved since the entry was made: it is made again for `exe`. An entry that is gone or
/// switched off stays so: that is the user's choice in the desktop's settings.
pub fn refresh(exe: &Path) -> anyhow::Result<()> {
    let command = std::fs::read_to_string(entry_path())
        .ok()
        .and_then(|text| command_of(&text));
    match command {
        Some(command) if Path::new(&command) != exe => enable(exe),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_runs_this_exe_at_login() {
        let exe = Path::new("/opt/Tools & Apps/zapret-hub");
        let text = entry(exe);
        assert!(
            text.contains("Exec=\"/opt/Tools & Apps/zapret-hub\" --autostart\n"),
            "{text}"
        );
        assert!(
            text.starts_with("[Desktop Entry]\nType=Application\n"),
            "{text}"
        );
        assert_eq!(
            command_of(&text).as_deref(),
            Some("/opt/Tools & Apps/zapret-hub")
        );
    }

    #[test]
    fn quotes_dollars_backslashes_and_percents_are_escaped_and_come_back() {
        let exe = Path::new("/home/me/a\"b$c`d\\e%f/zapret-hub");
        let text = entry(exe);
        assert!(
            text.contains(r#"Exec="/home/me/a\\"b\\$c\\`d\\\\e%%f/zapret-hub" --autostart"#),
            "{text}"
        );
        assert_eq!(
            command_of(&text).as_deref(),
            Some("/home/me/a\"b$c`d\\e%f/zapret-hub")
        );
    }

    #[test]
    fn an_entry_switched_off_in_the_desktop_settings_is_off() {
        let on = entry(Path::new("/usr/bin/zapret-hub"));
        assert!(command_of(&on).is_some());
        let gnome = on.replace(
            "X-GNOME-Autostart-enabled=true",
            "X-GNOME-Autostart-enabled=false",
        );
        assert_eq!(command_of(&gnome), None);
        assert_eq!(command_of(&format!("{on}Hidden=true\n")), None);
        assert_eq!(command_of("[Desktop Entry]\nExec=zapret-hub\n"), None);
    }
}

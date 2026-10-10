//! `utils\targets.txt`, read the way `test zapret.ps1` reads it.

use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// `https://...` for the HTTP/TLS checks; `None` for a ping-only target.
    pub url: Option<String>,
    /// Host or IP the ping goes to.
    pub ping: String,
}

/// `targets.txt`, or the script's built-in list when the file is missing or empty.
pub fn load(folder: &Folder) -> Vec<Target> {
    let parsed = crate::read_text(&folder.utils().join("targets.txt"))
        .map(|text| parse(&text))
        .unwrap_or_default();
    if parsed.is_empty() {
        defaults()
    } else {
        parsed
    }
}

/// Lines `Key = "value"`; a repeated key keeps its first place and takes the last value
/// (`OrderedDictionary` + `Add-OrSet`).
pub fn parse(text: &str) -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = parse_line(line) else {
            continue;
        };
        let target = convert(key, value);
        match targets.iter_mut().find(|t| t.name == key) {
            Some(existing) => *existing = target,
            None => targets.push(target),
        }
    }
    targets
}

/// `^\s*(\w+)\s*=\s*"(.+)"\s*$`
fn parse_line(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let value = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
    (!value.is_empty()).then_some((key, value))
}

/// `Convert-Target`: `PING:1.2.3.4` is ping-only; a URL pings its host.
fn convert(name: &str, value: &str) -> Target {
    if value
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("PING:"))
    {
        return Target {
            name: name.to_owned(),
            url: None,
            ping: value[5..].trim_start().to_owned(),
        };
    }
    Target {
        name: name.to_owned(),
        url: Some(value.to_owned()),
        ping: url_host(value).to_owned(),
    }
}

/// `https://host/path` → `host` (`-replace "^https?://", "" -replace "/.*$", ""`).
pub fn url_host(url: &str) -> &str {
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            url.get(..scheme.len())
                .filter(|p| p.eq_ignore_ascii_case(scheme))
                .map(|_| &url[scheme.len()..])
        })
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest)
}

/// The list `test zapret.ps1` falls back to.
pub fn defaults() -> Vec<Target> {
    [
        ("Discord Main", "https://discord.com"),
        ("Discord Gateway", "https://gateway.discord.gg"),
        ("Discord CDN", "https://cdn.discordapp.com"),
        ("Discord Updates", "https://updates.discord.com"),
        ("YouTube Web", "https://www.youtube.com"),
        ("YouTube Short", "https://youtu.be"),
        ("YouTube Image", "https://i.ytimg.com"),
        (
            "YouTube Video Redirect",
            "https://redirector.googlevideo.com",
        ),
        ("Google Main", "https://www.google.com"),
        ("Google Gstatic", "https://www.gstatic.com"),
        ("Cloudflare Web", "https://www.cloudflare.com"),
        ("Cloudflare CDN", "https://cdnjs.cloudflare.com"),
        ("Cloudflare DNS 1.1.1.1", "PING:1.1.1.1"),
        ("Cloudflare DNS 1.0.0.1", "PING:1.0.0.1"),
        ("Google DNS 8.8.8.8", "PING:8.8.8.8"),
        ("Google DNS 8.8.4.4", "PING:8.8.4.4"),
        ("Quad9 DNS 9.9.9.9", "PING:9.9.9.9"),
    ]
    .into_iter()
    .map(|(name, value)| convert(name, value))
    .collect()
}

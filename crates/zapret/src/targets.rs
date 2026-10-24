//! The targets of the tests and the quick check: the list Flowseal's `test zapret.ps1` falls back to.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// `https://...` for the HTTP/TLS checks; `None` for a ping-only target.
    pub url: Option<String>,
    /// Host or IP the ping goes to.
    pub ping: String,
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

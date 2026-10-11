//! `curl.exe` requests exactly as `test zapret.ps1` makes them, the DPI 16-20 request, downloads.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckKind {
    Http,
    Tls12,
    Tls13,
}

impl CheckKind {
    pub const ALL: [CheckKind; 3] = [Self::Http, Self::Tls12, Self::Tls13];

    /// The label `test zapret.ps1` prints.
    pub fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Tls12 => "TLS1.2",
            Self::Tls13 => "TLS1.3",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Http => &["--http1.1"],
            Self::Tls12 => &["--tlsv1.2", "--tls-max", "1.2"],
            Self::Tls13 => &["--tlsv1.3", "--tls-max", "1.3"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckStatus {
    Ok,
    Ssl,
    Unsup,
    Error,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CheckResult {
    pub kind: CheckKind,
    pub status: CheckStatus,
    /// `%{http_code}`; `000` when no response came.
    pub http_code: String,
    /// `%{time_total}` in ms, when curl printed one.
    pub time_ms: Option<u32>,
    pub stderr: String,
}

/// `-I -s -m 4 --connect-timeout 2 -o NUL -w "%{http_code} %{time_total}" --show-error`, then the
/// kind's arguments and the URL. Only `%{time_total}` is added to the script's request.
pub fn check(url: &str, kind: CheckKind, cancel: Option<&AtomicBool>) -> CheckResult {
    let mut cmd = Command::new("curl.exe");
    cmd.args([
        "-I",
        "-s",
        "-m",
        "4",
        "--connect-timeout",
        "2",
        "-o",
        "NUL",
        "-w",
        "%{http_code} %{time_total}",
        "--show-error",
    ])
    .args(kind.args())
    .arg(url);
    match exec::run(&mut cmd, Duration::from_secs(15), cancel) {
        Ok(out) => {
            let (http_code, time_ms) = parse_code_and_time(&out.stdout);
            CheckResult {
                kind,
                status: classify(out.code, &out.stderr),
                http_code,
                time_ms,
                stderr: out.stderr,
            }
        }
        Err(err) => CheckResult {
            kind,
            status: CheckStatus::Error,
            http_code: "000".into(),
            time_ms: None,
            stderr: err.to_string(),
        },
    }
}

/// The order of `test zapret.ps1`: certificate or DNS trouble, then "unsupported", then the exit code.
pub fn classify(code: Option<i32>, stderr: &str) -> CheckStatus {
    const SSL: [&str; 5] = [
        "could not resolve host",
        "certificate",
        "self-signed",
        "self signed",
        "selfsigned",
    ];
    const UNSUP: [&str; 8] = [
        "does not support",
        "not supported",
        "unsupported protocol",
        "unrecognized option",
        "unknown option",
        "unsupported option",
        "unsupported feature",
        "schannel",
    ];
    let err = stderr.to_ascii_lowercase();
    if SSL.iter().any(|p| err.contains(p)) {
        CheckStatus::Ssl
    } else if code == Some(35) || UNSUP.iter().any(|p| err.contains(p)) {
        CheckStatus::Unsup
    } else if code == Some(0) {
        CheckStatus::Ok
    } else {
        CheckStatus::Error
    }
}

/// `"200 0.153000"` → (`"200"`, `Some(153)`).
pub fn parse_code_and_time(stdout: &str) -> (String, Option<u32>) {
    let mut parts = stdout.split_whitespace();
    let code = parts.next().unwrap_or("000").to_owned();
    let time = parts
        .next()
        .and_then(|t| t.parse::<f64>().ok())
        .map(|s| (s * 1000.0).round() as u32);
    (code, time)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum DpiStatus {
    Ok,
    Fail,
    Unsupported,
    LikelyBlocked,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DpiResult {
    pub kind: CheckKind,
    pub status: DpiStatus,
    /// HTTP code, `UNSUP`, `ERR` or `NA`.
    pub code: String,
    pub up_bytes: u32,
    pub down_bytes: u32,
    /// `-1` when curl printed no time.
    pub time_s: f64,
}

pub const DPI_TIMEOUT_S: u32 = 5;
const DPI_RANGE_BYTES: usize = 65536;
const DPI_UNSUP: [&str; 9] = [
    "not supported",
    "does not support",
    "unsupported protocol",
    "unrecognized option",
    "unknown option",
    "unsupported option",
    "unsupported feature",
    "schannel",
    "ssl",
];

/// One request of `Invoke-DpiSuite`: POST 64 KiB, ask for 64 KiB back.
pub fn dpi_check(
    host: &str,
    kind: CheckKind,
    payload: &Path,
    cancel: Option<&AtomicBool>,
) -> DpiResult {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["--range", "0-65535", "-m", "5", "--connect-timeout", "3"])
        .args([
            "-w",
            "%{http_code} %{size_upload} %{size_download} %{time_total}",
            "-o",
            "NUL",
            "-X",
            "POST",
        ])
        .arg("--data-binary")
        .arg(format!("@{}", payload.display()))
        .arg("-s")
        .args(kind.args())
        .arg(format!("https://{host}"));
    let (code, text) = match exec::run(&mut cmd, Duration::from_secs(20), cancel) {
        Ok(out) => (out.code, format!("{}{}", out.stdout, out.stderr)),
        Err(err) => (None, err.to_string()),
    };
    dpi_classify(kind, code, text.trim(), DPI_TIMEOUT_S)
}

/// The verdict of `Invoke-DpiSuite`: something went up, nothing came down, the time ran out and
/// curl failed — the 16-20 KB freeze.
pub fn dpi_classify(kind: CheckKind, exit: Option<i32>, text: &str, timeout_s: u32) -> DpiResult {
    let mut result = DpiResult {
        kind,
        status: DpiStatus::Ok,
        code: "NA".into(),
        up_bytes: 0,
        down_bytes: 0,
        time_s: -1.0,
    };
    let lower = text.to_ascii_lowercase();
    if let Some((code, up, down, time)) = parse_w4(text) {
        (
            result.code,
            result.up_bytes,
            result.down_bytes,
            result.time_s,
        ) = (code, up, down, time);
    } else if exit == Some(35) || DPI_UNSUP.iter().any(|p| lower.contains(p)) {
        result.code = "UNSUP".into();
    } else if !text.is_empty() {
        result.code = "ERR".into();
    }
    result.status = if result.code == "UNSUP" {
        DpiStatus::Unsupported
    } else if exit != Some(0) || result.code == "ERR" || result.code == "NA" {
        DpiStatus::Fail
    } else {
        DpiStatus::Ok
    };
    if result.up_bytes > 0
        && result.down_bytes == 0
        && result.time_s >= f64::from(timeout_s)
        && exit != Some(0)
    {
        result.status = DpiStatus::LikelyBlocked;
    }
    result
}

/// `^(\d{3})\s+(\d+)\s+(\d+)\s+([\d.]+)$`
fn parse_w4(text: &str) -> Option<(String, u32, u32, f64)> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [code, up, down, time] = parts[..] else {
        return None;
    };
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !time.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some((
        code.to_owned(),
        up.parse().ok()?,
        down.parse().ok()?,
        time.parse().ok()?,
    ))
}

/// 64 KiB of random bytes for the DPI POST body.
pub fn write_payload(path: &Path) -> std::io::Result<()> {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let bytes: Vec<u8> = (0..DPI_RANGE_BYTES / 8)
        .flat_map(|i| state.hash_one(i).to_le_bytes())
        .collect();
    std::fs::write(path, bytes)
}

/// Windows 10 1803+ ships `curl.exe`; tests and updates need it.
pub fn available() -> bool {
    exec::run(
        Command::new("curl.exe").arg("--version"),
        Duration::from_secs(10),
        None,
    )
    .is_ok_and(|out| out.success())
}

/// `--ssl-no-revoke` for an old libcurl 7, `--ssl-revoke-best-effort` otherwise, as `:ipset_update` picks.
fn revoke_flag() -> &'static str {
    static FLAG: OnceLock<&'static str> = OnceLock::new();
    FLAG.get_or_init(|| {
        let version = exec::run(
            Command::new("curl.exe").arg("--version"),
            Duration::from_secs(10),
            None,
        )
        .map(|out| out.stdout)
        .unwrap_or_default();
        if version.contains("libcurl/7") {
            "--ssl-no-revoke"
        } else {
            "--ssl-revoke-best-effort"
        }
    })
}

/// `-L -f -s -S`, a 10 s connect timeout, the revocation flag, `no-cache`, through `socks5` when given.
pub fn get(url: &str, socks5: Option<&str>) -> Command {
    let mut cmd = Command::new("curl.exe");
    cmd.args([
        "-L",
        "-f",
        "-s",
        "-S",
        "--connect-timeout",
        "10",
        revoke_flag(),
        "-H",
        "Cache-Control: no-cache",
    ]);
    if let Some(proxy) = socks5 {
        cmd.args(["--socks5-hostname", proxy]);
    }
    cmd.arg(url);
    cmd
}

/// Tries directly, then once more through the proxy when there is one.
pub fn with_fallback<T>(
    socks5: Option<&str>,
    attempt: impl Fn(Option<&str>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    attempt(None).or_else(|direct| match socks5 {
        Some(proxy) => attempt(Some(proxy))
            .map_err(|via| anyhow::anyhow!("direct: {direct:#}\n\nthrough {proxy}: {via:#}")),
        None => Err(direct),
    })
}

/// A small text resource (version file, hosts, DPI suite).
pub fn fetch_text(url: &str, socks5: Option<&str>) -> anyhow::Result<String> {
    with_fallback(socks5, |proxy| {
        let mut cmd = get(url, proxy);
        cmd.args(["-m", "30"]);
        Ok(exec::run(&mut cmd, Duration::from_secs(40), None)?
            .check()?
            .stdout)
    })
}

/// A file (ipset list) straight to `dest`.
pub fn download(url: &str, dest: &Path, socks5: Option<&str>) -> anyhow::Result<()> {
    with_fallback(socks5, |proxy| {
        let mut cmd = get(url, proxy);
        cmd.args(["-m", "120", "-o"]).arg(dest);
        exec::run(&mut cmd, Duration::from_secs(130), None)?.check()?;
        Ok(())
    })
}

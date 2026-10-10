//! The local SOCKS5 proxy: does it answer, how fast, which ports to suggest.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Greeting `05 01 00` (no auth), answered with `05 00`.
pub fn handshake(addr: &str, timeout: Duration) -> bool {
    let Ok(addr) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut reply = [0u8; 2];
    stream.write_all(&[5, 1, 0]).is_ok() && stream.read_exact(&mut reply).is_ok() && reply == [5, 0]
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProxyStatus {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

/// `curl.exe --socks5-hostname <addr> -I https://www.gstatic.com/generate_204` (spec 8).
pub fn check(addr: &str) -> ProxyStatus {
    let mut cmd = Command::new("curl.exe");
    cmd.args([
        "-I",
        "-s",
        "-S",
        "-m",
        "8",
        "-o",
        "NUL",
        "-w",
        "%{http_code} %{time_total}",
        "--socks5-hostname",
        addr,
    ])
    .arg("https://www.gstatic.com/generate_204");
    match exec::run(&mut cmd, Duration::from_secs(15), None) {
        Ok(out) if out.success() => ProxyStatus {
            ok: true,
            latency_ms: crate::curl::parse_code_and_time(&out.stdout).1,
            error: None,
        },
        Ok(out) => ProxyStatus {
            ok: false,
            latency_ms: None,
            error: Some(out.stderr.trim().to_owned()),
        },
        Err(err) => ProxyStatus {
            ok: false,
            latency_ms: None,
            error: Some(err.to_string()),
        },
    }
}

/// Default SOCKS ports of known clients: NekoBox 2080, v2rayN 10808, Clash 7890.
pub const KNOWN_PORTS: [u16; 3] = [2080, 10808, 7890];

/// `inbound_socks_port` from `config\groups\nekobox.json` next to NekoBox's exe.
pub fn nekobox_port(exe_dir: &Path) -> Option<u16> {
    let text =
        std::fs::read_to_string(exe_dir.join("config").join("groups").join("nekobox.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    u16::try_from(json.get("inbound_socks_port")?.as_u64()?).ok()
}

/// `127.0.0.1:<port>` of every port that answers a SOCKS5 greeting, in the given order.
pub fn listening(ports: &[u16]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for port in ports {
        let addr = format!("127.0.0.1:{port}");
        if !out.contains(&addr) && handshake(&addr, Duration::from_millis(300)) {
            out.push(addr);
        }
    }
    out
}

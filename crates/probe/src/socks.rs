//! The local SOCKS5 proxy: does it answer, how fast, which ports to suggest.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
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
    let mut cmd = Command::new(crate::curl::CURL);
    cmd.args([
        "-I",
        "-s",
        "-S",
        "-m",
        "8",
        "-o",
        crate::curl::NULL,
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

/// Default SOCKS ports of common proxy clients: 2080, 10808 (v2rayN), 7890 (Clash).
pub const KNOWN_PORTS: [u16; 3] = [2080, 10808, 7890];

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

/// An A query for `dns.google`, id `7a 68`, recursion desired.
const UDP_CHECK_QUERY: [u8; 28] = [
    0x7a, 0x68, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 3, b'd', b'n', b's', 6, b'g', b'o', b'o', b'g',
    b'l', b'e', 0, 0, 1, 0, 1,
];

/// Does the proxy carry UDP: `UDP ASSOCIATE`, then a DNS query through the relay, twice.
/// `Some(false)`: refused, or the relay brought no answer; `None`: no SOCKS5 proxy answered.
pub fn relays_udp(addr: &str, timeout: Duration) -> Option<bool> {
    let proxy = addr.parse::<SocketAddr>().ok()?;
    let mut control = TcpStream::connect_timeout(&proxy, timeout).ok()?;
    control.set_read_timeout(Some(timeout)).ok()?;
    control.set_write_timeout(Some(timeout)).ok()?;
    let mut reply = [0u8; 2];
    control.write_all(&[5, 1, 0]).ok()?;
    control.read_exact(&mut reply).ok()?;
    if reply != [5, 0] {
        return None;
    }
    // From any address: the proxy learns ours from the first datagram.
    control.write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0]).ok()?;
    let mut head = [0u8; 4];
    control.read_exact(&mut head).ok()?;
    if head[1] != 0 {
        return Some(false);
    }
    let ip = match head[3] {
        1 => {
            let mut b = [0u8; 4];
            control.read_exact(&mut b).ok()?;
            IpAddr::from(b)
        }
        4 => {
            let mut b = [0u8; 16];
            control.read_exact(&mut b).ok()?;
            IpAddr::from(b)
        }
        _ => return Some(false),
    };
    let mut port = [0u8; 2];
    control.read_exact(&mut port).ok()?;
    // A proxy bound to every address names none: it is where we reached it.
    let ip = if ip.is_unspecified() { proxy.ip() } else { ip };
    let relay = SocketAddr::new(ip, u16::from_be_bytes(port));
    let sock = UdpSocket::bind(if relay.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .ok()?;
    sock.set_read_timeout(Some(timeout)).ok()?;
    // To 1.1.1.1:53.
    let mut packet = vec![0, 0, 0, 1, 1, 1, 1, 1, 0, 53];
    packet.extend_from_slice(&UDP_CHECK_QUERY);
    let mut buf = [0u8; 1500];
    for _ in 0..2 {
        sock.send_to(&packet, relay).ok()?;
        while let Ok((n, _)) = sock.recv_from(&mut buf) {
            // Header (10 bytes for an IPv4 source), then our id with the response bit.
            let r = &buf[..n];
            if r.len() >= 13 && r[10..12] == UDP_CHECK_QUERY[..2] && r[12] & 0x80 != 0 {
                return Some(true);
            }
        }
    }
    Some(false)
}

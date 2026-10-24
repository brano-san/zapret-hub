//! UDP probes: Discord voice IP discovery, STUN binding, DNS A queries.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Sends `packet`, waits until `timeout` for a datagram `accept` likes; the round trip in ms.
fn exchange(
    dst: SocketAddr,
    packet: &[u8],
    timeout: Duration,
    accept: impl Fn(&[u8]) -> bool,
) -> Option<(u32, Vec<u8>)> {
    let bind = if dst.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let sock = UdpSocket::bind(bind).ok()?;
    sock.send_to(packet, dst).ok()?;
    let start = Instant::now();
    let mut buf = [0u8; 1500];
    loop {
        let left = timeout
            .checked_sub(start.elapsed())?
            .max(Duration::from_millis(1));
        sock.set_read_timeout(Some(left)).ok()?;
        let (n, from) = sock.recv_from(&mut buf).ok()?;
        if from.ip() == dst.ip() && accept(&buf[..n]) {
            return Some((start.elapsed().as_millis() as u32, buf[..n].to_vec()));
        }
    }
}

/// Discord IP discovery: 74 bytes, type 1, length 70, the SSRC, zeros. Success is a type 2 reply of that SSRC.
pub fn ip_discovery(dst: SocketAddr, ssrc: u32, timeout: Duration) -> Option<u32> {
    let mut req = [0u8; 74];
    req[..2].copy_from_slice(&1u16.to_be_bytes());
    req[2..4].copy_from_slice(&70u16.to_be_bytes());
    req[4..8].copy_from_slice(&ssrc.to_be_bytes());
    exchange(dst, &req, timeout, |r| {
        r.len() >= 8 && r[..2] == [0, 2] && r[4..8] == ssrc.to_be_bytes()
    })
    .map(|(ms, _)| ms)
}

/// STUN Binding Request; success is a Binding Success with the same transaction id.
pub fn stun(dst: SocketAddr, timeout: Duration) -> Option<u32> {
    let mut req = [0u8; 20];
    req[..2].copy_from_slice(&1u16.to_be_bytes());
    req[4..8].copy_from_slice(&0x2112_A442u32.to_be_bytes());
    req[8..].copy_from_slice(&seed_bytes::<12>());
    let txn = req[8..].to_vec();
    exchange(dst, &req, timeout, |r| {
        r.len() >= 20 && r[..2] == [0x01, 0x01] && r[8..20] == txn[..]
    })
    .map(|(ms, _)| ms)
}

/// Bytes from the clock and a random hasher; unique enough for a transaction id.
fn seed_bytes<const N: usize>() -> [u8; N] {
    use std::hash::{BuildHasher, RandomState};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let state = RandomState::new();
    let mut out = [0u8; N];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let b = state.hash_one((nanos, i)).to_le_bytes();
        chunk.copy_from_slice(&b[..chunk.len()]);
    }
    out
}

/// A host name a query can carry as it is: letters, digits, `-`, labels of 1-63 bytes, 253 at
/// most (a trailing dot aside).
pub fn dns_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    (1..=253).contains(&name.len())
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// One A query over UDP. `None`: not a host name (`dns_name`), no answer, a failure (SERVFAIL,
/// REFUSED, ..) or a malformed reply; `Some(vec![])`: NXDOMAIN or no A record.
pub fn dns_udp(name: &str, server: SocketAddr, timeout: Duration) -> Option<Vec<Ipv4Addr>> {
    if !dns_name(name) {
        return None;
    }
    let id = seed_bytes::<2>();
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id);
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, one question
    for label in name.trim_end_matches('.').split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // root, A, IN
    let (_, reply) = exchange(server, &q, timeout, |r| {
        r.len() >= 12 && r[..2] == id && r[2] & 0x80 != 0
    })?;
    parse_a_records(&reply)
}

/// The A records of a DNS response; `None` when it is malformed or failed (an RCODE other than
/// NOERROR and NXDOMAIN).
fn parse_a_records(r: &[u8]) -> Option<Vec<Ipv4Addr>> {
    match r[3] & 0x0f {
        0 => {}
        3 => return Some(vec![]),
        _ => return None,
    }
    let qd = u16::from_be_bytes([r[4], r[5]]);
    let an = u16::from_be_bytes([r[6], r[7]]);
    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(r, pos)? + 4;
    }
    let mut out = vec![];
    for _ in 0..an {
        pos = skip_name(r, pos)?;
        let fixed = r.get(pos..pos + 10)?;
        let (ty, len) = (
            u16::from_be_bytes([fixed[0], fixed[1]]),
            usize::from(u16::from_be_bytes([fixed[8], fixed[9]])),
        );
        let data = r.get(pos + 10..pos + 10 + len)?;
        if ty == 1 && len == 4 {
            out.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        pos += 10 + len;
    }
    Some(out)
}

/// The offset after a name at `pos`: labels, ending in a zero byte or a compression pointer.
fn skip_name(r: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *r.get(pos)?;
        match len {
            0 => return Some(pos + 1),
            l if l & 0xC0 == 0xC0 => return Some(pos + 2),
            l => pos += 1 + usize::from(l),
        }
    }
}

/// A sustained IP discovery run: how many of the packets came back, and how fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Burst {
    pub sent: u32,
    pub received: u32,
    pub loss_pct: u32,
    pub median_ms: Option<u32>,
}

/// `sent` packets and the round trips of those answered.
pub fn burst_stats(sent: u32, mut rtts: Vec<u32>) -> Burst {
    rtts.sort_unstable();
    let received = (rtts.len() as u32).min(sent);
    Burst {
        sent,
        received,
        loss_pct: ((sent - received) * 100).checked_div(sent).unwrap_or(100),
        // The lower middle, as the diagnostics' median.
        median_ms: rtts.get(rtts.len().saturating_sub(1) / 2).copied(),
    }
}

/// `count` IP discovery packets spread over `over`, from one socket, each with its own SSRC so a
/// reply finds its packet. A flow cut after a few packets (DPI counting them) shows as loss,
/// which three single packets miss. Replies are awaited up to 1 s after the last packet.
pub fn ip_discovery_burst(dst: SocketAddr, count: u32, over: Duration) -> Burst {
    const BASE: u32 = 0x5A00_0000;
    let bind = if dst.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let Ok(sock) = UdpSocket::bind(bind) else {
        return burst_stats(0, vec![]);
    };
    let gap = over / count.max(1);
    let start = Instant::now();
    let mut sent_at: Vec<Instant> = Vec::new();
    let mut rtts: Vec<Option<u32>> = Vec::new();
    let mut buf = [0u8; 1500];
    for i in 0..=count {
        let deadline = if i < count {
            let mut req = [0u8; 74];
            req[..2].copy_from_slice(&1u16.to_be_bytes());
            req[2..4].copy_from_slice(&70u16.to_be_bytes());
            req[4..8].copy_from_slice(&(BASE + i).to_be_bytes());
            if sock.send_to(&req, dst).is_ok() {
                sent_at.push(Instant::now());
                rtts.push(None);
            }
            start + gap * (i + 1)
        } else {
            Instant::now() + Duration::from_secs(1)
        };
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if left.is_zero() || sock.set_read_timeout(Some(left)).is_err() {
                break;
            }
            let Ok((n, from)) = sock.recv_from(&mut buf) else {
                break;
            };
            let r = &buf[..n];
            if from.ip() != dst.ip() || r.len() < 8 || r[..2] != [0, 2] {
                continue;
            }
            let ssrc = u32::from_be_bytes([r[4], r[5], r[6], r[7]]);
            if let Some(slot) = ssrc
                .checked_sub(BASE)
                .and_then(|k| rtts.get_mut(k as usize))
                .filter(|s| s.is_none())
            {
                *slot = Some(sent_at[(ssrc - BASE) as usize].elapsed().as_millis() as u32);
            }
        }
    }
    burst_stats(sent_at.len() as u32, rtts.into_iter().flatten().collect())
}

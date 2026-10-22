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

/// One A query over UDP. `None`: no answer; `Some(vec![])`: NXDOMAIN or no A record.
pub fn dns_udp(name: &str, server: SocketAddr, timeout: Duration) -> Option<Vec<Ipv4Addr>> {
    let id = seed_bytes::<2>();
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id);
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, one question
    for label in name.trim_end_matches('.').split('.') {
        q.push(label.len().min(63) as u8);
        q.extend_from_slice(&label.as_bytes()[..label.len().min(63)]);
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // root, A, IN
    let (_, reply) = exchange(server, &q, timeout, |r| {
        r.len() >= 12 && r[..2] == id && r[2] & 0x80 != 0
    })?;
    Some(parse_a_records(&reply).unwrap_or_default())
}

/// The A records of a DNS response; `None` when it is malformed.
fn parse_a_records(r: &[u8]) -> Option<Vec<Ipv4Addr>> {
    if r[3] & 0x0f == 3 {
        return Some(vec![]);
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

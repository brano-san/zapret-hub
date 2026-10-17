//! DNS answers from the network (untrusted) and the IP-to-domain table built from them.

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// A known address stays at least this long, whatever the TTL says.
pub const MIN_TTL: Duration = Duration::from_secs(300);
/// A spoofed answer cannot pin an address to a name for longer (RFC 2181 8 also reads a TTL with
/// the top bit set as zero).
pub const MAX_TTL: u32 = 86_400;

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The asked name: lowercase, no trailing dot, at most 253 bytes.
    pub name: String,
    /// A records (with TTL) owned by the asked name or by a name of its CNAME chain.
    pub ips: Vec<(Ipv4Addr, u32)>,
}

/// Reads a (possibly compressed) name at `pos`; returns it and the offset after it in place.
/// Pointers must go strictly backward, so loops are impossible.
fn read_name(m: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut wire = 1; // the terminating zero
    let mut end = None;
    loop {
        let len = *m.get(pos)?;
        match len & 0xC0 {
            0 if len == 0 => return Some((name, end.unwrap_or(pos + 1))),
            0 => {
                let label = m.get(pos + 1..pos + 1 + len as usize)?;
                wire += label.len() + 1;
                if wire > 255 || !label.iter().all(|b| b.is_ascii_graphic() && *b != b'.') {
                    return None;
                }
                if !name.is_empty() {
                    name.push('.');
                }
                name.extend(label.iter().map(|b| b.to_ascii_lowercase() as char));
                pos += 1 + label.len();
            }
            0xC0 => {
                let target = (usize::from(len & 0x3F) << 8) | usize::from(*m.get(pos + 1)?);
                if target >= pos {
                    return None;
                }
                end.get_or_insert(pos + 2);
                pos = target;
            }
            _ => return None,
        }
    }
}

pub fn parse(m: &[u8]) -> Option<Answer> {
    let u16_at = |i: usize| {
        Some(usize::from(u16::from_be_bytes([
            *m.get(i)?,
            *m.get(i + 1)?,
        ])))
    };
    if m.len() < 12 || m[2] & 0x80 == 0 || m[3] & 0x0F != 0 || u16_at(4)? != 1 {
        return None;
    }
    let (name, mut pos) = read_name(m, 12)?;
    if name.is_empty() {
        return None;
    }
    pos += 4; // QTYPE, QCLASS
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut ips = Vec::new();
    for _ in 0..u16_at(6)? {
        let (owner, p) = read_name(m, pos)?;
        let (kind, class, rdlen) = (u16_at(p)?, u16_at(p + 2)?, u16_at(p + 8)?);
        let ttl = u32::from_be_bytes(m.get(p + 4..p + 8)?.try_into().ok()?);
        let ttl = if ttl > i32::MAX as u32 {
            0
        } else {
            ttl.min(MAX_TTL)
        };
        let data = p + 10;
        let rdata = m.get(data..data + rdlen)?;
        match (kind, class) {
            (5, 1) => edges.entry(owner).or_default().push(read_name(m, data)?.0),
            (1, 1) if rdlen == 4 => ips.push((
                owner,
                Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]),
                ttl,
            )),
            _ => {}
        }
        pos = data + rdlen;
    }
    let mut chain = HashSet::from([name.clone()]);
    let mut todo = vec![name.clone()];
    while let Some(n) = todo.pop() {
        for next in edges.get(&n).into_iter().flatten() {
            if chain.insert(next.clone()) {
                todo.push(next.clone());
            }
        }
    }
    let ips = ips
        .into_iter()
        .filter(|(o, ..)| chain.contains(o))
        .map(|(_, ip, ttl)| (ip, ttl))
        .collect();
    Some(Answer { name, ips })
}

#[derive(Default)]
pub struct Names(HashMap<Ipv4Addr, (String, Instant)>);

impl Names {
    /// True when an address is new or changed its name (the capture filter needs a rebuild).
    pub fn record(&mut self, answer: &Answer, now: Instant) -> bool {
        let mut changed = false;
        for &(ip, ttl) in &answer.ips {
            let until = now + MIN_TTL.max(Duration::from_secs(u64::from(ttl)));
            match self.0.get_mut(&ip) {
                Some((name, old)) => {
                    *old = (*old).max(until);
                    if *name != answer.name {
                        name.clone_from(&answer.name);
                        changed = true;
                    }
                }
                None => {
                    self.0.insert(ip, (answer.name.clone(), until));
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn name_of(&self, ip: Ipv4Addr, now: Instant) -> Option<&str> {
        self.0
            .get(&ip)
            .filter(|(_, until)| *until > now)
            .map(|(n, _)| n.as_str())
    }

    /// Addresses in `live` (destinations of live flows) are kept: their name lives another
    /// `MIN_TTL` from now, so the capture filter keeps them while they are in use.
    pub fn expire(&mut self, now: Instant, live: &[Ipv4Addr]) -> bool {
        let before = self.0.len();
        self.0.retain(|ip, (_, until)| {
            if live.contains(ip) {
                *until = (*until).max(now + MIN_TTL);
            }
            *until > now
        });
        self.0.len() != before
    }

    pub fn entries(&self) -> Vec<(Ipv4Addr, String)> {
        let mut all: Vec<_> = self.0.iter().map(|(ip, (n, _))| (*ip, n.clone())).collect();
        all.sort();
        all
    }
}

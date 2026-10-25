//! WinDivert: the capture filter, DLL choice and the address struct (pure part).

use crate::rules::{Protocol, Table};
use anyhow::{Result, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

pub const MAX_FILTER_RANGES: usize = 64;
pub const SOCKET_FILTER: &str =
    "!loopback and (udp or tcp) and (event == BIND or event == CONNECT or event == CLOSE)";
/// IPv4 and IPv6: `udp` holds for both.
pub const DNS_FILTER: &str = "udp and udp.SrcPort == 53";
pub const EVENT_SOCKET_CLOSE: u8 = 7;
pub const PROTO_TCP: u8 = 6;

fn port(proto: &str, &(a, b): &(u16, u16)) -> String {
    match (a, b) {
        _ if a == b => format!("{proto}.DstPort == {a}"),
        (_, 65535) => format!("{proto}.DstPort >= {a}"),
        _ => format!("({proto}.DstPort >= {a} and {proto}.DstPort <= {b})"),
    }
}

fn range(&(lo, hi): &(u32, u32)) -> String {
    let ip = |x: u32| Ipv4Addr::from(x);
    if lo == hi {
        format!("ip.DstAddr == {}", ip(lo))
    } else {
        format!("(ip.DstAddr >= {} and ip.DstAddr <= {})", ip(lo), ip(hi))
    }
}

fn range6(&(lo, hi): &(u128, u128)) -> String {
    let ip = |x: u128| Ipv6Addr::from(x);
    if lo == hi {
        format!("ipv6.DstAddr == {}", ip(lo))
    } else {
        format!(
            "(ipv6.DstAddr >= {} and ipv6.DstAddr <= {})",
            ip(lo),
            ip(hi)
        )
    }
}

fn any(items: impl Iterator<Item = String>) -> String {
    format!("({})", items.collect::<Vec<_>>().join(" or "))
}

/// `head` alone, or `head and (<ports in proto's field>)`.
fn on_ports(head: &str, proto: &str, ports: &[(u16, u16)]) -> String {
    if ports.is_empty() {
        head.to_string()
    } else {
        format!("{head} and {}", any(ports.iter().map(|p| port(proto, p))))
    }
}

/// WinDivert compiles a filter into at most 256 tests (`WINDIVERT_FILTER_MAXLEN`); this keeps
/// a margin, since `capture_filter` counts the tests itself.
pub const FILTER_BUDGET: usize = 200;
/// `outbound`, `!loopback`.
const BASE_TESTS: usize = 2;
/// `tcp and tcp.SrcPort == N`.
const ACCEPTOR_TESTS: usize = 2;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Udp,
    Tcp,
    Both,
    /// The first SYN of a connection (domain TCP rules).
    Syn,
    /// Destinations of live NAT entries.
    Held,
}

/// One `or` alternative: protocol and ports, ANDed with its IP ranges when it has any.
struct Part {
    kind: Kind,
    /// Sorted.
    ports: Vec<(u16, u16)>,
    /// Sorted, merged.
    ranges: Vec<(u32, u32)>,
    /// IPv6 likewise.
    ranges6: Vec<(u128, u128)>,
}

impl Part {
    fn tcp(&self) -> bool {
        self.kind != Kind::Udp
    }

    fn tests(&self) -> usize {
        let ports: usize = self
            .ports
            .iter()
            .map(|&(a, b)| if a == b || b == 65535 { 1 } else { 2 })
            .sum();
        let ranges: usize = self
            .ranges
            .iter()
            .map(|&(a, b)| if a == b { 1 } else { 2 })
            .chain(
                self.ranges6
                    .iter()
                    .map(|&(a, b)| if a == b { 1 } else { 2 }),
            )
            .sum();
        ranges
            + match self.kind {
                Kind::Udp | Kind::Tcp | Kind::Held => 1 + ports,
                Kind::Both => 2 + 2 * ports,
                Kind::Syn => 3 + ports,
            }
    }

    fn text(&self) -> String {
        let head = match self.kind {
            Kind::Udp => on_ports("udp", "udp", &self.ports),
            Kind::Tcp => on_ports("tcp", "tcp", &self.ports),
            Kind::Both if self.ports.is_empty() => "(udp or tcp)".into(),
            Kind::Both => format!(
                "(({}) or ({}))",
                on_ports("udp", "udp", &self.ports),
                on_ports("tcp", "tcp", &self.ports)
            ),
            Kind::Syn => on_ports("tcp and tcp.Syn and !tcp.Ack", "tcp", &self.ports),
            Kind::Held => "tcp".into(),
        };
        if self.ranges.is_empty() && self.ranges6.is_empty() {
            format!("({head})")
        } else {
            let ranges = self.ranges.iter().map(range);
            format!(
                "({head} and {})",
                any(ranges.chain(self.ranges6.iter().map(range6)))
            )
        }
    }
}

/// Inside one list, merges the two neighbouring items with the smallest gap (ties: the first).
/// False when no list has two items.
pub(crate) fn merge_closest<T: Copy + Ord + Into<u128>>(lists: Vec<&mut Vec<(T, T)>>) -> bool {
    let closest = lists
        .iter()
        .enumerate()
        .flat_map(|(l, list)| {
            list.windows(2)
                .enumerate()
                .map(move |(i, w)| (w[1].0.into().saturating_sub(w[0].1.into()), l, i))
        })
        .min_by_key(|&(gap, _, _)| gap);
    let Some((_, l, i)) = closest else {
        return false;
    };
    let list = lists.into_iter().nth(l).unwrap();
    list[i].1 = list[i].1.max(list[i + 1].1);
    list.remove(i + 1);
    true
}

/// Merges the closest IPv6 ranges, then IPv4 ones (IPv4 filters stay as exact as before IPv6),
/// then the closest ports, until at most `MAX_FILTER_RANGES`
/// ranges are left and the filter has at most `FILTER_BUDGET` tests, or nothing is left to merge.
// ponytail: O(n²) in the number of ranges and ports (hundreds at most); a heap of gaps if that
// ever grows. The filter becomes a superset; `Table` stays exact and the hub re-sends what it does
// not match.
fn fit(parts: &mut [Part], extra: usize) {
    loop {
        let ranges: usize = parts.iter().map(|p| p.ranges.len() + p.ranges6.len()).sum();
        let tests: usize = extra + parts.iter().map(Part::tests).sum::<usize>();
        if ranges <= MAX_FILTER_RANGES && tests <= FILTER_BUDGET {
            return;
        }
        let merged = merge_closest(parts.iter_mut().map(|p| &mut p.ranges6).collect())
            || merge_closest(parts.iter_mut().map(|p| &mut p.ranges).collect())
            || (ranges <= MAX_FILTER_RANGES
                && merge_closest(parts.iter_mut().map(|p| &mut p.ports).collect()));
        if !merged {
            return;
        }
    }
}

/// The router's reason for a capture filter WinDivert refused. The filter is generated, so an
/// invalid one (`ERROR_INVALID_PARAMETER`) means it did not fit WinDivert's limit.
pub fn refused(e: &std::io::Error) -> String {
    if e.raw_os_error() == Some(87) {
        format!("too many rules for the capture filter: disable or narrow some (WinDivert: {e})")
    } else {
        format!("WinDivert refused the filter: {e}")
    }
}

/// Only error 87 (the filter did not fit) is final for that filter text; memory or driver
/// errors may pass on the next rebuild.
pub fn refused_for_good(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(87)
}

pub fn filter(table: &Table) -> Option<String> {
    capture_filter(table, None, &[])
}

/// The capture handle's filter: each rule's protocol and ports over its ranges (subnets and
/// resolved IPs; none yet, no part), the first SYNs of domain and program TCP rules (so the hub
/// sees a SYN to an address learned since the last reopen, or decides a program's connection), the
/// UDP of program rules while their program runs (`Compiled.running`), destinations of live NAT entries (`held`) and the
/// local acceptor's replies. IPv4 and IPv6: `tcp` and `udp` hold for both.
pub fn capture_filter(table: &Table, acceptor: Option<u16>, held: &[IpAddr]) -> Option<String> {
    let mut parts: Vec<Part> = Vec::new();
    for r in &table.rules {
        let mut ports = r.ports.clone();
        ports.sort_unstable();
        if !r.ranges.is_empty() || !r.ranges6.is_empty() {
            let kind = match r.protocol {
                Protocol::Udp => Kind::Udp,
                Protocol::Tcp => Kind::Tcp,
                Protocol::Both => Kind::Both,
            };
            parts.push(Part {
                kind,
                ports: ports.clone(),
                ranges: r.ranges.clone(),
                ranges6: r.ranges6.clone(),
            });
        }
        let syn = r.protocol != Protocol::Udp && !(r.domains.is_empty() && r.apps.is_empty());
        // `running` is set only for rules that take UDP and have programs.
        let udp = r.protocol != Protocol::Tcp && !r.apps.is_empty() && r.running;
        for (wanted, kind) in [(syn, Kind::Syn), (udp, Kind::Udp)] {
            let same = |p: &Part| {
                p.kind == kind && p.ports == ports && p.ranges.is_empty() && p.ranges6.is_empty()
            };
            if wanted && !parts.iter().any(same) {
                parts.push(Part {
                    kind,
                    ports: ports.clone(),
                    ranges: vec![],
                    ranges6: vec![],
                });
            }
        }
    }
    if !held.is_empty() {
        let (mut v4, mut v6) = (vec![], vec![]);
        for ip in held {
            match ip {
                IpAddr::V4(ip) => v4.push((u32::from(*ip), u32::from(*ip))),
                IpAddr::V6(ip) => v6.push((u128::from(*ip), u128::from(*ip))),
            }
        }
        parts.push(Part {
            kind: Kind::Held,
            ports: vec![],
            ranges: crate::rules::merge(v4),
            ranges6: crate::rules::merge(v6),
        });
    }
    if parts.is_empty() {
        return None;
    }
    let acceptor = acceptor.filter(|_| parts.iter().any(Part::tcp));
    let extra = BASE_TESTS + acceptor.map_or(0, |_| ACCEPTOR_TESTS);
    fit(&mut parts, extra);
    let mut text: Vec<String> = parts.iter().map(Part::text).collect();
    if let Some(port) = acceptor {
        text.push(format!("(tcp and tcp.SrcPort == {port})"));
    }
    Some(format!(
        "outbound and !loopback and ({})",
        text.join(" or ")
    ))
}

/// The WinDivert DLL of zapret2, next to `winws2.exe` in `winws2_dir`: the driver it loads must
/// be the one the router talks to.
pub fn pick_dll(winws2_dir: Option<&Path>, exists: impl Fn(&Path) -> bool) -> Result<PathBuf> {
    let Some(dir) = winws2_dir else {
        bail!("the zapret2 folder is not set: WinDivert.dll comes from it, choose it in Settings");
    };
    let dll = dir.join("WinDivert.dll");
    if !exists(&dll) {
        bail!(
            "{} is missing: the router needs zapret2's WinDivert.dll",
            dll.display()
        );
    }
    Ok(dll)
}

/// `WINDIVERT_ADDRESS` of WinDivert 2.2.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct Address(pub [u8; 80]);

impl Address {
    pub fn outbound(&self) -> bool {
        self.0[10] & 0x02 != 0
    }

    /// Clears Outbound and Impostor; the rest stays.
    pub fn as_inbound(&self) -> Address {
        let mut a = *self;
        a.0[10] &= !0x0A;
        a
    }

    /// The packet or socket is IPv6.
    pub fn ipv6(&self) -> bool {
        self.0[10] & 0x10 != 0
    }

    pub fn event(&self) -> u8 {
        self.0[9]
    }

    pub fn socket_pid(&self) -> u32 {
        u32::from_le_bytes(self.0[32..36].try_into().unwrap())
    }

    pub fn socket_local_port(&self) -> u16 {
        u16::from_le_bytes(self.0[68..70].try_into().unwrap())
    }

    pub fn socket_protocol(&self) -> u8 {
        self.0[72]
    }
}

// ---- the DLL and handles (the live part) ----

#[cfg(windows)]
mod dll;
#[cfg(windows)]
pub use dll::*;

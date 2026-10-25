//! Flows as Linux conntrack sees them: ctnetlink messages (`parse`) and the counters they give
//! (`Conns`). Pure, so it builds and is tested everywhere; the sockets are in `observe::netlink`.
//!
//! What conntrack tells: the tuple, whether the other side ever answered (`IPS_SEEN_REPLY`), the
//! timeout (reset by each packet) and, with `nf_conntrack_acct` on, per-direction counters. Without
//! counters, the packets sent before the first answer are counted from the timeout resets seen
//! between two looks (at most one per look, so a lower bound), and an answered flow has one packet
//! back. That gives `SynUnanswered` and `UdpNoReply`; resets, retransmits, RTT and names from the
//! packets (`sni`) need the packets themselves and stay unset.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use super::FlowCounters;
use crate::rules::Protocol;

const NLMSG_HDRLEN: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NFNL_SUBSYS_CTNETLINK: u16 = 1;
const IPCTNL_MSG_CT_GET: u16 = 1;
const IPCTNL_MSG_CT_DELETE: u16 = 2;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
/// `NLA_F_NESTED | NLA_F_NET_BYTEORDER` off.
const NLA_TYPE_MASK: u16 = 0x3FFF;

const CTA_TUPLE_ORIG: u16 = 1;
const CTA_STATUS: u16 = 3;
const CTA_TIMEOUT: u16 = 7;
const CTA_COUNTERS_ORIG: u16 = 9;
const CTA_COUNTERS_REPLY: u16 = 10;
const CTA_ID: u16 = 12;
const CTA_TUPLE_IP: u16 = 1;
const CTA_TUPLE_PROTO: u16 = 2;
const CTA_PROTO_NUM: u16 = 1;
const CTA_PROTO_SRC_PORT: u16 = 2;
const CTA_PROTO_DST_PORT: u16 = 3;
const CTA_COUNTERS_PACKETS: u16 = 1;
const CTA_COUNTERS_BYTES: u16 = 2;
const IPS_SEEN_REPLY: u32 = 1 << 1;

/// The conntrack multicast groups `NEW`, `UPDATE` and `DESTROY` as a bind mask.
pub const EVENT_GROUPS: u32 = 0b111;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Count {
    pub packets: u64,
    pub bytes: u64,
}

/// One conntrack entry of a TCP or UDP flow, oriented as its first packet went (`src` sent it).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u32,
    pub protocol: Protocol,
    pub src: SocketAddr,
    pub dst: SocketAddr,
    pub status: u32,
    /// Seconds left; absent in a `DESTROY` event.
    pub timeout: Option<u32>,
    /// Original and reply direction, with `nf_conntrack_acct` on.
    pub counters: Option<(Count, Count)>,
}

#[derive(Debug, PartialEq)]
pub enum Msg {
    Entry(Entry),
    Done,
    /// `-errno`; 0 is an acknowledgement.
    Error(i32),
}

fn ne16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn ne32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes(b.try_into().ok()?))
}

fn be32(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.try_into().ok()?))
}

fn be64(b: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(b.try_into().ok()?))
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// The netlink attributes in `b`: (type, payload).
fn attrs(b: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let len = usize::from(ne16(b, at)?);
        let kind = ne16(b, at + 2)? & NLA_TYPE_MASK;
        let payload = b.get(at + 4..at + len.max(4))?;
        at += align4(len.max(4));
        Some((kind, payload))
    })
}

fn find(b: &[u8], kind: u16) -> Option<&[u8]> {
    attrs(b).find(|(k, _)| *k == kind).map(|(_, p)| p)
}

fn ip(b: &[u8]) -> Option<IpAddr> {
    match b.len() {
        4 => Some(Ipv4Addr::from(<[u8; 4]>::try_from(b).ok()?).into()),
        16 => Some(Ipv6Addr::from(<[u8; 16]>::try_from(b).ok()?).into()),
        _ => None,
    }
}

fn tuple(b: &[u8]) -> Option<(Protocol, SocketAddr, SocketAddr)> {
    // `CTA_IP_V4_SRC`/`V4_DST` (1, 2) or `V6_SRC`/`V6_DST` (3, 4).
    let ips: Vec<(u16, IpAddr)> = attrs(find(b, CTA_TUPLE_IP)?)
        .filter_map(|(k, p)| Some((k, ip(p)?)))
        .collect();
    let src = ips.iter().find(|(k, _)| k % 2 == 1)?.1;
    let dst = ips.iter().find(|(k, _)| k % 2 == 0)?.1;
    let proto = find(b, CTA_TUPLE_PROTO)?;
    let protocol = match find(proto, CTA_PROTO_NUM)?.first()? {
        6 => Protocol::Tcp,
        17 => Protocol::Udp,
        _ => return None,
    };
    let port = |kind| be16(find(proto, kind)?);
    Some((
        protocol,
        SocketAddr::new(src, port(CTA_PROTO_SRC_PORT)?),
        SocketAddr::new(dst, port(CTA_PROTO_DST_PORT)?),
    ))
}

fn count(b: &[u8]) -> Option<Count> {
    Some(Count {
        packets: be64(find(b, CTA_COUNTERS_PACKETS)?)?,
        bytes: be64(find(b, CTA_COUNTERS_BYTES)?)?,
    })
}

/// A ctnetlink message's body after `nfgenmsg`; `None` for what is not a TCP/UDP flow.
fn entry(b: &[u8]) -> Option<Entry> {
    let (protocol, src, dst) = tuple(find(b, CTA_TUPLE_ORIG)?)?;
    let counters = match (find(b, CTA_COUNTERS_ORIG), find(b, CTA_COUNTERS_REPLY)) {
        (Some(o), Some(r)) => Some((count(o)?, count(r)?)),
        _ => None,
    };
    Some(Entry {
        id: find(b, CTA_ID).and_then(be32).unwrap_or(0),
        protocol,
        src,
        dst,
        status: find(b, CTA_STATUS).and_then(be32).unwrap_or(0),
        timeout: find(b, CTA_TIMEOUT).and_then(be32),
        counters,
    })
}

/// The messages of one netlink datagram: a dump's part or events.
pub fn parse(buf: &[u8]) -> Vec<Msg> {
    parse_where(buf, |_| true)
}

/// `parse` of the messages answering the request `seq`: what is left of an earlier dump (one
/// whose reads timed out) is skipped.
pub fn parse_reply(buf: &[u8], seq: u32) -> Vec<Msg> {
    parse_where(buf, |s| s == seq)
}

fn parse_where(buf: &[u8], keep: impl Fn(u32) -> bool) -> Vec<Msg> {
    let mut out = Vec::new();
    let mut at = 0;
    while let (Some(len), Some(kind)) = (ne32(buf, at), ne16(buf, at + 4)) {
        let len = len as usize;
        let Some(body) = buf.get(at + NLMSG_HDRLEN..at + len) else {
            break;
        };
        if !keep(ne32(buf, at + 8).unwrap_or(0)) {
            at += align4(len.max(NLMSG_HDRLEN));
            continue;
        }
        match kind {
            NLMSG_DONE => out.push(Msg::Done),
            NLMSG_ERROR => out.push(Msg::Error(ne32(body, 0).unwrap_or(0) as i32)),
            k if k >> 8 == NFNL_SUBSYS_CTNETLINK && k & 0xFF != IPCTNL_MSG_CT_DELETE => {
                // `nfgenmsg`: family, version, resource id.
                if let Some(e) = body.get(4..).and_then(entry) {
                    out.push(Msg::Entry(e));
                }
            }
            // A `DESTROY` event: nothing more to learn than the last look gave.
            _ => {}
        }
        at += align4(len.max(NLMSG_HDRLEN));
    }
    out
}

/// A request for the whole table, both families.
pub fn dump_request(seq: u32) -> Vec<u8> {
    let mut m = Vec::with_capacity(20);
    m.extend(20u32.to_ne_bytes());
    m.extend((NFNL_SUBSYS_CTNETLINK << 8 | IPCTNL_MSG_CT_GET).to_ne_bytes());
    m.extend((NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    m.extend(seq.to_ne_bytes());
    m.extend(0u32.to_ne_bytes());
    // `nfgenmsg`: AF_UNSPEC, NFNETLINK_V0, resource id 0.
    m.extend([0, 0, 0, 0]);
    m
}

type Key = (Protocol, u16, SocketAddr);

#[derive(Default)]
struct Conn {
    c: FlowCounters,
    id: u32,
    /// (when, seconds left) at the last look at entry `id`.
    seen: Option<(Instant, u32)>,
    replied: bool,
    /// Looks before the first answer that found a packet since the previous one.
    out_unreplied: u32,
    first: Option<Instant>,
    /// From the first look to the last one without an answer.
    unreplied_for: Duration,
    /// Counters of the earlier entries of this key, and of entry `id`.
    done: (Count, Count),
    now: Option<(Count, Count)>,
}

/// Counters per (protocol, local port, remote address) as `Tally`'s, from conntrack entries. The
/// side that sent the first packet is the local one. Loopback flows are skipped, except those to
/// the system proxy (`proxy_port`): marked, without counters.
pub struct Conns {
    flows: HashMap<Key, Conn>,
    proxy_port: Option<u16>,
}

fn add(a: Count, b: Count) -> Count {
    Count {
        packets: a.packets + b.packets,
        bytes: a.bytes + b.bytes,
    }
}

/// SYNs Linux has sent after `d` unanswered, at the slowest (exponential backoff from 1 s; the
/// linear start of `tcp_syn_linear_timeouts` sends more).
fn syns_within(d: Duration) -> u32 {
    1 + [1, 3, 7, 15, 31, 63]
        .iter()
        .filter(|s| d >= Duration::from_secs(**s))
        .count() as u32
}

fn sat(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Conns {
    pub fn new(proxy_port: Option<u16>) -> Conns {
        Conns {
            flows: HashMap::new(),
            proxy_port,
        }
    }

    /// One look at an entry, from a dump or an event.
    pub fn update(&mut self, e: &Entry, now: Instant) {
        let (local, remote) = (e.src, e.dst);
        let proxied = remote.ip().is_loopback() && Some(remote.port()) == self.proxy_port;
        if remote.ip().is_loopback() && !proxied {
            return;
        }
        let conn = self
            .flows
            .entry((e.protocol, local.port(), remote))
            .or_insert_with(|| Conn {
                c: FlowCounters {
                    protocol: e.protocol,
                    local_port: local.port(),
                    dst: remote.to_string(),
                    ipv6: remote.is_ipv6(),
                    via_system_proxy: proxied,
                    ..FlowCounters::default()
                },
                ..Conn::default()
            });
        if proxied {
            return;
        }
        if conn.id != e.id {
            if let Some((o, r)) = conn.now.take() {
                conn.done = (add(conn.done.0, o), add(conn.done.1, r));
            }
            conn.id = e.id;
            conn.seen = None;
        }
        let replied = e.status & IPS_SEEN_REPLY != 0;
        if let Some(left) = e.timeout {
            // Without a packet the time left falls with the clock; whole seconds, so a reset
            // shows only once it moved the end by more than the rounding.
            let packet = conn.seen.is_none_or(|(at, before)| {
                f64::from(left) + now.duration_since(at).as_secs_f64() >= f64::from(before) + 1.0
            });
            if packet && !replied {
                conn.out_unreplied += 1;
            }
            conn.seen = Some((now, left));
        }
        conn.replied |= replied;
        let first = *conn.first.get_or_insert(now);
        if !conn.replied {
            conn.unreplied_for = now.duration_since(first);
        }
        if e.counters.is_some() {
            conn.now = e.counters;
        }
    }

    /// Sorted by destination, then local port.
    pub fn rows(&self) -> Vec<FlowCounters> {
        let mut rows: Vec<FlowCounters> = self.flows.values().map(Conn::row).collect();
        rows.sort_by(|a, b| (&a.dst, a.local_port).cmp(&(&b.dst, b.local_port)));
        rows
    }
}

impl Conn {
    fn row(&self) -> FlowCounters {
        let mut c = self.c.clone();
        if c.via_system_proxy {
            return c;
        }
        match self.now {
            Some((o, r)) => {
                let (o, r) = (add(self.done.0, o), add(self.done.1, r));
                c.up_packets = sat(o.packets);
                c.up_bytes = o.bytes as f64;
                c.down_packets = sat(r.packets);
                c.down_bytes = r.bytes as f64;
            }
            None => {
                c.up_packets = self.out_unreplied;
                c.down_packets = u32::from(self.replied);
            }
        }
        if c.protocol == Protocol::Tcp {
            c.syn_answered = self.replied;
            // Before an answer every packet out is a SYN. Conntrack refreshes (and counts) no
            // resent SYN: their number comes from the time unanswered too.
            let sent = match self.replied {
                false => c.up_packets,
                true => self.out_unreplied,
            };
            c.syns = sent.max(syns_within(self.unreplied_for));
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend((4 + payload.len() as u16).to_ne_bytes());
        a.extend(kind.to_ne_bytes());
        a.extend(payload);
        a.resize(align4(a.len()), 0);
        a
    }

    fn nested(kind: u16, parts: &[Vec<u8>]) -> Vec<u8> {
        attr(kind | 0x8000, &parts.concat())
    }

    fn msg(kind: u16, body: &[u8]) -> Vec<u8> {
        let mut m = Vec::new();
        m.extend((16 + body.len() as u32).to_ne_bytes());
        m.extend(kind.to_ne_bytes());
        m.extend(2u16.to_ne_bytes()); // NLM_F_MULTI
        m.extend(7u32.to_ne_bytes());
        m.extend(0u32.to_ne_bytes());
        m.extend(body);
        m
    }

    /// A ctnetlink entry as the kernel sends it: IPv4 (`v6` false) or IPv6.
    #[allow(clippy::too_many_arguments)]
    fn ct(
        kind: u16,
        proto: u8,
        src: IpAddr,
        sport: u16,
        dst: IpAddr,
        dport: u16,
        status: u32,
        counters: Option<(u64, u64, u64, u64)>,
    ) -> Vec<u8> {
        let ip = |a: IpAddr| match a {
            IpAddr::V4(a) => a.octets().to_vec(),
            IpAddr::V6(a) => a.octets().to_vec(),
        };
        let (s, d) = if src.is_ipv4() { (1, 2) } else { (3, 4) };
        let tuple = |kind, src, sport: u16, dst, dport: u16| {
            nested(
                kind,
                &[
                    nested(CTA_TUPLE_IP, &[attr(s, &ip(src)), attr(d, &ip(dst))]),
                    nested(
                        CTA_TUPLE_PROTO,
                        &[
                            attr(CTA_PROTO_NUM, &[proto]),
                            attr(CTA_PROTO_SRC_PORT, &sport.to_be_bytes()),
                            attr(CTA_PROTO_DST_PORT, &dport.to_be_bytes()),
                        ],
                    ),
                ],
            )
        };
        let mut body = vec![2, 0, 0, 0];
        body.extend(tuple(CTA_TUPLE_ORIG, src, sport, dst, dport));
        body.extend(tuple(2, dst, dport, src, sport));
        body.extend(attr(CTA_STATUS, &status.to_be_bytes()));
        body.extend(attr(CTA_TIMEOUT, &30u32.to_be_bytes()));
        if let Some((op, ob, rp, rb)) = counters {
            let c = |p: u64, b: u64| {
                [
                    attr(CTA_COUNTERS_PACKETS, &p.to_be_bytes()),
                    attr(CTA_COUNTERS_BYTES, &b.to_be_bytes()),
                ]
            };
            body.extend(nested(CTA_COUNTERS_ORIG, &c(op, ob)));
            body.extend(nested(CTA_COUNTERS_REPLY, &c(rp, rb)));
        }
        body.extend(attr(CTA_ID, &0xABCDu32.to_be_bytes()));
        msg(kind, &body)
    }

    const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5));
    const REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(162, 159, 128, 1));
    const NEW: u16 = 0x100;

    #[test]
    fn a_dump_part_gives_its_entries_and_ends_with_done() {
        let mut buf = ct(NEW, 6, LOCAL, 50000, REMOTE, 443, 0xE, None);
        buf.extend(ct(
            NEW,
            17,
            LOCAL,
            50001,
            REMOTE,
            50007,
            0x8,
            Some((5, 500, 2, 200)),
        ));
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        buf.extend(ct(NEW, 17, v6, 5353, v6, 53, 0x8, None));
        buf.extend(ct(NEW, 1, LOCAL, 0, REMOTE, 0, 0x8, None)); // ICMP: skipped
        buf.extend(ct(0x102, 6, LOCAL, 50002, REMOTE, 443, 0xE, None)); // DESTROY: skipped
        buf.extend(msg(NLMSG_DONE, &0u32.to_ne_bytes()));
        let msgs = parse(&buf);
        assert_eq!(msgs.len(), 4, "{msgs:?}");
        let Msg::Entry(tcp) = &msgs[0] else {
            panic!("{msgs:?}")
        };
        assert_eq!(tcp.protocol, Protocol::Tcp);
        assert_eq!(tcp.src, SocketAddr::new(LOCAL, 50000));
        assert_eq!(tcp.dst, SocketAddr::new(REMOTE, 443));
        assert_eq!(tcp.status & IPS_SEEN_REPLY, IPS_SEEN_REPLY);
        assert_eq!(tcp.timeout, Some(30));
        assert_eq!(tcp.id, 0xABCD);
        assert_eq!(tcp.counters, None);
        let Msg::Entry(udp) = &msgs[1] else {
            panic!("{msgs:?}")
        };
        assert_eq!(udp.protocol, Protocol::Udp);
        assert_eq!(
            udp.counters,
            Some((
                Count {
                    packets: 5,
                    bytes: 500
                },
                Count {
                    packets: 2,
                    bytes: 200
                }
            ))
        );
        let Msg::Entry(six) = &msgs[2] else {
            panic!("{msgs:?}")
        };
        assert_eq!(six.src, "[2001:db8::1]:5353".parse().unwrap());
        assert_eq!(msgs[3], Msg::Done);
        // Cut anywhere, nothing panics and nothing past the cut is read.
        for n in 0..buf.len() {
            assert!(parse(&buf[..n]).len() <= 4);
        }
        assert_eq!(
            parse(&msg(NLMSG_ERROR, &(-1i32).to_ne_bytes())),
            [Msg::Error(-1)]
        );
    }

    #[test]
    fn the_dump_request_asks_ctnetlink_for_everything() {
        let r = dump_request(9);
        assert_eq!(r.len(), 20);
        assert_eq!(ne32(&r, 0), Some(20));
        assert_eq!(ne16(&r, 4), Some(0x101));
        assert_eq!(ne16(&r, 6), Some(0x301));
        assert_eq!(ne32(&r, 8), Some(9));
    }

    fn entry(protocol: Protocol, status: u32, left: u32) -> Entry {
        Entry {
            id: 1,
            protocol,
            src: SocketAddr::new(LOCAL, 50000),
            dst: SocketAddr::new(REMOTE, 443),
            status,
            timeout: Some(left),
            counters: None,
        }
    }

    #[test]
    fn unanswered_syns_count_from_the_time_and_udp_from_the_timeout_resets() {
        let t = Instant::now();
        let s = |n: u64| t + Duration::from_millis(n);
        let mut conns = Conns::new(None);
        // Resent SYNs leave the timeout (120) as it was: looks each second, 3 s unanswered.
        for (at, left) in [(500, 119), (1500, 118), (2500, 117), (3500, 116)] {
            conns.update(&entry(Protocol::Tcp, 0x8, left), s(at));
        }
        let mut udp = entry(Protocol::Udp, 0x8, 0);
        udp.src.set_port(50001);
        // A packet each second, then silence.
        for (at, left) in [
            (100, 29),
            (1100, 29),
            (2100, 29),
            (3100, 29),
            (4100, 28),
            (5100, 27),
        ] {
            udp.timeout = Some(left);
            conns.update(&udp, s(at));
        }
        let rows = conns.rows();
        let (tcp, udp) = (&rows[0], &rows[1]);
        assert_eq!((tcp.syns, tcp.syn_answered), (3, false));
        assert_eq!((udp.up_packets, udp.down_packets), (4, 0));

        // Answered: the SYN counts, the reply is one packet back.
        let mut conns = Conns::new(None);
        conns.update(&entry(Protocol::Tcp, 0x8, 120), s(0));
        conns.update(&entry(Protocol::Tcp, 0xE, 431_999), s(1000));
        let row = &conns.rows()[0];
        assert_eq!((row.syns, row.syn_answered, row.down_packets), (1, true, 1));
    }

    #[test]
    fn counters_win_and_add_up_over_entries_of_one_flow() {
        let t = Instant::now();
        let mut conns = Conns::new(None);
        let mut e = entry(Protocol::Udp, 0x8, 30);
        e.counters = Some((
            Count {
                packets: 6,
                bytes: 600,
            },
            Count::default(),
        ));
        conns.update(&e, t);
        e.id = 2;
        e.counters = Some((
            Count {
                packets: 2,
                bytes: 100,
            },
            Count {
                packets: 1,
                bytes: 50,
            },
        ));
        conns.update(&e, t);
        let row = &conns.rows()[0];
        assert_eq!((row.up_packets, row.down_packets), (8, 1));
        assert_eq!((row.up_bytes, row.down_bytes), (700.0, 50.0));
    }

    #[test]
    fn loopback_is_skipped_but_the_system_proxy_is_marked() {
        let mut conns = Conns::new(Some(1080));
        let mut e = entry(Protocol::Tcp, 0x8, 120);
        e.dst = "127.0.0.1:8080".parse().unwrap();
        conns.update(&e, Instant::now());
        e.dst = "127.0.0.1:1080".parse().unwrap();
        conns.update(&e, Instant::now());
        let rows = conns.rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].via_system_proxy);
        assert_eq!(rows[0].syns, 0);
    }

    #[test]
    fn a_dump_reads_only_the_replies_to_its_request() {
        let old = msg(NLMSG_DONE, &[0; 4]); // seq 7: what a timed-out dump left
        let mut new = old.clone();
        new[8..12].copy_from_slice(&8u32.to_ne_bytes());
        let both = [old, new].concat();
        assert!(matches!(parse_reply(&both, 8)[..], [Msg::Done]));
        assert!(parse_reply(&both[..20], 8).is_empty());
        assert_eq!(parse(&both).len(), 2);
    }
}

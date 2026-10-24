//! Passive per-program flow observation for diagnostics: counters per flow (`Tally`, pure) and the
//! WinDivert sniffers that feed it (`Observer`). Nothing is held or changed.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::divert::{
    self, DNS_FILTER, Dll, FLAG_RECV_ONLY, FLAG_SNIFF, Handle, Layer, MAX_FILTER_RANGES, MAX_PACKET,
};
use crate::dns::{self, Names};
use crate::flows::{FlowRow, Via};
use crate::owner::Images;
use crate::packet::{ACK, RST, SYN, Tcp, parse_tcp, parse_udp};
use crate::rules::{Protocol, norm_app, runs};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FlowCounters {
    pub protocol: Protocol,
    pub local_port: u16,
    /// ip:port
    pub dst: String,
    pub domain: Option<String>,
    /// TLS SNI, or the `Host` of plain HTTP.
    pub sni: Option<String>,
    pub ipv6: bool,
    pub via_system_proxy: bool,
    /// The hub's router takes this flow through the proxy and injects its replies itself. The
    /// sniff handle opens at the router's WinDivert priority and never sees injected packets, so
    /// `down_*` would stay 0: `down_bytes` holds the reply bytes the router counted (`mark_routed`).
    pub via_router: bool,
    pub up_packets: u32,
    pub down_packets: u32,
    pub up_bytes: f64,
    pub down_bytes: f64,
    /// SYNs sent, the first one included.
    pub syns: u32,
    pub syn_answered: bool,
    pub syn_ack_ms: Option<u32>,
    /// RST in the first `RESPONSE_WINDOW` bytes of the response.
    pub rst_early: bool,
    /// An HTTP redirect in the first `RESPONSE_WINDOW` bytes of the response.
    pub redirect_early: bool,
    /// Resent data segments starting in the first `RETRANS_WINDOW` bytes sent.
    pub data_retrans: u32,
    pub udp_first_reply_ms: Option<u32>,
    /// Closed 1 s windows with packets.
    pub udp_windows: u32,
    /// Windows with packets out and none back.
    pub udp_silent_windows: u32,
    pub rtt_ms: Vec<u32>,
}

const RESPONSE_WINDOW: u64 = 4096;
const RETRANS_WINDOW: u32 = 32_768;
const UDP_WINDOW: Duration = Duration::from_secs(1);
/// A ClientHello longer than this (or not one) gives no SNI.
const HELLO_MAX: usize = 16_384;
const MAX_RTT_SAMPLES: usize = 256;

type Key = (Protocol, u16, SocketAddr);

#[derive(Default)]
struct Flow {
    c: FlowCounters,
    /// The last SYN sent.
    syn_at: Option<Instant>,
    /// Sequence number of the first data byte.
    isn: Option<u32>,
    /// Data bytes sent (the end of the highest segment).
    sent: u32,
    /// (end offset, sent at) of a segment awaiting its ACK; a resend drops it (Karn).
    probe: Option<(u32, Instant)>,
    /// Response data bytes received.
    down: u64,
    /// The client's first bytes while its name (SNI or `Host`) is unknown.
    hello: Vec<u8>,
    named: bool,
    first_up: Option<Instant>,
    /// (start, packets out, packets back) of the open UDP window.
    window: Option<(Instant, u32, u32)>,
}

/// Counters per (protocol, local port, remote address). Loopback flows are skipped, except those to
/// the system proxy (`proxy_port`): they are marked and get no counters, since what happens to them
/// past the proxy is not in these packets.
pub struct Tally {
    flows: HashMap<Key, Flow>,
    proxy_port: Option<u16>,
}

fn ms(d: Duration) -> u32 {
    u32::try_from(d.as_millis()).unwrap_or(u32::MAX)
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes(
        b.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// Protocol, source and destination of an IPv4 (via `packet`) or IPv6 TCP/UDP packet. IPv6
/// extension headers are not followed.
fn endpoints(p: &[u8]) -> Option<(Protocol, SocketAddr, SocketAddr)> {
    match p.first()? >> 4 {
        4 => match parse_tcp(p) {
            Some(t) => Some((Protocol::Tcp, t.src.into(), t.dst.into())),
            None => parse_udp(p).map(|u| (Protocol::Udp, u.src.into(), u.dst.into())),
        },
        6 if p.len() >= 44 => {
            let protocol = match p[6] {
                6 => Protocol::Tcp,
                17 => Protocol::Udp,
                _ => return None,
            };
            let ip = |at: usize| {
                IpAddr::V6(Ipv6Addr::from(
                    <[u8; 16]>::try_from(&p[at..at + 16]).unwrap(),
                ))
            };
            let port = |at: usize| u16::from_be_bytes([p[at], p[at + 1]]);
            Some((
                protocol,
                SocketAddr::new(ip(8), port(40)),
                SocketAddr::new(ip(24), port(42)),
            ))
        }
        _ => None,
    }
}

/// The server_name of a TLS ClientHello in one record; `None` while `b` is incomplete.
fn sni(b: &[u8]) -> Option<Option<String>> {
    if b.first() != Some(&0x16) || b.get(5).is_some_and(|&t| t != 1) {
        return Some(None);
    }
    // Record header (5), handshake header (4), version (2), random (32).
    let mut at = 43;
    at += 1 + usize::from(*b.get(at)?); // session id
    at += 2 + be16(b, at)?; // cipher suites
    at += 1 + usize::from(*b.get(at)?); // compression methods
    let end = at + 2 + be16(b, at)?;
    at += 2;
    while at + 4 <= end {
        let (kind, len) = (be16(b, at)?, be16(b, at + 2)?);
        if kind == 0 {
            // List length (2), name type (1), name length (2), name.
            let n = be16(b, at + 7)?;
            let name = b.get(at + 9..at + 9 + n)?;
            return Some(std::str::from_utf8(name).ok().map(str::to_lowercase));
        }
        at += 4 + len;
    }
    Some(None)
}

/// The `Host` of a request's complete lines; `None` while the headers go on and it is not seen.
fn host(request: &[u8]) -> Option<Option<String>> {
    let ended = request.windows(4).any(|w| w == b"\r\n\r\n");
    // A line cut by the segment's end is not read yet.
    let complete = match request.windows(2).rposition(|w| w == b"\r\n") {
        Some(end) => &request[..end + 2],
        None => &[],
    };
    let found = String::from_utf8_lossy(complete)
        .split("\r\n")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("host")
                .then(|| v.trim().to_lowercase())
        });
    match found {
        Some(name) => Some(Some(name)),
        None if ended => Some(None),
        None => None,
    }
}

fn redirect(response: &[u8]) -> bool {
    response.starts_with(b"HTTP/1.")
        && [b"301", b"302", b"303", b"307", b"308"]
            .iter()
            .any(|code| response.get(9..12) == Some(&code[..]))
}

impl Flow {
    fn name(&mut self, port: u16, payload: &[u8]) {
        let read = match port {
            443 => sni,
            80 => host,
            _ => |_: &[u8]| Some(None),
        };
        self.hello.extend_from_slice(payload);
        let found = match read(&self.hello) {
            None if self.hello.len() < HELLO_MAX => return,
            found => found.flatten(),
        };
        self.named = true;
        self.hello = Vec::new();
        self.c.sni = found;
    }

    fn tcp(&mut self, t: &Tcp, seq: u32, ack: u32, up: bool, now: Instant) {
        let syn = t.flags & SYN != 0;
        let acks = t.flags & ACK != 0;
        let c = &mut self.c;
        if up {
            if syn && !acks {
                c.syns += 1;
                self.syn_at = Some(now);
                self.isn = Some(seq.wrapping_add(1));
                return;
            }
            if t.payload.is_empty() {
                return;
            }
            let isn = *self.isn.get_or_insert(seq);
            let at = seq.wrapping_sub(isn);
            // Before an ISN guessed mid-stream: an old segment, not one past 2 GiB.
            if at >= 1 << 31 {
                return;
            }
            if at < self.sent {
                if at < RETRANS_WINDOW {
                    c.data_retrans += 1;
                }
                self.probe = None;
                return;
            }
            let contiguous = at == self.sent;
            self.sent = at.wrapping_add(t.payload.len() as u32);
            self.probe.get_or_insert((self.sent, now));
            if !self.named && contiguous {
                self.name(t.dst.port(), t.payload);
            }
            return;
        }
        if syn && acks {
            if !c.syn_answered {
                c.syn_answered = true;
                if let Some(at) = self.syn_at {
                    c.syn_ack_ms = Some(ms(now - at));
                    c.rtt_ms.push(ms(now - at));
                }
            }
            return;
        }
        if acks
            && let (Some(isn), Some((end, at))) = (self.isn, self.probe)
            && ack.wrapping_sub(isn).wrapping_sub(end) < 1 << 31
        {
            if c.rtt_ms.len() < MAX_RTT_SAMPLES {
                c.rtt_ms.push(ms(now - at));
            }
            self.probe = None;
        }
        if self.down < RESPONSE_WINDOW {
            c.rst_early |= t.flags & RST != 0;
            c.redirect_early |= redirect(t.payload);
        }
        self.down += t.payload.len() as u64;
    }

    /// `all`: the open window too, however short (the observation ends).
    fn close_window(&mut self, now: Instant, all: bool) {
        if let Some((start, up, down)) = self.window
            && (all || now.duration_since(start) >= UDP_WINDOW)
        {
            let c = &mut self.c;
            c.udp_windows += 1;
            if up > 0 && down == 0 {
                c.udp_silent_windows += 1;
            }
            self.window = None;
        }
    }

    fn udp(&mut self, up: bool, now: Instant) {
        self.close_window(now, false);
        let w = self.window.get_or_insert((now, 0, 0));
        if up {
            w.1 += 1;
            self.first_up.get_or_insert(now);
        } else {
            w.2 += 1;
            let c = &mut self.c;
            if c.udp_first_reply_ms.is_none()
                && let Some(at) = self.first_up
            {
                c.udp_first_reply_ms = Some(ms(now - at));
            }
        }
    }
}

impl Tally {
    pub fn new(proxy_port: Option<u16>) -> Tally {
        Tally {
            flows: HashMap::new(),
            proxy_port,
        }
    }

    pub fn outbound(&mut self, packet: &[u8], now: Instant) {
        self.packet(packet, true, now);
    }

    pub fn inbound(&mut self, packet: &[u8], now: Instant) {
        self.packet(packet, false, now);
    }

    fn packet(&mut self, p: &[u8], up: bool, now: Instant) {
        let Some((protocol, src, dst)) = endpoints(p) else {
            return;
        };
        let (local, remote) = if up { (src, dst) } else { (dst, src) };
        let proxied = remote.ip().is_loopback() && Some(remote.port()) == self.proxy_port;
        if remote.ip().is_loopback() && !proxied {
            return;
        }
        let flow = self
            .flows
            .entry((protocol, local.port(), remote))
            .or_insert_with(|| Flow {
                c: FlowCounters {
                    protocol,
                    local_port: local.port(),
                    dst: remote.to_string(),
                    ipv6: remote.is_ipv6(),
                    via_system_proxy: proxied,
                    ..FlowCounters::default()
                },
                ..Flow::default()
            });
        let c = &mut flow.c;
        if proxied {
            return;
        }
        if up {
            c.up_packets += 1;
            c.up_bytes += p.len() as f64;
        } else {
            c.down_packets += 1;
            c.down_bytes += p.len() as f64;
        }
        if c.ipv6 {
            return;
        }
        match protocol {
            Protocol::Udp => flow.udp(up, now),
            _ => {
                let t = parse_tcp(p).unwrap();
                let ihl = usize::from(p[0] & 0x0F) * 4;
                flow.tcp(&t, be32(p, ihl + 4), be32(p, ihl + 8), up, now);
            }
        }
    }

    /// Closes the UDP windows open for a second or longer.
    pub fn tick(&mut self, now: Instant) {
        for flow in self.flows.values_mut() {
            flow.close_window(now, false);
        }
    }

    /// Closes every open UDP window: the observation ends and its last packets count.
    pub fn finish(&mut self, now: Instant) {
        for flow in self.flows.values_mut() {
            flow.close_window(now, true);
        }
    }

    /// Sorted by destination, then local port.
    pub fn rows(&self) -> Vec<FlowCounters> {
        let mut rows: Vec<FlowCounters> = self.flows.values().map(|f| f.c.clone()).collect();
        rows.sort_by(|a, b| (&a.dst, a.local_port).cmp(&(&b.dst, b.local_port)));
        rows
    }
}

/// Packets of the programs' local ports, both directions, IPv4 and IPv6. Ports merge into ranges
/// (the closest first) until at most `MAX_FILTER_RANGES` comparisons are left, far inside
/// `divert::FILTER_BUDGET`; the superset brings other programs' packets, which `Observer` drops.
pub fn sniff_filter(ports: &[u16]) -> Option<String> {
    let mut ranges: Vec<(u16, u16)> = ports.iter().map(|&p| (p, p)).collect();
    ranges.sort_unstable();
    ranges.dedup();
    if ranges.is_empty() {
        return None;
    }
    let tests =
        |r: &[(u16, u16)]| -> usize { r.iter().map(|&(a, b)| if a == b { 1 } else { 2 }).sum() };
    while tests(&ranges) > MAX_FILTER_RANGES && divert::merge_closest(vec![&mut ranges]) {}
    let parts: Vec<String> = ranges
        .iter()
        .map(|&(a, b)| match a == b {
            true => format!("localPort == {a}"),
            false => format!("(localPort >= {a} and localPort <= {b})"),
        })
        .collect();
    Some(format!(
        "(ip or ipv6) and (tcp or udp) and ({})",
        parts.join(" or ")
    ))
}

/// Marks the flows the router proxies (any of `rows`, from `Router::flows` over time, not
/// direct) and takes their reply bytes from it, the most it counted.
pub fn mark_routed(flows: &mut [FlowCounters], rows: &[FlowRow]) {
    for f in flows {
        let protocol = match f.protocol {
            Protocol::Tcp => "tcp",
            Protocol::Udp | Protocol::Both => "udp",
        };
        for r in rows.iter().filter(|r| {
            r.protocol == protocol
                && r.local_port == f.local_port
                && r.destination == f.dst
                && !matches!(r.via, Via::Direct { .. })
        }) {
            f.via_router = true;
            f.down_bytes = f.down_bytes.max(r.down);
        }
    }
}

/// Unlike `divert::SOCKET_FILTER`, loopback too: a program's connection to the system proxy is
/// learnt from its loopback CONNECT.
const SOCKET_FILTER: &str = "(udp or tcp) and (event == BIND or event == CONNECT)";
/// Every SYN, SYN-ACK and ClientHello (first byte 22, handshake type 1) of the machine: a new
/// connection's port joins the filter only after its socket event and a reopen, by when its
/// first SYN and often its ClientHello have passed.
const EARLY: &str = "tcp.Syn or (outbound and tcp.Payload[0] == 22 and tcp.Payload[5] == 1)";
const POLL: Duration = Duration::from_millis(50);
const REOPEN_GAP: Duration = Duration::from_secs(1);

struct Shared {
    stopped: AtomicBool,
    tally: Mutex<Tally>,
    names: Mutex<Names>,
    /// The programs' sockets by protocol and local port; never removed during an observation
    /// (late packets).
    ports: Mutex<HashSet<(Protocol, u16)>>,
    sockets: Handle,
    /// `None` when WinDivert refused it (seen while the router holds its own DNS sniffer): names
    /// then come from SNI and Host only.
    dns: Option<Handle>,
    sniff: Mutex<Option<Arc<Handle>>>,
}

/// Own passive WinDivert handles (independent of the router's): socket events pick the programs'
/// ports, a DNS sniffer names addresses, and a network sniffer feeds a `Tally`.
pub struct Observer {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

/// The sniff handle's whole filter: `EARLY` and the programs' ports.
pub fn sniff_text(ports: &[u16]) -> String {
    match sniff_filter(ports) {
        Some(f) => format!("{EARLY} or ({f})"),
        None => EARLY.into(),
    }
}

fn open_sniff(dll: &Dll, ports: &[u16]) -> std::io::Result<Arc<Handle>> {
    Handle::open(
        dll,
        &sniff_text(ports),
        Layer::Network,
        FLAG_SNIFF | FLAG_RECV_ONLY,
    )
    .map(Arc::new)
}

fn read_sniff(shared: Arc<Shared>, handle: Arc<Handle>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; MAX_PACKET];
        while let Ok((n, addr)) = handle.recv(&mut buf) {
            let mut tally = shared.tally.lock().unwrap();
            if addr.outbound() {
                tally.outbound(&buf[..n], Instant::now());
            } else {
                tally.inbound(&buf[..n], Instant::now());
            }
        }
    })
}

const IPPROTO_UDP: u8 = 17;

/// The rows of the programs' sockets: a TCP and a UDP socket may share a port number.
pub fn of_programs(
    mut rows: Vec<FlowCounters>,
    ports: &HashSet<(Protocol, u16)>,
) -> Vec<FlowCounters> {
    rows.retain(|r| ports.contains(&(r.protocol, r.local_port)));
    rows
}

impl Observer {
    /// `apps` as in routing rules (a file name or a full path).
    pub fn start(dll: &Path, apps: Vec<String>, proxy_port: Option<u16>) -> Result<Observer> {
        let dll = Dll::load(dll)?;
        let passive = FLAG_SNIFF | FLAG_RECV_ONLY;
        let sockets = Handle::open(&dll, SOCKET_FILTER, Layer::Socket, passive)
            .map_err(|e| anyhow!("WinDivert refused the socket handle: {e}"))?;
        let dns = Handle::open(&dll, DNS_FILTER, Layer::Network, passive)
            .inspect_err(|e| {
                log::warn!("observing without DNS names: WinDivert refused the handle: {e}")
            })
            .ok();
        let sniff = open_sniff(&dll, &[])
            .map_err(|e| anyhow!("WinDivert refused the sniff handle: {e}"))?;
        let shared = Arc::new(Shared {
            stopped: AtomicBool::new(false),
            tally: Mutex::new(Tally::new(proxy_port)),
            names: Mutex::new(Names::default()),
            ports: Mutex::new(HashSet::new()),
            sockets,
            dns,
            sniff: Mutex::new(Some(sniff.clone())),
        });
        let apps: Vec<String> = apps.iter().map(|a| norm_app(a)).collect();
        let mut threads = Vec::new();

        let s = shared.clone();
        threads.push(std::thread::spawn(move || {
            let images = Images::default();
            // `SOCKET_FILTER` passes BIND and CONNECT only.
            while let Ok((_, addr)) = s.sockets.recv(&mut []) {
                if images
                    .get(addr.socket_pid())
                    .is_some_and(|exe| apps.iter().any(|a| runs(a, &exe)))
                {
                    let protocol = match addr.socket_protocol() {
                        IPPROTO_UDP => Protocol::Udp,
                        _ => Protocol::Tcp,
                    };
                    s.ports
                        .lock()
                        .unwrap()
                        .insert((protocol, addr.socket_local_port()));
                }
            }
        }));

        let s = shared.clone();
        threads.push(std::thread::spawn(move || {
            let Some(dns) = &s.dns else { return };
            let mut buf = vec![0u8; MAX_PACKET];
            while let Ok((n, _)) = dns.recv(&mut buf) {
                if let Some(answer) = parse_udp(&buf[..n]).and_then(|u| dns::parse(u.payload)) {
                    s.names.lock().unwrap().record(&answer, Instant::now());
                }
            }
        }));

        // Ticks the UDP windows and reopens the sniff handle when the ports change: the old one
        // stops and its reader drains first, so no packet is counted twice or out of order (a few
        // may be missed in between).
        let s = shared.clone();
        threads.push(std::thread::spawn(move || {
            let mut reader = Some(read_sniff(s.clone(), sniff));
            let mut current: Vec<u16> = vec![];
            let mut opened: Option<Instant> = None;
            while !s.stopped.load(Ordering::SeqCst) {
                std::thread::sleep(POLL);
                s.tally.lock().unwrap().tick(Instant::now());
                let mut wanted: Vec<u16> = s.ports.lock().unwrap().iter().map(|k| k.1).collect();
                wanted.sort_unstable();
                wanted.dedup();
                if wanted == current || opened.is_some_and(|at| at.elapsed() < REOPEN_GAP) {
                    continue;
                }
                if let Some(old) = s.sniff.lock().unwrap().take() {
                    old.shutdown();
                }
                // Bounded: recv fails once the shut handle's queue is drained.
                if let Some(r) = reader.take() {
                    let _ = r.join();
                }
                match open_sniff(&dll, &wanted) {
                    Ok(h) => {
                        reader = Some(read_sniff(s.clone(), h.clone()));
                        *s.sniff.lock().unwrap() = Some(h);
                        current = wanted;
                    }
                    // Retried after the gap: the old handle is already shut.
                    Err(e) => log::warn!("observe: WinDivert refused the sniff filter: {e}"),
                }
                opened = Some(Instant::now());
            }
            if let Some(h) = s.sniff.lock().unwrap().take() {
                h.shutdown();
            }
            if let Some(r) = reader {
                let _ = r.join();
            }
        }));
        Ok(Observer { shared, threads })
    }

    /// The programs' flows so far, named from DNS where known.
    pub fn snapshot(&self) -> Vec<FlowCounters> {
        let ports = self.shared.ports.lock().unwrap().clone();
        let mut rows = of_programs(self.shared.tally.lock().unwrap().rows(), &ports);
        let names = self.shared.names.lock().unwrap();
        let now = Instant::now();
        for r in &mut rows {
            if let Ok(SocketAddr::V4(dst)) = r.dst.parse() {
                r.domain = names.name_of(*dst.ip(), now).map(str::to_string);
            }
        }
        rows
    }

    /// False when WinDivert refused the DNS sniffer: flows then carry SNI or Host names only.
    pub fn has_dns(&self) -> bool {
        self.shared.dns.is_some()
    }

    pub fn stop(mut self) -> Vec<FlowCounters> {
        self.halt();
        self.shared.tally.lock().unwrap().finish(Instant::now());
        self.snapshot()
    }

    fn halt(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.shared.sockets.shutdown();
        if let Some(dns) = &self.shared.dns {
            dns.shutdown();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.halt();
    }
}

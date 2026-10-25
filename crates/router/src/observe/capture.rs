//! The WinDivert sniffers of an `Observer`.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};

use super::{FlowCounters, Tally, of_programs, sniff_text};
use crate::divert::{DNS_FILTER, Dll, FLAG_RECV_ONLY, FLAG_SNIFF, Handle, Layer, MAX_PACKET};
use crate::dns::{self, Names};
use crate::owner::Images;
use crate::packet::parse_udp_any;
use crate::rules::{Protocol, norm_app, runs};

const IPPROTO_UDP: u8 = 17;

/// Unlike `divert::SOCKET_FILTER`, loopback too: a program's connection to the system proxy is
/// learnt from its loopback CONNECT.
const SOCKET_FILTER: &str = "(udp or tcp) and (event == BIND or event == CONNECT)";
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
                if let Some(answer) = parse_udp_any(&buf[..n]).and_then(|(_, _, p)| dns::parse(p)) {
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
            if let Ok(dst) = r.dst.parse::<SocketAddr>() {
                r.domain = names.name_of(dst.ip(), now).map(str::to_string);
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

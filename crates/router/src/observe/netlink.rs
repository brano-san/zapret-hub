//! The Linux `Observer`: conntrack over netlink (events, and a dump each second that catches what
//! the events missed and reads the timeouts, see `conntrack`), owners from `/proc` while the
//! sockets live, names from the DNS sniffer. Needs the routing capabilities (`CAP_NET_ADMIN` for
//! ctnetlink, `CAP_NET_RAW` for the sniffer). Conntrack tracks only while something in the network
//! namespace needs it: a table of the hub's own with one `ct` match makes sure, for the
//! observation only. Nothing is held or changed.

use std::collections::HashSet;
use std::io::{ErrorKind, Read};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use socket2::{Domain, Socket, Type};

use super::conntrack::{Conns, EVENT_GROUPS, Msg, dump_request, parse};
use super::{FlowCounters, of_programs};
use crate::dns::Names;
use crate::linux::caps;
use crate::linux::live::run;
use crate::linux::sniff::{Asked, Seen, Sniffer};
use crate::owner::{Images, port_pids};
use crate::rules::{Protocol, norm_app, runs};

const POLL: Duration = Duration::from_millis(100);
const DUMP_EVERY: Duration = Duration::from_secs(1);
const TABLE: &str = "zapret_hub_observe";

struct Shared {
    stopped: AtomicBool,
    conns: Mutex<Conns>,
    names: Mutex<Names>,
    /// The programs' sockets by protocol and local port; never removed during an observation.
    ports: Mutex<HashSet<(Protocol, u16)>>,
    dns: bool,
}

pub struct Observer {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    /// Our conntrack table is in place: dropped on stop.
    table: bool,
}

/// A ctnetlink socket; `groups`: the event groups to join (0: none, for dumps).
fn netlink(groups: u32) -> std::io::Result<Socket> {
    let socket = Socket::new(
        Domain::from(libc::AF_NETLINK),
        Type::RAW,
        Some(libc::NETLINK_NETFILTER.into()),
    )?;
    // SAFETY: a zeroed `sockaddr_nl` is valid; the size passed is its own.
    let rc = unsafe {
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        addr.nl_groups = groups;
        libc::bind(
            std::os::fd::AsRawFd::as_raw_fd(&socket),
            (&raw const addr).cast(),
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    socket.set_read_timeout(Some(POLL))?;
    if groups != 0 {
        // Bursts of new flows; an overflow only costs events, the next dump has them.
        let _ = socket.set_recv_buffer_size(1 << 20);
    }
    Ok(socket)
}

/// The whole table, both families; an error reply as `Msg::Error`.
fn dump(socket: &Socket, seq: u32, buf: &mut [u8]) -> Vec<Msg> {
    let mut out = Vec::new();
    if let Err(e) = socket.send(&dump_request(seq)) {
        out.push(Msg::Error(-e.raw_os_error().unwrap_or(libc::EIO)));
        return out;
    }
    // Bounded: a silent socket times out after `POLL`.
    while let Ok(n) = (&*socket).read(buf) {
        let msgs = parse(&buf[..n]);
        let end = n == 0 || msgs.iter().any(|m| !matches!(m, Msg::Entry(_)));
        out.extend(msgs);
        if end {
            break;
        }
    }
    out
}

/// A program as `runs` takes it, without the `.exe` of the profiles (`Discord.exe` is `Discord`).
fn linux_app(app: &str) -> String {
    let app = norm_app(app);
    app.strip_suffix(".exe").map(str::to_owned).unwrap_or(app)
}

fn nft(text: &str) -> Result<(), String> {
    run("nft", &["-f", "-"], Some(text)).map(drop)
}

impl Observer {
    /// `apps` as in routing rules; `_dll` is WinDivert's, Windows only.
    pub fn start(_dll: &Path, apps: Vec<String>, proxy_port: Option<u16>) -> Result<Observer> {
        if !caps::present() {
            bail!(
                "flow observation needs the routing capabilities: {}",
                caps::HINT
            );
        }
        // Replaces what a crashed observation left.
        let table = nft(&format!(
            "add table inet {TABLE}\ndelete table inet {TABLE}\n\
             table inet {TABLE} {{ chain track {{ ct state new; }}; }}\n"
        ))
        .inspect_err(|e| log::warn!("observe: conntrack may be off, no table of ours: {e}"))
        .is_ok();
        let mut observer = Observer {
            shared: Arc::new(Shared {
                stopped: AtomicBool::new(false),
                conns: Mutex::new(Conns::new(proxy_port)),
                names: Mutex::new(Names::default()),
                ports: Mutex::new(HashSet::new()),
                dns: false,
            }),
            threads: Vec::new(),
            table,
        };
        let events = netlink(EVENT_GROUPS)?;
        let dumps = netlink(0)?;
        let mut buf = vec![0u8; 1 << 16];
        if let Some(Msg::Error(e)) = dump(&dumps, 0, &mut buf)
            .iter()
            .find(|m| matches!(m, Msg::Error(e) if *e != 0))
        {
            bail!(
                "conntrack refused the dump: {}",
                std::io::Error::from_raw_os_error(-e)
            );
        }
        let sniffer = Sniffer::open(POLL)
            .inspect_err(|e| log::warn!("observing without DNS names: no sniffer: {e}"))
            .ok();
        Arc::get_mut(&mut observer.shared).unwrap().dns = sniffer.is_some();
        let apps: Vec<String> = apps.iter().map(|a| linux_app(a)).collect();

        let s = observer.shared.clone();
        observer.threads.push(std::thread::spawn(move || {
            track(&s, &events, &dumps, &apps)
        }));
        if let Some(sniffer) = sniffer {
            let s = observer.shared.clone();
            observer
                .threads
                .push(std::thread::spawn(move || names(&s, &sniffer)));
        }
        Ok(observer)
    }

    /// The programs' flows so far, named from DNS where known.
    pub fn snapshot(&self) -> Vec<FlowCounters> {
        let ports = self.shared.ports.lock().unwrap().clone();
        let mut rows = of_programs(self.shared.conns.lock().unwrap().rows(), &ports);
        let names = self.shared.names.lock().unwrap();
        let now = Instant::now();
        for r in &mut rows {
            if let Ok(SocketAddr::V4(dst)) = r.dst.parse() {
                r.domain = names.name_of(*dst.ip(), now).map(str::to_string);
            }
        }
        rows
    }

    /// False without the DNS sniffer: flows then carry no names.
    pub fn has_dns(&self) -> bool {
        self.shared.dns
    }

    pub fn stop(mut self) -> Vec<FlowCounters> {
        self.halt();
        self.snapshot()
    }

    fn halt(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        if std::mem::take(&mut self.table)
            && let Err(e) = nft(&format!("delete table inet {TABLE}\n"))
        {
            log::warn!("observe: cannot remove the conntrack table: {e}");
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Feeds `Conns` from events and dumps, and learns which local ports are the programs'.
fn track(s: &Shared, events: &Socket, dumps: &Socket, apps: &[String]) {
    let mut buf = vec![0u8; 1 << 16];
    let images = Images::default();
    // Entries whose owner was looked up (by conntrack id): once each.
    let mut asked: HashSet<u32> = HashSet::new();
    let mut pending: Vec<(Protocol, u16)> = Vec::new();
    let (mut dumped, mut resolved): (Option<Instant>, Option<Instant>) = (None, None);
    let mut seq = 0;
    while !s.stopped.load(Ordering::SeqCst) {
        // A timeout, or an overflow (`ENOBUFS`) that the next dump makes up for.
        let mut seen = match (&*events).read(&mut buf) {
            Ok(n) => parse(&buf[..n]),
            Err(_) => Vec::new(),
        };
        if dumped.is_none_or(|at| at.elapsed() >= DUMP_EVERY) {
            seq += 1;
            seen.extend(dump(dumps, seq, &mut buf));
            dumped = Some(Instant::now());
        }
        let now = Instant::now();
        {
            let mut conns = s.conns.lock().unwrap();
            let ports = s.ports.lock().unwrap();
            for e in seen.iter().filter_map(|m| match m {
                Msg::Entry(e) => Some(e),
                _ => None,
            }) {
                conns.update(e, now);
                let key = (e.protocol, e.src.port());
                if !ports.contains(&key) && asked.insert(e.id) {
                    pending.push(key);
                }
            }
        }
        // ponytail: one walk of every process's fds per `POLL` while new flows come; an fd
        // cache (or sock_diag) if that shows on busy machines
        if pending.is_empty() || resolved.is_some_and(|at| at.elapsed() < POLL) {
            continue;
        }
        resolved = Some(Instant::now());
        let owners = port_pids();
        let mut ours = Vec::new();
        for key in pending.drain(..) {
            if owners
                .get(&key)
                .and_then(|pid| images.get(*pid))
                .is_some_and(|exe| apps.iter().any(|a| runs(a, &exe)))
            {
                ours.push(key);
            }
        }
        s.ports.lock().unwrap().extend(ours);
    }
}

/// Names the addresses from DNS answers to queries seen going out.
fn names(s: &Shared, sniffer: &Sniffer) {
    let mut buf = vec![0u8; 65536];
    let mut asked = Asked::default();
    while !s.stopped.load(Ordering::SeqCst) {
        let now = Instant::now();
        match sniffer.next(&mut buf) {
            Ok(Some(Seen::Query(e))) => asked.ask(e, now),
            Ok(Some(Seen::Answer(e, a))) if asked.answers(&e, now) => {
                s.names.lock().unwrap().record(&a, now);
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => std::thread::sleep(POLL),
        }
    }
}

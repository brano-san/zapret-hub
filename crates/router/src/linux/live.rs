//! A routing session on Linux: the nftables table and policy routing, the TCP acceptor, the
//! TPROXY socket, the DNS sniffer and the proxy checks. Rows, counters and proxy states are those
//! of Windows (`flows`), so the UI, the CLI and the diagnostics read the same data.

mod tcp;
mod udp;

use std::collections::HashMap;
use std::io::{ErrorKind, Write as _};
use std::mem::discriminant;
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket,
};
use std::os::linux::net::SocketAddrExt as _;
use std::os::unix::net::{SocketAddr as UnixAddr, UnixListener};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

use crate::dns::Names;
use crate::flows::{self, FlowKey, ProxyState, Stats, Via};
use crate::linux::sniff::{Asked, Seen, Sniffer};
use crate::linux::{caps, nft, sockopt};
use crate::owner::{self, Images};
use crate::rules::Table;
use crate::socks::{self, OPEN_TIMEOUT, SocksError};
use crate::{Inner, Setup};

const PROXY_CHECK: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_millis(500);
const RULES_CHECK_TICKS: u32 = 4;
/// How long a blocking read waits before it looks whether the session stopped.
const POLL: Duration = Duration::from_millis(500);
/// A DNS flow (one query and its answer, a new port each) holds its sockets and thread no
/// longer than this.
const DNS_IDLE: Duration = Duration::from_secs(5);

/// One running session: sockets, flows and counters; dropped whole on stop or pause.
pub(crate) struct Live {
    inner: Weak<Inner>,
    proxy: SocketAddrV4,
    stopped: AtomicBool,
    acceptor: TcpListener,
    /// The acceptor on `[::1]`, same port; with `tproxy6`, `None` when the system has no IPv6
    /// loopback (IPv6 then passes unrouted).
    acceptor6: Option<TcpListener>,
    acceptor_port: u16,
    tproxy: UdpSocket,
    tproxy6: Option<UdpSocket>,
    /// The rules without resolved names; `table` is it with them.
    base: Mutex<Table>,
    table: RwLock<Arc<Table>>,
    /// The match chains in force (`nft::update` text); held while `nft` runs, so updates queue.
    applied: Mutex<String>,
    names: Arc<Mutex<Names>>,
    images: Arc<Images>,
    flows: Mutex<HashMap<udp::UdpKey, udp::Flow>>,
    /// Both sockets of each relayed TCP connection, shut down when the session ends.
    conns: Mutex<HashMap<FlowKey, (TcpStream, TcpStream)>>,
    stats: Mutex<Stats>,
    /// IPv6 destinations whose UDP the proxy never answered: new flows to them go direct.
    silent6: Mutex<flows::Silent6>,
    proxy_state: Mutex<ProxyState>,
    proxy_pid: Mutex<Option<u32>>,
    /// `session_lock`, released once `shut` cleared the routing.
    lock: Mutex<Option<UnixListener>>,
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

/// Runs a system tool with the hub's routing capabilities: its output, or its error output as
/// the error.
pub(crate) fn run<S: AsRef<str>>(
    program: &str,
    args: &[S],
    stdin: Option<&str>,
) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let mut cmd = Command::new(zapret::root::tool(program));
    caps::inherit(zapret::root::c_locale(&mut cmd))
        .args(&args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // Dropping the pipe ends the input.
        let _ = pipe.write_all(text.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// One session per network namespace (the app and `zapret-hub-cli routes start` share one table):
/// an abstract socket name, which the kernel frees when its process dies, so a crash never blocks.
fn session_lock() -> Result<UnixListener> {
    let name = UnixAddr::from_abstract_name(format!("{}.lock", nft::TABLE))?;
    UnixListener::bind_addr(&name).map_err(|e| match e.kind() {
        ErrorKind::AddrInUse => anyhow::anyhow!(
            "another Zapret Hub process (the app or zapret-hub-cli) is routing already: stop it first"
        ),
        _ => anyhow::Error::new(e).context("cannot take the routing lock"),
    })
}

fn nft(text: &str) -> Result<(), String> {
    run("nft", &["-f", "-"], Some(text)).map(drop)
}

/// Removes the table and the policy routing of both families, also what a crashed session or an
/// older version left: every rule with our mark, and the route in the table it looks up.
/// Harmless when there is nothing to remove (or no IPv6).
fn clear() {
    let _ = nft(&nft::delete());
    for (family, v6) in [("-4", false), ("-6", true)] {
        if let Ok(rules) = run("ip", &[family, "rule", "show"], None) {
            for policy in nft::ours(&rules) {
                for argv in policy.argv(false, v6) {
                    let _ = run("ip", &argv, None);
                }
            }
        }
    }
}

/// The acceptor on 127.0.0.1 and, when the system has an IPv6 loopback, on `[::1]` at the same
/// port (one `redirect to :port` serves both families).
fn acceptors() -> Result<(TcpListener, Option<TcpListener>)> {
    for _ in 0..16 {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return Ok((v4, Some(v6))),
            // Someone holds the port on [::1]: another one.
            Err(e) if e.kind() == ErrorKind::AddrInUse => {}
            Err(e) => {
                log::info!("routing IPv4 only: no IPv6 loopback: {e}");
                return Ok((v4, None));
            }
        }
    }
    bail!("no port is free on both 127.0.0.1 and [::1]")
}

/// A rule priority and a table nobody else uses (`nft::pick`): `ip` rules of both families,
/// the `rt_tables` names and the routes of each candidate table.
fn free_policy() -> Result<nft::Policy, String> {
    let mut rules = run("ip", &["-4", "rule", "show"], None)?;
    rules += &run("ip", &["-6", "rule", "show"], None).unwrap_or_default();
    let mut named = Vec::new();
    let dropins = std::fs::read_dir("/etc/iproute2/rt_tables.d")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path());
    for path in [
        "/etc/iproute2/rt_tables",
        "/usr/share/iproute2/rt_tables",
        "/usr/lib/iproute2/rt_tables",
    ]
    .map(std::path::PathBuf::from)
    .into_iter()
    .chain(dropins)
    {
        named.extend(nft::named_tables(
            &std::fs::read_to_string(path).unwrap_or_default(),
        ));
    }
    let has_routes = |table: u32| {
        let table = table.to_string();
        ["-4", "-6"].iter().any(|family| {
            // A table never used "does not exist"; another IPv4 error counts as taken, no IPv6
            // at all as empty.
            match run("ip", &[*family, "route", "show", "table", &table], None) {
                Ok(routes) => !routes.trim().is_empty(),
                Err(e) => *family == "-4" && !e.contains("does not exist"),
            }
        })
    };
    nft::pick(&rules, &named, has_routes)
        .ok_or_else(|| "no free policy routing table: 256 tables from 23112 on are in use".into())
}

/// Clears what a crashed session left (`clear`) unless a live session holds the lock; needs the
/// capabilities. True when it cleared.
pub(crate) fn clear_stale() -> bool {
    let Ok(_lock) = session_lock() else {
        return false;
    };
    if !caps::present() {
        return false;
    }
    clear();
    true
}

/// `nft::GROUP` exists (`nft` refuses an unknown group name).
// ponytail: reads /etc/group only; a group from NSS (LDAP) is missed, then not skipped.
fn bypass_group() -> bool {
    let line = format!("{}:", nft::GROUP);
    std::fs::read_to_string("/etc/group").is_ok_and(|t| t.lines().any(|l| l.starts_with(&line)))
}

/// The subnet broadcasts of the local interfaces now: read on every `apply`, so housekeeping's
/// re-apply each tick puts a changed network in force.
fn broadcasts() -> Vec<Ipv4Addr> {
    nft::broadcasts(&sockopt::ipv4_addrs().unwrap_or_default())
}

/// Caches the exe of every process with a UDP socket: a UDP rule's program is running.
fn learn_udp_programs(images: &Images) {
    for pid in owner::udp_pids() {
        images.get(pid);
    }
}

impl Live {
    pub(crate) fn proxy(&self) -> ProxyState {
        self.proxy_state.lock().unwrap().clone()
    }

    pub(crate) fn stats(&self) -> MutexGuard<'_, Stats> {
        self.stats.lock().unwrap()
    }

    pub(crate) fn open(inner: &Arc<Inner>, setup: &Setup, table: Table) -> Result<Arc<Live>> {
        if !caps::present() {
            let installed = std::env::current_exe()
                .map(|exe| caps::installed(&exe))
                .unwrap_or(caps::Installed::None);
            bail!(caps::hint(installed));
        }
        // Taken before `clear`: what it removes is a dead session's.
        let lock = session_lock()?;
        clear();
        let (acceptor, acceptor6) = acceptors().context("cannot open the local TCP acceptor")?;
        let acceptor_port = acceptor.local_addr()?.port();
        let tproxy =
            sockopt::transparent_udp(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), nft::MARK_OWN)
                .context("cannot open the TPROXY socket")?;
        tproxy.set_read_timeout(Some(POLL))?;
        let tproxy_port = tproxy.local_addr()?.port();
        let tproxy6 = match &acceptor6 {
            Some(_) => {
                let socket = sockopt::transparent_udp(
                    SocketAddr::from((Ipv6Addr::LOCALHOST, 0)),
                    nft::MARK_OWN,
                )
                .context("cannot open the IPv6 TPROXY socket")?;
                socket.set_read_timeout(Some(POLL))?;
                Some(socket)
            }
            None => None,
        };
        let tproxy6_port = match &tproxy6 {
            Some(s) => Some(s.local_addr()?.port()),
            None => None,
        };
        let sniffer = Sniffer::open(POLL).context("cannot open the DNS sniffer")?;
        inner.names.lock().unwrap().expire(Instant::now(), &[]);
        if table.idle_apps() {
            learn_udp_programs(&inner.images);
        }
        let live = Arc::new(Live {
            inner: Arc::downgrade(inner),
            proxy: setup.proxy,
            stopped: AtomicBool::new(false),
            acceptor,
            acceptor6,
            acceptor_port,
            tproxy,
            tproxy6,
            base: Mutex::new(Table::default()),
            table: RwLock::new(Arc::new(Table::default())),
            applied: Mutex::new(String::new()),
            names: inner.names.clone(),
            images: inner.images.clone(),
            flows: Mutex::new(HashMap::new()),
            conns: Mutex::new(HashMap::new()),
            stats: Mutex::new(Stats::default()),
            silent6: Mutex::new(flows::Silent6::default()),
            proxy_state: Mutex::new(ProxyState::Unknown),
            proxy_pid: Mutex::new(None),
            lock: Mutex::new(Some(lock)),
        });
        {
            let mut applied = live.applied.lock().unwrap();
            let full = live.compose(Some(table));
            let broadcasts = broadcasts();
            let text = nft::ruleset(
                &full,
                acceptor_port,
                tproxy_port,
                tproxy6_port,
                setup.proxy,
                bypass_group(),
                &broadcasts,
            );
            let installed = nft(&text).and_then(|()| {
                let policy = free_policy()?;
                // The rule first: `clear` finds the route through it.
                let v6 = live.tproxy6.as_ref().map(|_| policy.argv(true, true));
                policy
                    .argv(true, false)
                    .iter()
                    .chain(v6.iter().flatten())
                    .try_for_each(|argv| run("ip", argv, None).map(drop))
            });
            if let Err(e) = installed {
                drop(applied);
                live.shut();
                bail!("cannot set up routing: {e}");
            }
            *applied = nft::update(&full, acceptor_port, &broadcasts);
        }
        let l = live.clone();
        spawn(move || l.accept_loop(&l.acceptor));
        let l = live.clone();
        spawn(move || l.udp_loop(&l.tproxy));
        if live.tproxy6.is_some() {
            let l = live.clone();
            spawn(move || l.accept_loop(l.acceptor6.as_ref().unwrap()));
            let l = live.clone();
            spawn(move || l.udp_loop(l.tproxy6.as_ref().unwrap()));
        }
        let l = live.clone();
        spawn(move || l.dns_loop(&sniffer));
        let l = live.clone();
        spawn(move || l.housekeeping());
        let l = live.clone();
        spawn(move || l.check_proxy());
        Ok(live)
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Takes the routing down first (no new connection reaches a closing session), then ends the
    /// relayed connections (programs reconnect, directly) and the threads. Works after a panic
    /// poisoned the locks; a second call does nothing new.
    pub(crate) fn shut(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        {
            // An `apply` past its `stopped` check finishes first: its old chains can never land
            // in the table of the next session.
            let _applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
            clear();
        }
        self.lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let conns: Vec<_> = self
            .conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .collect();
        for (_, (a, b)) in &conns {
            let _ = a.shutdown(std::net::Shutdown::Both);
            let _ = b.shutdown(std::net::Shutdown::Both);
        }
        // Wakes the acceptors, which then see `stopped`.
        for acceptor in std::iter::once(&self.acceptor).chain(&self.acceptor6) {
            if let Ok(addr) = acceptor.local_addr() {
                let _ = TcpStream::connect_timeout(&addr, Duration::from_secs(1));
            }
        }
        self.flows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// `base` (`None`: the rules in force) with the names known now and the programs running.
    fn compose(&self, base: Option<Table>) -> Arc<Table> {
        let base = match base {
            Some(b) => {
                *self.base.lock().unwrap() = b.clone();
                b
            }
            None => self.base.lock().unwrap().clone(),
        };
        let exes = self.images.paths();
        let exes: Vec<&str> = exes.iter().map(|e| &**e).collect();
        let table = Arc::new(
            base.with_names(&self.names.lock().unwrap().entries())
                .with_running(&exes),
        );
        *self.table.write().unwrap() = table.clone();
        table
    }

    /// New rules (`None`: the same ones with the names and programs known now) and the interfaces'
    /// broadcasts replace the match chains in one `nft` transaction: no moment without them, and live connections keep their
    /// redirect (conntrack NAT). Unchanged chains run nothing. `nft`'s refusal is the error.
    pub(crate) fn apply(self: &Arc<Self>, base: Option<Table>) -> Result<(), String> {
        let mut applied = self.applied.lock().unwrap();
        let table = self.compose(base);
        let text = nft::update(&table, self.acceptor_port, &broadcasts());
        if *applied == text || self.is_stopped() {
            return Ok(());
        }
        nft(&text)?;
        *applied = text;
        Ok(())
    }

    /// Re-applies the rules with the names known now; a refusal shows as the rules error.
    fn recompose(self: &Arc<Self>) {
        if self.is_stopped() {
            return;
        }
        let error = self.apply(None).err();
        if let Some(inner) = self.inner.upgrade() {
            inner.rules.lock().unwrap().filter_error = error;
        }
    }

    fn domain_of(&self, dst: SocketAddr, now: Instant) -> Option<String> {
        self.names
            .lock()
            .unwrap()
            .name_of(dst.ip(), now)
            .map(str::to_owned)
    }

    /// `rule` is empty for a flow caught for a program rule that matches none.
    fn open_row(
        &self,
        key: FlowKey,
        rule: &str,
        exe: Option<&str>,
        domain: Option<String>,
        via: Via,
        now: Instant,
    ) {
        let mut stats = self.stats.lock().unwrap();
        stats.open(key, rule, via, now);
        if let Some(d) = domain {
            stats.set_domain(key, d);
        }
        if let Some(exe) = exe {
            stats.set_program(key, exe.to_owned());
        }
    }

    /// Why a new flow of this owner goes direct, or `None` (through the proxy).
    fn decide(&self, pid: Option<u32>) -> Option<flows::DirectReason> {
        let state = self.proxy();
        let proxy_pid = *self.proxy_pid.lock().unwrap();
        flows::decide(&state, pid, std::process::id(), proxy_pid)
    }

    /// Records the addresses of names some rule wants, from answers to queries seen going out;
    /// ends with the session.
    fn dns_loop(self: &Arc<Self>, sniffer: &Sniffer) {
        let mut buf = vec![0u8; 65536];
        let mut asked = Asked::default();
        while !self.is_stopped() {
            let answer = match sniffer.next(&mut buf) {
                Ok(Some(Seen::Query(e))) => {
                    asked.ask(e, Instant::now());
                    continue;
                }
                Ok(Some(Seen::Answer(e, a))) if asked.answers(&e, Instant::now()) => a,
                Ok(_) => continue,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            if !self.base.lock().unwrap().wants(&answer.name) {
                continue;
            }
            if self.names.lock().unwrap().record(&answer, Instant::now()) {
                self.recompose();
            }
        }
    }

    /// Calls back only when the kind changes (ok -> down), not on every latency update.
    fn set_proxy(&self, new: ProxyState) {
        self.update_proxy(|_| Some(new));
    }

    /// Decides and writes under one lock, so a concurrent writer cannot be overwritten.
    fn update_proxy(&self, f: impl FnOnce(&ProxyState) -> Option<ProxyState>) {
        let mut state = self.proxy_state.lock().unwrap();
        let Some(new) = f(&state) else {
            return;
        };
        let changed = discriminant(&*state) != discriminant(&new);
        let failing = matches!(
            new,
            ProxyState::Down { .. } | ProxyState::AuthRequired | ProxyState::NoUdp
        );
        let recovered = changed && matches!(new, ProxyState::Ok { .. });
        *state = new;
        drop(state);
        if failing || recovered {
            // Their next datagrams open new flows: direct while it fails, through it once it is
            // back (as on Windows).
            let closing = self.udp_closing(failing, recovered);
            let now = Instant::now();
            for key in closing {
                self.close_flow(key, now);
            }
        }
        if changed
            && !self.is_stopped()
            && let Some(inner) = self.inner.upgrade()
        {
            inner.notify();
        }
    }

    /// Sleeps `total` in short steps; false once the session stopped.
    fn sleep(&self, total: Duration) -> bool {
        let end = Instant::now() + total;
        while !self.is_stopped() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            std::thread::sleep(left.min(Duration::from_millis(100)));
        }
        false
    }

    /// Right at start and every 10 s, debounced by `flows::checked`.
    fn check_proxy(&self) {
        let mut failures = 0;
        loop {
            let new = match socks::check(self.proxy, OPEN_TIMEOUT) {
                Ok(took) => ProxyState::Ok {
                    latency_ms: took.as_millis() as u32,
                },
                Err(SocksError::AuthRequired) => ProxyState::AuthRequired,
                Err(e) => ProxyState::Down {
                    error: e.to_string(),
                },
            };
            *self.proxy_pid.lock().unwrap() = owner::listener_pid(self.proxy.port());
            self.update_proxy(|current| flows::checked(current, new, &mut failures));
            if !self.sleep(PROXY_CHECK) {
                return;
            }
        }
    }

    /// Every tick: close idle flows, drop old closed rows, expire names (not those of live flows),
    /// put new names and programs in force; every 2 s: forget exited programs, look for a UDP
    /// rule's program not yet seen, re-read the rules.
    fn housekeeping(self: &Arc<Self>) {
        let mut tick = 0u32;
        while self.sleep(TICK) {
            tick += 1;
            let now = Instant::now();
            let idle = self.stats.lock().unwrap().idle_after(now, |k| {
                if k.dst.port() == 53 {
                    DNS_IDLE
                } else {
                    flows::IDLE
                }
            });
            for key in idle {
                self.close_flow(key, now);
            }
            self.stats.lock().unwrap().prune(now);
            let mut live: Vec<IpAddr> = self
                .flows
                .lock()
                .unwrap()
                .keys()
                .map(|k| k.0.dst.ip())
                .collect();
            live.extend(self.conns.lock().unwrap().keys().map(|k| k.dst.ip()));
            self.names.lock().unwrap().expire(now, &live);
            if tick.is_multiple_of(RULES_CHECK_TICKS) {
                self.images.prune(owner::process_start);
                if self.table.read().unwrap().idle_apps() {
                    learn_udp_programs(&self.images);
                }
                match self.inner.upgrade() {
                    Some(inner) => inner.refresh(),
                    None => return,
                }
            }
            self.recompose();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_session_holds_the_lock_until_it_lets_go() {
        let held = session_lock().unwrap();
        let err = session_lock().unwrap_err().to_string();
        assert!(err.contains("routing already"), "{err}");
        assert!(!clear_stale(), "a live session's routing stays");
        drop(held);
        session_lock().unwrap();
    }

    /// No capabilities (an ordinary test run): the router is unavailable with the hint.
    #[test]
    fn without_the_capabilities_the_router_is_unavailable_with_the_hint() {
        if caps::present() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let router = crate::Router::new(dir.path().join("routes.toml"));
        let setup = Setup {
            dll: Default::default(),
            proxy: "127.0.0.1:2080".parse().unwrap(),
        };
        let err = router.start(setup).unwrap_err();
        assert_eq!(err.to_string(), caps::HINT);
        assert_eq!(
            router.status().state,
            crate::RouterState::Unavailable {
                reason: caps::HINT.into()
            }
        );
        assert!(router.status().needs_grant);
    }
}

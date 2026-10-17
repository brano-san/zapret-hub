pub mod divert;
pub mod dns;
pub mod doh;
pub mod flows;
mod live_tcp;
pub mod owner;
pub mod packet;
pub mod rules;
pub mod socks;
pub mod tcp;

use std::collections::{HashMap, hash_map::Entry};
use std::mem::discriminant;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use divert::{
    Address, DNS_FILTER, Dll, FLAG_RECV_ONLY, FLAG_SNIFF, Handle, Layer, MAX_PACKET, PROTO_TCP,
    SOCKET_FILTER,
};
use dns::Names;
use flows::{DirectReason, FlowKey, FlowRow, Pending, ProxyState, Stats, Totals, Traffic, Via};
use owner::{Images, Owners, PID_WAIT};
use packet::{build_udp, parse_socks_udp_from, parse_tcp, parse_udp, socks_header_to};
use rules::{Protocol, Rule, Table};
use socks::{OPEN_TIMEOUT, SocksError};

const PROXY_CHECK: Duration = Duration::from_secs(10);
/// At most 500 ms: an unaccepted SYN must go direct before its 3 s retry (`tcp::ACCEPT_WAIT`).
const TICK: Duration = Duration::from_millis(500);
const RULES_CHECK_TICKS: u32 = 4;
const REOPEN_GAP: Duration = Duration::from_secs(1);
/// On a stop, time for the acceptor's FINs to pass the capture before it shuts down.
const FIN_DRAIN: Duration = Duration::from_millis(100);
/// The largest UDP payload an IPv4 packet carries (`build_udp` refuses more).
const MAX_PAYLOAD: usize = 65507;

#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    pub dll: PathBuf,
    pub proxy: SocketAddrV4,
}

pub fn parse_proxy(socks5: Option<&str>) -> Result<SocketAddrV4> {
    let text = socks5
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("set the SOCKS5 proxy in Settings first"))?;
    let addr: SocketAddrV4 = text.parse().map_err(|_| {
        anyhow!("the router needs an IPv4 SOCKS5 address like 127.0.0.1:2080: {text}")
    })?;
    // The relay sockets are bound to loopback (spec 1: a local proxy).
    if !addr.ip().is_loopback() {
        bail!("the router needs a local SOCKS5 proxy like 127.0.0.1:2080: {text}");
    }
    Ok(addr)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state")]
pub enum RouterState {
    Off,
    Running,
    Paused,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RouterStatus {
    pub state: RouterState,
    pub proxy: ProxyState,
    pub totals: Totals,
    pub rules_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RuleRow {
    pub rule: Rule,
    pub traffic: Traffic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RoutesView {
    pub rules: Vec<RuleRow>,
    pub error: Option<String>,
    /// Windows resolves names over DNS over HTTPS: domain rules see no answers.
    pub system_doh: bool,
}

#[derive(Clone)]
pub struct Router(Arc<Inner>);

type Callback = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    path: PathBuf,
    on_change: Mutex<Option<Callback>>,
    ctl: Mutex<Ctl>,
    /// State and session as `control` left them: `status` and `flows` never wait for a launch.
    snapshot: Mutex<(RouterState, Option<Arc<Live>>)>,
    rules: Mutex<RulesFile>,
    /// The table in force; a launch starts from it.
    table: Mutex<Table>,
    /// Outlives sessions: programs keep resolved addresses cached across a pause.
    names: Arc<Mutex<Names>>,
    /// Outlives sessions too: a program seen before a pause still counts as running.
    images: Arc<Images>,
}

struct Ctl {
    state: RouterState,
    setup: Option<Setup>,
    /// Set by `pause` in any state, cleared only by `resume`: a test run stays unrouted.
    paused: bool,
    live: Option<Arc<Live>>,
}

struct RulesFile {
    list: Vec<Rule>,
    modified: Option<SystemTime>,
    file_error: Option<String>,
    filter_error: Option<String>,
}

impl RulesFile {
    fn error(&self) -> Option<String> {
        self.file_error
            .clone()
            .or_else(|| self.filter_error.clone())
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Re-reads the file when its write time changed (or `force`); a broken file keeps the rules.
/// True when the rules were replaced.
fn reload(path: &Path, file: &mut RulesFile, force: bool) -> bool {
    let now = modified(path);
    if !force && now == file.modified {
        return false;
    }
    file.modified = now;
    match rules::load(path) {
        Ok(list) => {
            file.list = list;
            file.file_error = None;
            true
        }
        Err(e) => {
            file.file_error = Some(e.to_string());
            false
        }
    }
}

impl Router {
    pub fn new(routes: PathBuf) -> Router {
        let mut file = RulesFile {
            list: Vec::new(),
            modified: None,
            file_error: None,
            filter_error: None,
        };
        reload(&routes, &mut file, true);
        let table = Mutex::new(Table::new(&file.list));
        Router(Arc::new(Inner {
            table,
            names: Arc::default(),
            images: Arc::default(),
            path: routes,
            on_change: Mutex::new(None),
            ctl: Mutex::new(Ctl {
                state: RouterState::Off,
                setup: None,
                paused: false,
                live: None,
            }),
            snapshot: Mutex::new((RouterState::Off, None)),
            rules: Mutex::new(file),
        }))
    }

    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.0.on_change.lock().unwrap() = Some(Arc::new(f));
    }

    /// Runs `op` on the control block, then calls back (no lock held) if the state changed.
    fn control<T>(&self, op: impl FnOnce(&Arc<Inner>, &mut Ctl) -> T) -> T {
        let mut ctl = self.0.ctl.lock().unwrap();
        let before = ctl.state.clone();
        let out = op(&self.0, &mut ctl);
        let changed = ctl.state != before;
        *self.0.snapshot.lock().unwrap() = (ctl.state.clone(), ctl.live.clone());
        drop(ctl);
        if changed {
            self.0.notify();
        }
        out
    }

    pub fn start(&self, setup: Setup) -> Result<()> {
        self.control(|inner, ctl| {
            if ctl.paused {
                halt(ctl);
                ctl.setup = Some(setup);
                ctl.state = RouterState::Paused;
                return Ok(());
            }
            if ctl.state == RouterState::Running && ctl.setup.as_ref() == Some(&setup) {
                return Ok(());
            }
            ctl.setup = Some(setup.clone());
            launch(inner, ctl, &setup)
        })
    }

    pub fn fail(&self, reason: String) {
        self.control(|_, ctl| {
            halt(ctl);
            ctl.setup = None;
            ctl.state = RouterState::Unavailable { reason };
        });
    }

    /// The pause flag survives: only `resume` clears it (ruling F4).
    pub fn stop(&self) {
        self.control(|_, ctl| {
            halt(ctl);
            ctl.setup = None;
            ctl.state = RouterState::Off;
        });
    }

    pub fn pause(&self) {
        self.control(|_, ctl| {
            ctl.paused = true;
            if ctl.state == RouterState::Running {
                halt(ctl);
                ctl.state = RouterState::Paused;
            }
        });
    }

    pub fn resume(&self) {
        self.control(|inner, ctl| {
            ctl.paused = false;
            if ctl.state == RouterState::Paused
                && let Some(setup) = ctl.setup.clone()
            {
                let _ = launch(inner, ctl, &setup);
            }
        });
    }

    pub fn status(&self) -> RouterStatus {
        let (state, live) = self.0.snapshot.lock().unwrap().clone();
        RouterStatus {
            state,
            proxy: live.as_ref().map_or(ProxyState::Unknown, |l| {
                l.proxy_state.lock().unwrap().clone()
            }),
            totals: live
                .map(|l| l.stats.lock().unwrap().totals())
                .unwrap_or_default(),
            rules_error: self.0.rules.lock().unwrap().error(),
        }
    }

    pub fn rules(&self) -> RoutesView {
        self.0.refresh();
        let live = self.0.live();
        let file = self.0.rules.lock().unwrap();
        let traffic = |name: &str| {
            live.as_ref()
                .map(|l| l.stats.lock().unwrap().rule_traffic(name))
                .unwrap_or_default()
        };
        RoutesView {
            rules: file
                .list
                .iter()
                .map(|r| RuleRow {
                    rule: r.clone(),
                    traffic: traffic(&r.name),
                })
                .collect(),
            error: file.error(),
            system_doh: doh::system_doh(),
        }
    }

    pub fn flows(&self, all: bool) -> Vec<FlowRow> {
        let live = self.0.snapshot.lock().unwrap().1.clone();
        match live {
            Some(l) => l.stats.lock().unwrap().rows(Instant::now(), all),
            None => Vec::new(),
        }
    }

    /// The match table in force (what the capture uses).
    #[doc(hidden)]
    pub fn table(&self) -> Table {
        self.0.table.lock().unwrap().clone()
    }

    pub fn edit<T>(&self, f: impl FnOnce(&mut Vec<Rule>) -> Result<T>) -> Result<T> {
        // An outside change takes effect even if this edit then fails.
        self.0.refresh();
        let mut file = self.0.rules.lock().unwrap();
        if reload(&self.0.path, &mut file, false) {
            // Changed again since `refresh`: apply it too (not under the rules lock).
            let table = Table::new(&file.list);
            drop(file);
            self.0.apply(table);
            file = self.0.rules.lock().unwrap();
        }
        if let Some(e) = &file.file_error {
            bail!("routes.toml has an error, fix or delete it first: {e}");
        }
        let mut list = file.list.clone();
        let out = f(&mut list)?;
        for rule in &list {
            if let Some(e) = rules::check(rule).first() {
                bail!("{}: {}: {}", rule.name, e.field, e.message);
            }
        }
        rules::save(&self.0.path, &list)?;
        file.modified = modified(&self.0.path);
        file.list = list;
        let table = Table::new(&file.list);
        drop(file);
        self.0.apply(table);
        Ok(out)
    }
}

fn halt(ctl: &mut Ctl) {
    if let Some(live) = ctl.live.take() {
        live.shut();
    }
}

/// Counters start from zero: every launch is a new `Live`.
fn launch(inner: &Arc<Inner>, ctl: &mut Ctl, setup: &Setup) -> Result<()> {
    halt(ctl);
    let table = inner.table.lock().unwrap().clone();
    match Live::open(inner, setup, table) {
        Ok(live) => {
            ctl.live = Some(live);
            ctl.state = RouterState::Running;
            inner.rules.lock().unwrap().filter_error = None;
            Ok(())
        }
        Err(e) => {
            ctl.state = RouterState::Unavailable {
                reason: format!("{e:#}"),
            };
            Err(e)
        }
    }
}

impl Inner {
    fn notify(&self) {
        let f = self.on_change.lock().unwrap().clone();
        if let Some(f) = f {
            f();
        }
    }

    fn live(&self) -> Option<Arc<Live>> {
        self.ctl.lock().unwrap().live.clone()
    }

    fn refresh(&self) {
        let mut file = self.rules.lock().unwrap();
        if reload(&self.path, &mut file, false) {
            let table = Table::new(&file.list);
            drop(file);
            self.apply(table);
        }
    }

    fn apply(&self, table: Table) {
        *self.table.lock().unwrap() = table.clone();
        let Some(live) = self.live() else { return };
        let error = live.apply(Some(table)).err();
        self.rules.lock().unwrap().filter_error = error;
    }
}

struct Relay {
    _control: TcpStream,
    socket: UdpSocket,
    relay: SocketAddrV4,
    inbound: Address,
    local: SocketAddrV4,
    /// Named in each datagram's header instead of the address, when known.
    domain: Option<String>,
}

impl Relay {
    fn send(&self, dst: SocketAddrV4, payload: &[u8]) {
        let mut datagram = socks_header_to(dst, self.domain.as_deref());
        datagram.extend_from_slice(payload);
        let _ = self.socket.send_to(&datagram, self.relay);
    }
}

enum FlowState {
    Opening(Pending),
    Proxy(Arc<Relay>),
    Direct(DirectReason),
}

#[derive(Default)]
struct FilterState {
    text: Option<String>,
    opened: Option<Instant>,
    /// The last text WinDivert refused and the reason.
    failed: Option<(Option<String>, String)>,
}

/// SYNs to addresses the table has but the open capture filter may not (ruling R1): reflecting
/// one before the filter catches its connection's later segments would break the connection.
#[derive(Default)]
struct Waiting {
    /// The table of the filter in force.
    active: Arc<Table>,
    /// The held destinations (live NAT entries) of the filter in force.
    held: Vec<Ipv4Addr>,
    syns: Vec<(Vec<u8>, Address)>,
}

impl Waiting {
    /// The filter in force catches the connection's later segments.
    fn covers(&self, dst: SocketAddrV4) -> bool {
        self.active.matches(Protocol::Tcp, dst).is_some() || self.held.contains(dst.ip())
    }

    /// Puts a new filter in force and takes the held SYNs it covers (and unparsable ones, which
    /// go out unchanged); the others stay held for the next filter (ruling R1).
    fn activate(&mut self, active: Arc<Table>, held: Vec<Ipv4Addr>) -> Vec<(Vec<u8>, Address)> {
        self.active = active;
        self.held = held;
        let (go, stay) = std::mem::take(&mut self.syns)
            .into_iter()
            .partition(|(p, _)| parse_tcp(p).is_none_or(|s| self.covers(s.dst)));
        self.syns = stay;
        go
    }

    /// Drops the held SYNs of connections given up (source port, destination): a later filter
    /// must not replay them.
    fn forget(&mut self, gone: &[(u16, SocketAddrV4)]) {
        self.syns
            .retain(|(p, _)| parse_tcp(p).is_none_or(|s| !gone.contains(&(s.src.port(), s.dst))));
    }
}

/// One running session: handles, flows and counters; dropped whole on stop or pause.
struct Live {
    inner: Weak<Inner>,
    dll: Dll,
    proxy: SocketAddrV4,
    stopped: AtomicBool,
    /// The capture handle, also the one that injects.
    capture: RwLock<Option<Arc<Handle>>>,
    sockets: Mutex<Option<Arc<Handle>>>,
    filter: Mutex<FilterState>,
    /// The rules without resolved names; `table` is it with them.
    base: Mutex<Table>,
    table: RwLock<Arc<Table>>,
    names: Arc<Mutex<Names>>,
    images: Arc<Images>,
    waiting: Mutex<Waiting>,
    flows: Mutex<HashMap<FlowKey, FlowState>>,
    /// UDP ports; TCP ports have their own map, so a TCP socket event never replaces a UDP owner.
    owners: Owners,
    tcp_owners: Owners,
    nat: Mutex<tcp::Nat>,
    acceptor: TcpListener,
    acceptor_port: u16,
    dns: Mutex<Option<Arc<Handle>>>,
    /// Both sockets of each proxied TCP connection, shut down when its NAT entry or the session ends.
    conns: Mutex<HashMap<FlowKey, (TcpStream, TcpStream)>>,
    stats: Mutex<Stats>,
    proxy_state: Mutex<ProxyState>,
    proxy_pid: Mutex<Option<u32>>,
    next_id: AtomicU16,
    /// A spawned `recompose` has not started yet: more requests ride on it.
    recompose_pending: AtomicBool,
}

/// Caches the exe of every process with a UDP socket: a program started before the session
/// (or during a pause) sent no socket event the hub saw (ruling M1).
// ponytail: a table read and an OpenProcess per UDP-owning process every 2 s while a UDP program
// rule is idle; upgrade: look up only PIDs not yet in `Images`.
fn learn_udp_programs(images: &Images) {
    for pid in owner::udp_pids() {
        images.get(pid);
    }
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

impl Live {
    fn open(inner: &Arc<Inner>, setup: &Setup, table: Table) -> Result<Arc<Live>> {
        let dll = Dll::load(&setup.dll)?;
        let sockets = Arc::new(
            Handle::open(
                &dll,
                SOCKET_FILTER,
                Layer::Socket,
                FLAG_SNIFF | FLAG_RECV_ONLY,
            )
            .map_err(|e| anyhow!("WinDivert refused the socket handle: {e}"))?,
        );
        let dns = Arc::new(
            Handle::open(
                &dll,
                DNS_FILTER,
                Layer::Network,
                FLAG_SNIFF | FLAG_RECV_ONLY,
            )
            .map_err(|e| anyhow!("WinDivert refused the DNS handle: {e}"))?,
        );
        let acceptor = TcpListener::bind("0.0.0.0:0")
            .map_err(|e| anyhow!("cannot open the local TCP acceptor: {e}"))?;
        let acceptor_port = acceptor.local_addr()?.port();
        inner.names.lock().unwrap().expire(Instant::now(), &[]);
        if table.idle_apps() {
            learn_udp_programs(&inner.images);
        }
        let live = Arc::new(Live {
            inner: Arc::downgrade(inner),
            dll,
            proxy: setup.proxy,
            stopped: AtomicBool::new(false),
            capture: RwLock::new(None),
            sockets: Mutex::new(Some(sockets.clone())),
            filter: Mutex::new(FilterState::default()),
            base: Mutex::new(Table::default()),
            table: RwLock::new(Arc::new(Table::default())),
            names: inner.names.clone(),
            images: inner.images.clone(),
            waiting: Mutex::new(Waiting::default()),
            flows: Mutex::new(HashMap::new()),
            owners: Owners::default(),
            tcp_owners: Owners::default(),
            nat: Mutex::new(tcp::Nat::default()),
            acceptor,
            acceptor_port,
            dns: Mutex::new(Some(dns.clone())),
            conns: Mutex::new(HashMap::new()),
            stats: Mutex::new(Stats::default()),
            proxy_state: Mutex::new(ProxyState::Unknown),
            proxy_pid: Mutex::new(None),
            next_id: AtomicU16::new(0),
            recompose_pending: AtomicBool::new(false),
        });
        if let Err(e) = live.apply(Some(table)) {
            live.shut();
            bail!(e);
        }
        let l = live.clone();
        spawn(move || {
            while let Ok((_, addr)) = sockets.recv(&mut []) {
                let port = addr.socket_local_port();
                let owners = if addr.socket_protocol() == PROTO_TCP {
                    &l.tcp_owners
                } else {
                    &l.owners
                };
                if addr.event() == divert::EVENT_SOCKET_CLOSE {
                    owners.forget(port);
                    continue;
                }
                owners.record(port, addr.socket_pid());
                if addr.socket_protocol() == PROTO_TCP {
                    continue;
                }
                // A program of a UDP rule appeared: its UDP joins the filter.
                let table = l.table.read().unwrap().clone();
                if table.idle_apps()
                    && l.images
                        .get(addr.socket_pid())
                        .is_some_and(|exe| table.starts(&exe))
                {
                    l.recompose_soon();
                }
            }
        });
        let l = live.clone();
        spawn(move || l.dns_loop(&dns));
        let l = live.clone();
        spawn(move || l.accept_loop());
        let l = live.clone();
        spawn(move || l.housekeeping());
        let l = live.clone();
        spawn(move || l.check_proxy());
        Ok(live)
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Works after a panic poisoned the locks (`capture_loop`).
    fn shut(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        // Held SYNs go out unchanged while the capture can still send.
        let syns = std::mem::take(
            &mut self
                .waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .syns,
        );
        if let Some(h) = &*self.capture.read().unwrap_or_else(PoisonError::into_inner) {
            for (packet, addr) in &syns {
                let _ = h.send(packet, addr);
            }
        }
        if let Some(h) = self
            .dns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            h.shutdown();
        }
        // The redirected connections end (programs reconnect). Their NAT entries stay, so the
        // capture still reflects the acceptor's FIN to each client before it shuts down.
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
        if !conns.is_empty() {
            // ponytail: a fixed wait for the FINs to reach the capture; upgrade: wait until the
            // capture has reflected one per connection.
            std::thread::sleep(FIN_DRAIN);
        }
        // Wakes the acceptor, which then sees `stopped`.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.acceptor_port)),
            Duration::from_secs(1),
        );
        if let Some(h) = self
            .sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            h.shutdown();
        }
        if let Some(h) = self
            .capture
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            h.shutdown();
        }
        self.flows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn inject(&self, packet: &[u8], addr: &Address) {
        let handle = self.capture.read().unwrap().clone();
        if let Some(h) = handle {
            let _ = h.send(packet, addr);
        }
    }

    /// New rules (`None`: the same ones with the names known now) take effect at once; a changed
    /// filter opens a new handle (at most once a second, unless a SYN waits for it) before the old
    /// one stops receiving. No rules: a `false` filter, which captures nothing but still injects.
    /// A refused text is not retried until the filter changes; the error is the router's reason.
    fn apply(self: &Arc<Self>, base: Option<Table>) -> Result<(), String> {
        let mut filter = self.filter.lock().unwrap();
        // Cleared under the filter lock (one recompose waits behind a running apply at most) and
        // before the NAT is read below, so a SYN admitted after this point asks again.
        self.recompose_pending.store(false, Ordering::SeqCst);
        let base = match base {
            Some(b) => {
                *self.base.lock().unwrap() = b.clone();
                b
            }
            None => self.base.lock().unwrap().clone(),
        };
        let exes = self.images.paths();
        let exes: Vec<&str> = exes.iter().map(|e| &**e).collect();
        let table = base
            .with_names(&self.names.lock().unwrap().entries())
            .with_running(&exes);
        // Live connections keep their destination captured (spec 7), matched or not.
        let held: Vec<Ipv4Addr> = self
            .nat
            .lock()
            .unwrap()
            .destinations()
            .into_iter()
            .filter(|d| table.matches(Protocol::Tcp, *d).is_none())
            .map(|d| *d.ip())
            .collect();
        let text = divert::capture_filter(&table, Some(self.acceptor_port), &held);
        let table = Arc::new(table);
        *self.table.write().unwrap() = table.clone();
        if filter.opened.is_some() && text == filter.text {
            filter.failed = None;
            self.release(Some((table, held)));
            return Ok(());
        }
        if let Some((failed, reason)) = &filter.failed
            && *failed == text
        {
            self.release(None);
            return Err(reason.clone());
        }
        if let Some(at) = filter.opened {
            // A held SYN skips the gap: its program is waiting for this reopen.
            while at.elapsed() < REOPEN_GAP && self.waiting.lock().unwrap().syns.is_empty() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let new = match Handle::open(
            &self.dll,
            text.as_deref().unwrap_or("false"),
            Layer::Network,
            0,
        ) {
            Ok(h) => Arc::new(h),
            Err(e) => {
                let reason = divert::refused(&e);
                if divert::refused_for_good(&e) {
                    filter.failed = Some((text, reason.clone()));
                }
                self.release(None);
                return Err(reason);
            }
        };
        filter.text = text;
        filter.failed = None;
        filter.opened = Some(Instant::now());
        let (l, h) = (self.clone(), new.clone());
        spawn(move || l.capture_loop(&h));
        let old = {
            let mut cap = self.capture.write().unwrap();
            if self.is_stopped() {
                new.shutdown();
                return Ok(());
            }
            cap.replace(new)
        };
        if let Some(old) = old {
            old.shutdown();
        }
        self.release(Some((table, held)));
        Ok(())
    }

    /// Ends when its handle shuts down. Any other end (a panic, a recv error) while the handle is
    /// still the capture would black-hole matched traffic: the session fails instead.
    fn capture_loop(self: &Arc<Self>, handle: &Arc<Handle>) {
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut buf = vec![0u8; MAX_PACKET];
            loop {
                match handle.recv(&mut buf) {
                    Ok((n, addr)) => self.on_packet(handle, &buf[..n], addr),
                    Err(e) => return format!("WinDivert stopped capturing: {e}"),
                }
            }
        }));
        let reason = run.unwrap_or_else(|_| "the capture thread panicked".to_owned());
        let current = self
            .capture
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|h| Arc::ptr_eq(h, handle));
        if !current || self.is_stopped() {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            Router(inner).control(|_, ctl| {
                if ctl
                    .live
                    .as_ref()
                    .is_some_and(|l| std::ptr::eq(&**l, &**self))
                {
                    halt(ctl);
                    ctl.setup = None;
                    ctl.state = RouterState::Unavailable { reason };
                }
            });
        }
    }

    fn on_packet(self: &Arc<Self>, handle: &Handle, packet: &[u8], addr: Address) {
        if let Some(seg) = parse_tcp(packet) {
            return self.on_tcp(Some(handle), packet, &seg, addr);
        }
        // Stopped: the old handle drains its queue unchanged, no new flows open.
        let Some(udp) = parse_udp(packet)
            .filter(|u| !self.is_stopped() && packet::unicast(*u.src.ip(), *u.dst.ip()))
        else {
            let _ = handle.send(packet, &addr);
            return;
        };
        let table = self.table.read().unwrap().clone();
        if table.match_udp(udp.dst).is_none() && !table.wants_owner(Protocol::Udp, udp.dst.port()) {
            let _ = handle.send(packet, &addr);
            return;
        }
        let key = FlowKey {
            protocol: rules::Protocol::Udp,
            local_port: udp.src.port(),
            dst: udp.dst,
        };
        let up = udp.payload.len() as u64;
        if !self.flows.lock().unwrap().contains_key(&key) {
            // ponytail: waiting for the PID blocks capture up to 50 ms per new flow (new flows are
            // rare); upgrade: a deferred queue of first packets.
            let pid = self.owners.pid(key.local_port, PID_WAIT, |p| {
                owner::table_pid(Protocol::Udp, p)
            });
            let exe = pid.and_then(|p| self.images.get(p));
            let now = Instant::now();
            let Some(rule) = table.matches_flow(Protocol::Udp, udp.dst, exe.as_deref()) else {
                // Caught for a program rule, not its program: remembered, sent unchanged.
                let mut flows = self.flows.lock().unwrap();
                let new = match flows.entry(key) {
                    Entry::Vacant(e) => {
                        e.insert(FlowState::Direct(DirectReason::NoRule));
                        true
                    }
                    Entry::Occupied(_) => false,
                };
                drop(flows);
                let mut stats = self.stats.lock().unwrap();
                if new {
                    let reason = DirectReason::NoRule;
                    stats.open(key, "", Via::Direct { reason }, now);
                    if let Some(exe) = &exe {
                        stats.set_program(key, exe.to_string());
                    }
                }
                stats.add(key, up, 0, now);
                drop(stats);
                let _ = handle.send(packet, &addr);
                return;
            };
            let state = self.proxy_state.lock().unwrap().clone();
            let proxy_pid = *self.proxy_pid.lock().unwrap();
            let reason = flows::decide(&state, pid, std::process::id(), proxy_pid);
            let domain = self.domain_of(udp.dst, now);
            let mut flows = self.flows.lock().unwrap();
            if let Entry::Vacant(e) = flows.entry(key) {
                match reason {
                    Some(reason) => {
                        e.insert(FlowState::Direct(reason.clone()));
                        drop(flows);
                        let mut stats = self.stats.lock().unwrap();
                        stats.open(key, rule, Via::Direct { reason }, now);
                        if let Some(d) = domain {
                            stats.set_domain(key, d);
                        }
                        if let Some(exe) = &exe {
                            stats.set_program(key, exe.to_string());
                        }
                        stats.add(key, up, 0, now);
                        drop(stats);
                        let _ = handle.send(packet, &addr);
                    }
                    None => {
                        let mut pending = Pending::default();
                        pending.push(packet.to_vec(), addr);
                        e.insert(FlowState::Opening(pending));
                        let mut stats = self.stats.lock().unwrap();
                        stats.open(key, rule, Via::Opening, now);
                        if let Some(d) = domain.clone() {
                            stats.set_domain(key, d);
                        }
                        if let Some(exe) = &exe {
                            stats.set_program(key, exe.to_string());
                        }
                        drop(stats);
                        drop(flows);
                        let (l, local, inbound) = (self.clone(), udp.src, addr.as_inbound());
                        spawn(move || l.open_flow(key, local, inbound, domain));
                    }
                }
                return;
            }
        }
        let mut flows = self.flows.lock().unwrap();
        match flows.get_mut(&key) {
            Some(FlowState::Opening(pending)) => pending.push(packet.to_vec(), addr),
            Some(FlowState::Proxy(relay)) => {
                let relay = relay.clone();
                drop(flows);
                relay.send(udp.dst, udp.payload);
                self.stats.lock().unwrap().add(key, up, 0, Instant::now());
            }
            Some(FlowState::Direct(_)) | None => {
                drop(flows);
                let _ = handle.send(packet, &addr);
                self.stats.lock().unwrap().add(key, up, 0, Instant::now());
            }
        }
    }

    // ponytail: a control TCP that drops after the flow opened is not watched; proxied flows close
    // when two 10 s checks in a row see the proxy fail (`set_proxy`), so a dead proxy black-holes
    // up to 20 s; upgrade: watch `control` for EOF.
    fn open_flow(
        self: Arc<Self>,
        key: FlowKey,
        local: SocketAddrV4,
        inbound: Address,
        domain: Option<String>,
    ) {
        let started = Instant::now();
        let opened = socks::udp_associate(self.proxy, key.dst, OPEN_TIMEOUT).and_then(|assoc| {
            let socket = UdpSocket::bind("127.0.0.1:0")?;
            socket.set_read_timeout(Some(Duration::from_secs(1)))?;
            Ok(Arc::new(Relay {
                _control: assoc.control,
                socket,
                relay: assoc.relay,
                inbound,
                local,
                domain,
            }))
        });
        let latency_ms = started.elapsed().as_millis() as u32;
        let mut flows = self.flows.lock().unwrap();
        // Closed (idle or stopped) while opening: nothing to do.
        let Some(FlowState::Opening(pending)) = flows.get_mut(&key) else {
            return;
        };
        let pending = pending.drain();
        let mut up = 0u64;
        // Pending packets go out under the lock, so later packets cannot overtake them.
        let via = match opened {
            Ok(relay) => {
                for (packet, _) in &pending {
                    if let Some(udp) = parse_udp(packet) {
                        relay.send(udp.dst, udp.payload);
                        up += udp.payload.len() as u64;
                    }
                }
                flows.insert(key, FlowState::Proxy(relay.clone()));
                drop(flows);
                let l = self.clone();
                spawn(move || l.relay_rx(key, &relay));
                self.update_proxy(|current| {
                    (*current == ProxyState::NoUdp).then_some(ProxyState::Ok { latency_ms })
                });
                Via::Proxy
            }
            Err(e) => {
                for (packet, addr) in &pending {
                    self.inject(packet, addr);
                    up += parse_udp(packet).map_or(0, |u| u.payload.len() as u64);
                }
                let reason = DirectReason::from(&e);
                flows.insert(key, FlowState::Direct(reason.clone()));
                drop(flows);
                match e {
                    SocksError::AuthRequired => self.set_proxy(ProxyState::AuthRequired),
                    SocksError::Refused(7) => self.set_proxy(ProxyState::NoUdp),
                    _ => {}
                }
                Via::Direct { reason }
            }
        };
        let mut stats = self.stats.lock().unwrap();
        stats.set_via(key, via);
        stats.add(key, up, 0, Instant::now());
    }

    fn relay_rx(&self, key: FlowKey, relay: &Arc<Relay>) {
        let mut buf = vec![0u8; 10 + MAX_PAYLOAD];
        loop {
            let current = matches!(
                self.flows.lock().unwrap().get(&key),
                Some(FlowState::Proxy(r)) if Arc::ptr_eq(r, relay)
            );
            if !current || self.is_stopped() {
                return;
            }
            let (n, from) = match relay.socket.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => {
                    // An ICMP reset or an oversized datagram: skip it without spinning.
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            if from != SocketAddr::V4(relay.relay) {
                continue;
            }
            // A short domain header leaves more than MAX_PAYLOAD, which `build_udp` refuses.
            let Some((src, data)) = parse_socks_udp_from(&buf[..n], key.dst)
                .filter(|(_, data)| data.len() <= MAX_PAYLOAD)
            else {
                continue;
            };
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let mut packet = build_udp(src, relay.local, data, id);
            let mut addr = relay.inbound;
            self.dll.checksums(&mut packet, &mut addr);
            self.inject(&packet, &addr);
            self.stats
                .lock()
                .unwrap()
                .add(key, 0, data.len() as u64, Instant::now());
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
        if recovered {
            self.nat.lock().unwrap().clear_marks();
        }
        if failing || recovered {
            // Their next packets open new flows: direct while it fails, through it once it is
            // back (spec 4.2, 6).
            let closing: Vec<FlowKey> = self
                .flows
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, f)| match f {
                    FlowState::Proxy(_) => failing,
                    FlowState::Direct(reason) => recovered && reason.proxy_failure(),
                    FlowState::Opening(_) => false,
                })
                .map(|(k, _)| *k)
                .collect();
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

    /// Every tick: close idle flows, drop old closed rows, expire names and NAT entries; every
    /// 2 s: forget exited programs, look for a UDP rule's program not yet seen, re-read the rules.
    fn housekeeping(self: &Arc<Self>) {
        let mut tick = 0u32;
        while self.sleep(TICK) {
            tick += 1;
            let now = Instant::now();
            let idle = self.stats.lock().unwrap().idle(now);
            for key in idle {
                self.close_flow(key, now);
            }
            self.stats.lock().unwrap().prune(now);
            self.tcp_tick(now);
            if tick.is_multiple_of(RULES_CHECK_TICKS) {
                // Exited programs leave; the next recompose drops their UDP part.
                self.images.prune(owner::process_start);
                if self.table.read().unwrap().idle_apps() {
                    learn_udp_programs(&self.images);
                }
                match self.inner.upgrade() {
                    Some(inner) => inner.refresh(),
                    None => return,
                }
            }
        }
    }

    /// Drops the relay (control TCP and socket) and forgets the port's owner, unless the flow was
    /// a loop (the hub's or the proxy's own socket must stay known) or another flow uses the port.
    fn close_flow(&self, key: FlowKey, now: Instant) {
        let mut flows = self.flows.lock().unwrap();
        let removed = flows.remove(&key);
        let port_busy = flows.keys().any(|k| k.local_port == key.local_port);
        drop(flows);
        let was_loop = matches!(removed, Some(FlowState::Direct(DirectReason::Loop)));
        if !was_loop && !port_busy {
            self.owners.forget(key.local_port);
        }
        self.stats.lock().unwrap().close(key, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syn_to(dst: SocketAddrV4) -> (Vec<u8>, Address) {
        let mut p = vec![
            0x45, 0, 0, 40, 0, 1, 0x40, 0, 128, 6, 0, 0, 192, 168, 1, 5, 0, 0, 0, 0,
        ];
        p[16..20].copy_from_slice(&dst.ip().octets());
        p.extend_from_slice(&[0xC3, 0x50]);
        p.extend_from_slice(&dst.port().to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0x50, 0x02, 0xFA, 0xF0, 0, 0, 0, 0]);
        (p, Address([0; 80]))
    }

    #[test]
    fn a_new_filter_takes_only_the_held_syns_it_covers() {
        let ruled: SocketAddrV4 = "1.1.1.1:443".parse().unwrap();
        let held: SocketAddrV4 = "8.8.8.8:443".parse().unwrap();
        let later: SocketAddrV4 = "9.9.9.9:443".parse().unwrap();
        let table = Table::new(&[Rule {
            name: "one".into(),
            enabled: true,
            protocol: Protocol::Tcp,
            ports: vec![],
            ips: vec!["1.1.1.1/32".into()],
            domains: vec![],
            apps: vec![],
        }]);
        let mut w = Waiting {
            syns: vec![
                syn_to(ruled),
                syn_to(held),
                syn_to(later),
                (vec![1, 2, 3], Address([0; 80])),
            ],
            ..Waiting::default()
        };
        let go = w.activate(Arc::new(table), vec![*held.ip()]);
        let dst = |s: &(Vec<u8>, Address)| parse_tcp(&s.0).map(|t| t.dst);
        assert_eq!(
            go.iter().map(dst).collect::<Vec<_>>(),
            [Some(ruled), Some(held), None],
            "covered and unparsable go"
        );
        assert_eq!(w.syns.iter().map(dst).collect::<Vec<_>>(), [Some(later)]);
        assert!(w.covers(held) && !w.covers(later));
    }

    #[test]
    fn forgetting_a_connection_drops_only_its_held_syn() {
        let a: SocketAddrV4 = "1.1.1.1:443".parse().unwrap();
        let b: SocketAddrV4 = "2.2.2.2:443".parse().unwrap();
        let mut w = Waiting {
            syns: vec![syn_to(a), syn_to(b), (vec![1, 2, 3], Address([0; 80]))],
            ..Waiting::default()
        };
        w.forget(&[(50000, a)]);
        let dst = |s: &(Vec<u8>, Address)| parse_tcp(&s.0).map(|t| t.dst);
        assert_eq!(w.syns.iter().map(dst).collect::<Vec<_>>(), [Some(b), None]);
    }
}

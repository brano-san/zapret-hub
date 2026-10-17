pub mod divert;
pub mod dns;
pub mod flows;
pub mod owner;
pub mod packet;
pub mod rules;
pub mod socks;
pub mod tcp;

use std::collections::{HashMap, hash_map::Entry};
use std::mem::discriminant;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use divert::{Address, Dll, FLAG_RECV_ONLY, FLAG_SNIFF, Handle, Layer, MAX_PACKET, SOCKET_FILTER};
use flows::{DirectReason, FlowKey, FlowRow, Pending, ProxyState, Stats, Totals, Traffic, Via};
use owner::{Owners, PID_WAIT};
use packet::{build_udp, parse_socks_udp, parse_udp, socks_header};
use rules::{Rule, Table};
use socks::{OPEN_TIMEOUT, SocksError};

const PROXY_CHECK: Duration = Duration::from_secs(10);
const RULES_CHECK_TICKS: u32 = 2;
const REOPEN_GAP: Duration = Duration::from_secs(1);
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
        let error = live
            .apply(table)
            .err()
            .map(|e| format!("WinDivert refused the filter: {e}"));
        self.rules.lock().unwrap().filter_error = error;
    }
}

struct Relay {
    _control: std::net::TcpStream,
    socket: UdpSocket,
    relay: SocketAddrV4,
    inbound: Address,
    local: SocketAddrV4,
}

impl Relay {
    fn send(&self, dst: SocketAddrV4, payload: &[u8]) {
        let mut datagram = socks_header(dst).to_vec();
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
    table: RwLock<Arc<Table>>,
    flows: Mutex<HashMap<FlowKey, FlowState>>,
    owners: Owners,
    stats: Mutex<Stats>,
    proxy_state: Mutex<ProxyState>,
    proxy_pid: Mutex<Option<u32>>,
    next_id: AtomicU16,
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
        let live = Arc::new(Live {
            inner: Arc::downgrade(inner),
            dll,
            proxy: setup.proxy,
            stopped: AtomicBool::new(false),
            capture: RwLock::new(None),
            sockets: Mutex::new(Some(sockets.clone())),
            filter: Mutex::new(FilterState::default()),
            table: RwLock::new(Arc::new(Table::default())),
            flows: Mutex::new(HashMap::new()),
            owners: Owners::default(),
            stats: Mutex::new(Stats::default()),
            proxy_state: Mutex::new(ProxyState::Unknown),
            proxy_pid: Mutex::new(None),
            next_id: AtomicU16::new(0),
        });
        if let Err(e) = live.apply(table) {
            live.shut();
            bail!("WinDivert refused the filter: {e}");
        }
        let l = live.clone();
        spawn(move || {
            while let Ok((_, addr)) = sockets.recv(&mut []) {
                let port = addr.socket_local_port();
                if addr.event() == divert::EVENT_SOCKET_CLOSE {
                    l.owners.forget(port);
                } else {
                    l.owners.record(port, addr.socket_pid());
                }
            }
        });
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

    /// New table now; a changed filter opens a new handle (at most once a second) before the old
    /// one stops receiving. No rules: a `false` filter, which captures nothing but still injects.
    fn apply(self: &Arc<Self>, table: Table) -> std::io::Result<()> {
        let mut filter = self.filter.lock().unwrap();
        let text = divert::filter(&table);
        *self.table.write().unwrap() = Arc::new(table);
        if filter.opened.is_some() && text == filter.text {
            return Ok(());
        }
        if let Some(at) = filter.opened {
            std::thread::sleep(REOPEN_GAP.saturating_sub(at.elapsed()));
        }
        let new = Arc::new(Handle::open(
            &self.dll,
            text.as_deref().unwrap_or("false"),
            Layer::Network,
            0,
        )?);
        filter.text = text;
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
        // Stopped: the old handle drains its queue unchanged, no new flows open.
        let Some(udp) = parse_udp(packet)
            .filter(|u| !self.is_stopped() && packet::unicast(*u.src.ip(), *u.dst.ip()))
        else {
            let _ = handle.send(packet, &addr);
            return;
        };
        let table = self.table.read().unwrap().clone();
        let Some(rule) = table.match_udp(udp.dst) else {
            let _ = handle.send(packet, &addr);
            return;
        };
        let key = FlowKey {
            protocol: rules::Protocol::Udp,
            local_port: udp.src.port(),
            dst: udp.dst,
        };
        let up = udp.payload.len() as u64;
        if !self.flows.lock().unwrap().contains_key(&key) {
            // ponytail: waiting for the PID blocks capture up to 50 ms per new flow (new flows are
            // rare); upgrade: a deferred queue of first packets.
            let pid = self.owners.wait(key.local_port, PID_WAIT);
            let state = self.proxy_state.lock().unwrap().clone();
            let proxy_pid = *self.proxy_pid.lock().unwrap();
            let reason = flows::decide(&state, pid, std::process::id(), proxy_pid);
            let mut flows = self.flows.lock().unwrap();
            if let Entry::Vacant(e) = flows.entry(key) {
                let now = Instant::now();
                match reason {
                    Some(reason) => {
                        e.insert(FlowState::Direct(reason.clone()));
                        drop(flows);
                        let mut stats = self.stats.lock().unwrap();
                        stats.open(key, rule, Via::Direct { reason }, now);
                        stats.add(key, up, 0, now);
                        drop(stats);
                        let _ = handle.send(packet, &addr);
                    }
                    None => {
                        let mut pending = Pending::default();
                        pending.push(packet.to_vec(), addr);
                        e.insert(FlowState::Opening(pending));
                        self.stats
                            .lock()
                            .unwrap()
                            .open(key, rule, Via::Opening, now);
                        drop(flows);
                        let (l, local, inbound) = (self.clone(), udp.src, addr.as_inbound());
                        spawn(move || l.open_flow(key, local, inbound));
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
    fn open_flow(self: Arc<Self>, key: FlowKey, local: SocketAddrV4, inbound: Address) {
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
            // The buffer caps `data` at MAX_PAYLOAD, so `build_udp` never panics.
            let Some((src, data)) = parse_socks_udp(&buf[..n]) else {
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

    /// Every second: close idle flows and drop old closed rows; every 2 s: re-read the rules.
    fn housekeeping(&self) {
        let mut tick = 0u32;
        while self.sleep(Duration::from_secs(1)) {
            tick += 1;
            let now = Instant::now();
            let idle = self.stats.lock().unwrap().idle(now);
            for key in idle {
                self.close_flow(key, now);
            }
            self.stats.lock().unwrap().rows(now, true);
            if tick.is_multiple_of(RULES_CHECK_TICKS) {
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

pub mod divert;
pub mod dns;
pub mod doh;
pub mod flows;
pub mod linux;
#[cfg(windows)]
mod live;
#[cfg(target_os = "linux")]
use linux::live;
pub mod observe;
pub mod owner;
pub mod packet;
pub mod rules;
pub mod socks;
pub mod tcp;

/// The name of the diagnostics' temporary rule (`Router::set_temporary`): no saved rule takes it.
pub const TEMPORARY: &str = "diag-check";

use std::net::SocketAddrV4;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use dns::Names;
use flows::{FlowRow, ProxyState, Totals, Traffic};
use live::Live;
use owner::Images;
use rules::{Rule, Table};

/// Routing needs WinDivert or nftables: elsewhere a session never starts.
#[cfg(not(any(windows, target_os = "linux")))]
mod live {
    use std::sync::{Arc, MutexGuard};

    use anyhow::{Result, bail};

    use crate::flows::{ProxyState, Stats};
    use crate::rules::Table;
    use crate::{Inner, Setup};

    pub(super) enum Live {}

    impl Live {
        pub(super) fn open(_: &Arc<Inner>, _: &Setup, _: Table) -> Result<Arc<Live>> {
            bail!("routing is not supported on this system")
        }

        pub(super) fn shut(&self) {
            match *self {}
        }

        pub(super) fn apply(&self, _: Option<Table>) -> Result<(), String> {
            match *self {}
        }

        pub(super) fn proxy(&self) -> ProxyState {
            match *self {}
        }

        pub(super) fn stats(&self) -> MutexGuard<'_, Stats> {
            match *self {}
        }
    }
}

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
    /// A rule held in memory only (a diagnostics run): on top of the file rules, never saved.
    temporary: Mutex<Option<Rule>>,
    /// The table in force; a launch starts from it.
    table: Mutex<Table>,
    /// Tables are numbered as built (under the rules lock) ...
    built: AtomicU64,
    /// ... and one built before the last applied is never applied: two edits racing past the
    /// rules lock cannot leave the older table in force.
    applied: Mutex<u64>,
    /// Outlives sessions: programs keep resolved addresses cached across a pause.
    #[cfg_attr(
        not(any(windows, target_os = "linux")),
        expect(dead_code, reason = "only a routing session reads it")
    )]
    names: Arc<Mutex<Names>>,
    /// Outlives sessions too: a program seen before a pause still counts as running.
    #[cfg_attr(
        not(any(windows, target_os = "linux")),
        expect(dead_code, reason = "only a routing session reads it")
    )]
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
        Router(Arc::new(Inner {
            table: Mutex::new(Table::new(&file.list)),
            built: AtomicU64::new(0),
            applied: Mutex::new(0),
            temporary: Mutex::new(None),
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
        self.control(|inner, ctl| {
            halt(ctl);
            ctl.setup = None;
            ctl.state = RouterState::Off;
            if inner.temporary.lock().unwrap().take().is_some() {
                let (_, table) = inner.table_of(&inner.rules.lock().unwrap().list);
                *inner.table.lock().unwrap() = table;
            }
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
            proxy: live.as_ref().map_or(ProxyState::Unknown, |l| l.proxy()),
            totals: live.map(|l| l.stats().totals()).unwrap_or_default(),
            rules_error: self.0.rules.lock().unwrap().error(),
        }
    }

    pub fn rules(&self) -> RoutesView {
        self.0.refresh();
        let live = self.0.live();
        let file = self.0.rules.lock().unwrap();
        let traffic = |name: &str| {
            live.as_ref()
                .map(|l| l.stats().rule_traffic(name))
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
                .chain(self.0.temporary.lock().unwrap().iter().map(|r| RuleRow {
                    rule: r.clone(),
                    traffic: traffic(&r.name),
                }))
                .collect(),
            error: file.error(),
            system_doh: doh::system_doh(),
        }
    }

    /// Puts a rule on top of the file rules (`None` takes it off) without saving it; a running
    /// session routes it at once. `stop` drops it, `pause` keeps it.
    pub fn set_temporary(&self, rule: Option<Rule>) -> Result<()> {
        self.0.refresh();
        let file = self.0.rules.lock().unwrap();
        if let Some(rule) = &rule {
            if let Some(e) = rules::check(rule).first() {
                bail!("{}: {}: {}", rule.name, e.field, e.message);
            }
            if file.list.iter().any(|r| r.name == rule.name) {
                bail!("a rule named {} already exists", rule.name);
            }
        }
        *self.0.temporary.lock().unwrap() = rule;
        let table = self.0.table_of(&file.list);
        drop(file);
        self.0.apply(table);
        Ok(())
    }

    pub fn temporary(&self) -> Option<Rule> {
        self.0.temporary.lock().unwrap().clone()
    }

    pub fn flows(&self, all: bool) -> Vec<FlowRow> {
        let live = self.0.snapshot.lock().unwrap().1.clone();
        match live {
            Some(l) => l.stats().rows(Instant::now(), all),
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
            let table = self.0.table_of(&file.list);
            drop(file);
            self.0.apply(table);
            file = self.0.rules.lock().unwrap();
        }
        if let Some(e) = &file.file_error {
            bail!("routes.toml has an error, fix or delete it first: {e}");
        }
        let mut list = file.list.clone();
        let out = f(&mut list)?;
        let temporary = self
            .0
            .temporary
            .lock()
            .unwrap()
            .as_ref()
            .map(|t| t.name.to_lowercase());
        let reserved = |name: &str| {
            let name = name.to_lowercase();
            name == TEMPORARY || Some(&name) == temporary.as_ref()
        };
        // A saved rule of that name stays editable; a new one may not take it.
        if let Some(rule) = list
            .iter()
            .find(|r| reserved(&r.name) && !file.list.iter().any(|o| o.name == r.name))
        {
            bail!(
                "{}: the name is reserved for the diagnostics' temporary rule",
                rule.name
            );
        }
        for rule in &list {
            if let Some(e) = rules::check(rule).first() {
                bail!("{}: {}: {}", rule.name, e.field, e.message);
            }
        }
        rules::save(&self.0.path, &list)?;
        file.modified = modified(&self.0.path);
        file.list = list;
        let table = self.0.table_of(&file.list);
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
            let table = self.table_of(&file.list);
            drop(file);
            self.apply(table);
        }
    }

    /// The file rules with the temporary one on top, and the build's number (`apply`).
    fn table_of(&self, list: &[Rule]) -> (u64, Table) {
        let mut list = list.to_vec();
        list.extend(self.temporary.lock().unwrap().clone());
        (
            self.built.fetch_add(1, Ordering::SeqCst) + 1,
            Table::new(&list),
        )
    }

    fn apply(&self, (number, table): (u64, Table)) {
        let mut applied = self.applied.lock().unwrap();
        if number < *applied {
            return;
        }
        *applied = number;
        *self.table.lock().unwrap() = table.clone();
        let Some(live) = self.live() else { return };
        let error = live.apply(Some(table)).err();
        self.rules.lock().unwrap().filter_error = error;
    }
}

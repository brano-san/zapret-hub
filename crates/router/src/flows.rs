//! Per-flow decisions, the queue of packets waiting for the proxy, and counters.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::divert::Address;
use crate::rules::Protocol;
use crate::socks::SocksError;

pub const QUEUE: usize = 64;
pub const IDLE: Duration = Duration::from_secs(30);
pub const KEEP_CLOSED: Duration = Duration::from_secs(30);
/// Most rule-less rows listed with `all`.
pub const MAX_UNRULED: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state")]
pub enum ProxyState {
    Unknown,
    Ok { latency_ms: u32 },
    Down { error: String },
    AuthRequired,
    NoUdp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum DirectReason {
    ProxyDown,
    AuthRequired,
    NoUdp,
    Timeout,
    Failed {
        error: String,
    },
    Loop,
    /// The address is sent direct for a while after the proxy refused a connection to it.
    Marked,
    /// The SYN never reached the local acceptor.
    NotAccepted,
    /// Caught for a program rule, but this flow matches no rule.
    NoRule,
}

impl From<&SocksError> for DirectReason {
    fn from(err: &SocksError) -> Self {
        match err {
            SocksError::Timeout => Self::Timeout,
            SocksError::AuthRequired => Self::AuthRequired,
            SocksError::Refused(7) => Self::NoUdp,
            _ => Self::Failed {
                error: err.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "via")]
pub enum Via {
    Opening,
    Proxy,
    Direct { reason: DirectReason },
}

/// Bytes are `f64`: specta forbids u64 in TypeScript.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Traffic {
    pub flows: u32,
    pub up: f64,
    pub down: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Totals {
    pub proxied: Traffic,
    pub direct: Traffic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FlowRow {
    pub protocol: String,
    pub domain: Option<String>,
    /// Full path of the program's exe, when known.
    pub program: Option<String>,
    pub destination: String,
    pub local_port: u16,
    pub rule: Option<String>,
    pub via: Via,
    pub up: f64,
    pub down: f64,
    pub seconds: u32,
    pub closed: bool,
}

/// Packets of a flow waiting for the proxy; the oldest go first when it overflows.
#[derive(Default)]
pub struct Pending(VecDeque<(Vec<u8>, Address)>);

impl Pending {
    pub fn push(&mut self, packet: Vec<u8>, addr: Address) {
        if self.0.len() == QUEUE {
            self.0.pop_front();
        }
        self.0.push_back((packet, addr));
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn drain(&mut self) -> Vec<(Vec<u8>, Address)> {
        self.0.drain(..).collect()
    }
}

/// `None` = through the proxy. UDP refusal (`NoUdp`) is learned from `UDP ASSOCIATE` itself.
pub fn decide(
    proxy: &ProxyState,
    pid: Option<u32>,
    own: u32,
    proxy_pid: Option<u32>,
) -> Option<DirectReason> {
    if pid.is_some_and(|p| p == own || Some(p) == proxy_pid) {
        return Some(DirectReason::Loop);
    }
    match proxy {
        ProxyState::Down { .. } => Some(DirectReason::ProxyDown),
        ProxyState::AuthRequired => Some(DirectReason::AuthRequired),
        ProxyState::Unknown | ProxyState::Ok { .. } | ProxyState::NoUdp => None,
    }
}

impl DirectReason {
    /// Sent direct by a proxy failure: such a flow re-opens through the proxy once it is back.
    pub fn proxy_failure(&self) -> bool {
        matches!(self, Self::ProxyDown | Self::AuthRequired | Self::NoUdp)
    }
}

/// The state a proxy check sets, `None` = keep the current one. A working proxy goes `Down` only
/// after two failed checks in a row (one lost handshake must not drop a live call); `NoUdp` stays
/// until an associate succeeds.
pub fn checked(current: &ProxyState, new: ProxyState, failures: &mut u32) -> Option<ProxyState> {
    let down = matches!(new, ProxyState::Down { .. });
    *failures = if down { failures.saturating_add(1) } else { 0 };
    match (current, &new) {
        (ProxyState::Ok { .. }, ProxyState::Down { .. }) if *failures < 2 => None,
        (ProxyState::NoUdp, ProxyState::Ok { .. }) => None,
        _ => Some(new),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub protocol: Protocol,
    pub local_port: u16,
    pub dst: SocketAddrV4,
}

struct Flow {
    rule: String,
    domain: Option<String>,
    program: Option<String>,
    via: Via,
    up: u64,
    down: u64,
    started: Instant,
    last: Instant,
    closed: Option<Instant>,
}

impl Flow {
    fn proxied(&self) -> bool {
        !matches!(self.via, Via::Direct { .. })
    }
}

#[derive(Default)]
pub struct Stats(HashMap<FlowKey, Flow>);

impl Stats {
    pub fn open(&mut self, key: FlowKey, rule: &str, via: Via, now: Instant) {
        let flow = Flow {
            rule: rule.to_owned(),
            domain: None,
            program: None,
            via,
            up: 0,
            down: 0,
            started: now,
            last: now,
            closed: None,
        };
        self.0.insert(key, flow);
    }

    pub fn set_via(&mut self, key: FlowKey, via: Via) {
        if let Some(f) = self.0.get_mut(&key) {
            f.via = via;
        }
    }

    pub fn set_domain(&mut self, key: FlowKey, domain: String) {
        if let Some(f) = self.0.get_mut(&key) {
            f.domain = Some(domain);
        }
    }

    pub fn set_program(&mut self, key: FlowKey, program: String) {
        if let Some(f) = self.0.get_mut(&key) {
            f.program = Some(program);
        }
    }

    pub fn add(&mut self, key: FlowKey, up: u64, down: u64, now: Instant) {
        if let Some(f) = self.0.get_mut(&key) {
            f.up += up;
            f.down += down;
            f.last = now;
        }
    }

    pub fn close(&mut self, key: FlowKey, now: Instant) {
        if let Some(f) = self.0.get_mut(&key) {
            f.closed.get_or_insert(now);
        }
    }

    /// Open UDP flows without packets for `IDLE` or more; TCP ends with its connection.
    pub fn idle(&self, now: Instant) -> Vec<FlowKey> {
        let quiet = |f: &Flow| f.closed.is_none() && now.saturating_duration_since(f.last) >= IDLE;
        self.0
            .iter()
            .filter(|(k, f)| k.protocol == Protocol::Udp && quiet(f))
            .map(|(k, _)| *k)
            .collect()
    }

    /// Drops the flows closed `KEEP_CLOSED` ago or more.
    pub fn prune(&mut self, now: Instant) {
        self.0.retain(|_, f| {
            f.closed
                .is_none_or(|c| now.saturating_duration_since(c) < KEEP_CLOSED)
        });
    }

    /// Open flows and those closed less than `KEEP_CLOSED` ago; older closed ones are dropped.
    /// Rule-less flows are listed only with `all`, the oldest beyond `MAX_UNRULED` left out.
    pub fn rows(&mut self, now: Instant, all: bool) -> Vec<FlowRow> {
        self.prune(now);
        let mut rows: Vec<(Instant, FlowRow)> = self
            .0
            .iter()
            .filter(|(_, f)| all || !f.rule.is_empty())
            .map(|(k, f)| {
                let end = f.closed.unwrap_or(now);
                let row = FlowRow {
                    protocol: match k.protocol {
                        Protocol::Tcp => "tcp",
                        Protocol::Udp | Protocol::Both => "udp",
                    }
                    .into(),
                    domain: f.domain.clone(),
                    program: f.program.clone(),
                    destination: k.dst.to_string(),
                    local_port: k.local_port,
                    rule: Some(f.rule.clone()).filter(|r| !r.is_empty()),
                    via: f.via.clone(),
                    up: f.up as f64,
                    down: f.down as f64,
                    seconds: end.saturating_duration_since(f.started).as_secs() as u32,
                    closed: f.closed.is_some(),
                };
                (f.started, row)
            })
            .collect();
        rows.sort_by_key(|(started, r)| (*started, r.local_port));
        let mut excess = rows
            .iter()
            .filter(|(_, r)| r.rule.is_none())
            .count()
            .saturating_sub(MAX_UNRULED);
        rows.retain(|(_, r)| {
            let drop = r.rule.is_none() && excess > 0;
            excess -= drop as usize;
            !drop
        });
        rows.into_iter().map(|(_, r)| r).collect()
    }

    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for f in self.0.values().filter(|f| !f.rule.is_empty()) {
            let side = if f.proxied() {
                &mut t.proxied
            } else {
                &mut t.direct
            };
            side.flows += 1;
            side.up += f.up as f64;
            side.down += f.down as f64;
        }
        t
    }

    pub fn rule_traffic(&self, rule: &str) -> Traffic {
        let mut t = Traffic::default();
        for f in self.0.values().filter(|f| f.rule == rule) {
            t.flows += 1;
            t.up += f.up as f64;
            t.down += f.down as f64;
        }
        t
    }
}

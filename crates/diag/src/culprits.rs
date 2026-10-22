//! Which observed flows look broken, and their grouping (spec section 5).

use std::net::SocketAddr;

use router::observe::FlowCounters;
use router::rules::Protocol;
use serde::{Deserialize, Serialize};

/// `ResetEarly` is the default only for reading a report that lacks the fault.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum Fault {
    SynUnanswered {
        syns: u32,
    },
    #[default]
    ResetEarly,
    RedirectEarly,
    DataRetrans {
        count: u32,
    },
    UdpNoReply {
        up: u32,
        down: u32,
    },
    Slow {
        median_ms: u32,
    },
    Lossy {
        loss_pct: u32,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Group {
    /// Domain, SNI, or `a.b.c.0/24`.
    pub key: String,
    pub protocol: Protocol,
    pub ports: Vec<u16>,
    pub members: Vec<FlowCounters>,
    /// Up to 3, most packets sent first.
    pub representatives: Vec<FlowCounters>,
    pub fault: Fault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub enum Verdict {
    NoFlows,
    Clean,
    Culprits(Vec<Group>),
}

/// The spec's "3 SYN retries" read as 3 SYNs in all: Windows sends a SYN and 2 retries.
pub fn fault(f: &FlowCounters) -> Option<Fault> {
    // IPv6 and system-proxy flows are handled separately.
    if f.ipv6 || f.via_system_proxy {
        return None;
    }
    if f.syns >= 3 && !f.syn_answered {
        return Some(Fault::SynUnanswered { syns: f.syns });
    }
    if f.rst_early {
        return Some(Fault::ResetEarly);
    }
    if f.redirect_early {
        return Some(Fault::RedirectEarly);
    }
    if f.data_retrans >= 3 {
        return Some(Fault::DataRetrans {
            count: f.data_retrans,
        });
    }
    if matches!(f.protocol, Protocol::Udp) && f.up_packets >= 4 && f.down_packets <= 1 {
        return Some(Fault::UdpNoReply {
            up: f.up_packets,
            down: f.down_packets,
        });
    }
    let mut rtt = f.rtt_ms.clone();
    rtt.sort_unstable();
    // Even count: the lower middle.
    if let Some(&median_ms) = rtt.get(rtt.len().saturating_sub(1) / 2)
        && median_ms > 250
    {
        return Some(Fault::Slow { median_ms });
    }
    if f.protocol == Protocol::Udp
        && let Some(loss_pct) = (f.udp_silent_windows * 100).checked_div(f.udp_windows)
        && loss_pct > 20
    {
        return Some(Fault::Lossy { loss_pct });
    }
    None
}

fn key_of(f: &FlowCounters) -> String {
    if let Some(name) = f.domain.as_ref().or(f.sni.as_ref()) {
        return name.clone();
    }
    match f.dst.parse::<SocketAddr>() {
        Ok(SocketAddr::V4(a)) => {
            let [x, y, z, _] = a.ip().octets();
            format!("{x}.{y}.{z}.0/24")
        }
        _ => f.dst.clone(),
    }
}

fn port_of(f: &FlowCounters) -> Option<u16> {
    f.dst.rsplit_once(':')?.1.parse().ok()
}

/// Faulty flows only, by name or /24 (and protocol), in order of first appearance.
pub fn group(flows: &[FlowCounters]) -> Vec<Group> {
    let mut groups: Vec<Group> = vec![];
    for (f, fault) in flows.iter().filter_map(|f| Some((f, fault(f)?))) {
        let key = key_of(f);
        match groups
            .iter_mut()
            .find(|g| g.key == key && g.protocol == f.protocol)
        {
            Some(g) => g.members.push(f.clone()),
            None => groups.push(Group {
                key,
                protocol: f.protocol,
                ports: vec![],
                members: vec![f.clone()],
                representatives: vec![],
                // The first representative's, below.
                fault,
            }),
        }
    }
    for g in &mut groups {
        let mut by_up: Vec<&FlowCounters> = g.members.iter().collect();
        by_up.sort_by_key(|f| std::cmp::Reverse(f.up_packets)); // stable
        g.representatives = by_up.into_iter().take(3).cloned().collect();
        if let Some(f) = fault(&g.representatives[0]) {
            g.fault = f;
        }
        g.ports = g.members.iter().filter_map(port_of).collect();
        g.ports.sort_unstable();
        g.ports.dedup();
    }
    groups
}

pub fn verdict(flows: &[FlowCounters]) -> Verdict {
    if flows.is_empty() {
        return Verdict::NoFlows;
    }
    match group(flows) {
        g if g.is_empty() => Verdict::Clean,
        g => Verdict::Culprits(g),
    }
}

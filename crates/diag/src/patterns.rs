//! Failure patterns R1–R6 from the live analysis (spec 16): all that fit, not the first one.

use std::net::SocketAddr;

use router::observe::FlowCounters;
use router::rules::Protocol;
use serde::{Deserialize, Serialize};

use crate::evaluate::works;
use crate::logs::discord::{self, Voice};
use crate::logs::neko::{self, Conn, Route};
use crate::logs::neko_routes::Routes;
use crate::paths::{Ctx, PathKind};
use crate::run::Outcome;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "id")]
pub enum Pattern {
    R1RuleMissesProtocol {
        rule: String,
        has: Protocol,
        needs: Protocol,
    },
    R2VoiceUdp {
        server: String,
        via_vpn: bool,
    },
    R3SlowProxyDns {
        median_ms: u32,
        direct_dns: String,
    },
    R4DnsSpoofed {
        domain: String,
    },
    R5Whitelist,
    R6DaveStalled {
        alone: bool,
        /// Where NekoBox sent the voice control channel (`*.discord.media`).
        control_route: Option<String>,
        /// The control channel went direct (not through the VPN).
        direct: bool,
    },
}

/// What was seen besides the paths; a missing source is `None`.
#[derive(Clone, Debug, Default)]
pub struct Evidence {
    pub flows: Vec<FlowCounters>,
    pub neko: Option<Vec<Conn>>,
    pub neko_routes: Option<Routes>,
    pub voice: Option<Voice>,
    /// The proxy's external address, from a probe through it.
    pub proxy_ip: Option<String>,
    /// Domains whose UDP 53 and DoH answers differ.
    pub dns_mismatch: Vec<String>,
    /// `https://ya.ru` works.
    pub ru_ok: Option<bool>,
    /// Foreign addresses do not answer a SYN on any path.
    pub foreign_syn_dead: Option<bool>,
}

/// Cloudflare voice (`104.29.0.0/16`) or GCP screen share (`35.217.0.0/16`).
fn discord_media(f: &FlowCounters) -> bool {
    matches!(f.dst.parse(), Ok(SocketAddr::V4(a)) if matches!(a.ip().octets(), [104, 29, ..] | [35, 217, ..]))
}

fn route_text(r: &Route) -> String {
    match r {
        Route::Direct => "напрямую".into(),
        Route::Proxy => "через VPN".into(),
        Route::Other(s) => s.clone(),
    }
}

pub fn detect(evidence: &Evidence, outcome: &Outcome, ctx: &Ctx) -> Vec<Pattern> {
    let mut out = vec![];
    for gr in &outcome.per_group {
        for (path, score) in &gr.results {
            if let PathKind::ExtendRule { rule, .. } = path
                && works(score)
                && let Some(r) = ctx.app_rules.iter().find(|r| &r.name == rule)
            {
                out.push(Pattern::R1RuleMissesProtocol {
                    rule: rule.clone(),
                    has: r.protocol,
                    needs: gr.group.protocol,
                });
            }
        }
    }
    for gr in &outcome.per_group {
        if gr.group.protocol == Protocol::Udp && gr.group.members.iter().any(discord_media) {
            let local = evidence
                .voice
                .as_ref()
                .and_then(|v| v.local_address.as_ref());
            out.push(Pattern::R2VoiceUdp {
                server: gr.group.key.clone(),
                via_vpn: local.is_some() && local == evidence.proxy_ip.as_ref(),
            });
        }
    }
    if let Some(conns) = &evidence.neko
        && neko::slow_dns(conns)
    {
        let mut ms: Vec<u32> = conns.iter().filter_map(|c| c.outbound_ms).collect();
        ms.sort_unstable();
        out.push(Pattern::R3SlowProxyDns {
            median_ms: ms[(ms.len() - 1) / 2],
            direct_dns: evidence
                .neko_routes
                .as_ref()
                .map(|r| r.direct_dns.clone())
                .unwrap_or_default(),
        });
    }
    out.extend(
        evidence
            .dns_mismatch
            .iter()
            .map(|d| Pattern::R4DnsSpoofed { domain: d.clone() }),
    );
    if evidence.ru_ok == Some(true) && evidence.foreign_syn_dead == Some(true) {
        out.push(Pattern::R5Whitelist);
    }
    if let Some(v) = &evidence.voice
        && discord::stalled(v)
    {
        let route = evidence
            .neko
            .as_ref()
            .and_then(|c| neko::route_of(c, "discord.media"));
        out.push(Pattern::R6DaveStalled {
            alone: v.peers.is_empty(),
            direct: route == Some(Route::Direct),
            control_route: route.map(|r| route_text(&r)),
        });
    }
    unique(out)
}

/// First occurrences only, in order.
pub(crate) fn unique<T: PartialEq>(v: Vec<T>) -> Vec<T> {
    let mut out = Vec::with_capacity(v.len());
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

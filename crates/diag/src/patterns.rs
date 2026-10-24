//! Failure patterns R1–R6 from the live analysis (spec 16): all that fit, not the first one.

use std::net::SocketAddr;

use router::observe::FlowCounters;
use router::rules::Protocol;
use serde::{Deserialize, Serialize};

use crate::evaluate::works;
use crate::logs::discord::{self, Voice};
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
    R4DnsSpoofed {
        domain: String,
    },
    R5Whitelist,
    R6DaveStalled {
        alone: bool,
    },
}

/// What was seen besides the paths; a missing source is `None`.
#[derive(Clone, Debug, Default)]
pub struct Evidence {
    pub flows: Vec<FlowCounters>,
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

pub fn detect(evidence: &Evidence, outcome: &Outcome, ctx: &Ctx) -> Vec<Pattern> {
    let mut out = vec![];
    for gr in &outcome.per_group {
        for (path, score) in &gr.results {
            if let PathKind::ExtendRule { rule, .. } = path
                && works(score)
                && let Some(r) = ctx.rules.iter().find(|r| &r.name == rule)
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
    out.extend(
        evidence
            .dns_mismatch
            .iter()
            .map(|d| Pattern::R4DnsSpoofed { domain: d.clone() }),
    );
    if evidence.ru_ok == Some(true) && evidence.foreign_syn_dead == Some(true) {
        out.push(Pattern::R5Whitelist);
    }
    // Alone, DAVE waits for a second participant: normal, not a pattern (`judge` notes it).
    if let Some(v) = &evidence.voice
        && discord::stalled(v)
        && !v.peers.is_empty()
    {
        out.push(Pattern::R6DaveStalled { alone: false });
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

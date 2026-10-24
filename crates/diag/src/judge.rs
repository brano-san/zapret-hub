//! The run's answer as codes (agent proposal 2, schema review): verdict, confidence, the observed
//! counts, and the facts behind them. Pure: everything comes from what the run already has.

use router::observe::FlowCounters;
use router::rules::Protocol;

use crate::agent::{Confidence, Fact, Level, Observed, VerdictCode, VoiceStage};
use crate::culprits::{self, Fault, Verdict};
use crate::logs::discord;
use crate::paths::{Ctx, NO_TRICKS_FOR_SYN, PROXY_NO_UDP, PROXY_SILENT, PROXY_UNSET, routed_note};
use crate::patterns::{Evidence, Pattern, unique};
use crate::profiles::ProbeKind;
use crate::run::SYN_DEAD;
use crate::summary::{Summary, WHITELIST, pattern_texts};

const DAVE_ALONE: &str =
    "вы один в канале: Discord ждёт второго участника, чтобы начать шифрование (DAVE); это не сбой";
const PROXY_UNSEEN: &str =
    "соединения через системный прокси не разобраны: нет журнала или маршрутов NekoBox";
const NO_VOICE_FLOW: &str =
    "голосового UDP-потока не было: зайдите в голосовой канал во время наблюдения";

/// Discord voice: Cloudflare 19294–19344, GCP 50000 and up.
fn voice_port(port: u16) -> bool {
    matches!(port, 19294..=19344 | 50000..)
}

fn port_of(f: &FlowCounters) -> Option<u16> {
    f.dst.rsplit_once(':')?.1.parse().ok()
}

/// A stable code for a `sources_missing` entry (evidence.rs words them).
fn source_code(what: &str) -> &'static str {
    match what {
        "журнал NekoBox" => "neko_log",
        "маршруты NekoBox" => "neko_routes",
        "журнал Discord" => "discord_log",
        _ => "other",
    }
}

/// What a clean run did not see, as codes with their text.
fn unseen(
    flows: &[FlowCounters],
    evidence: &Evidence,
    ctx: &Ctx,
) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![];
    let neko_missing = evidence.neko.is_none() || evidence.neko_routes.is_none();
    if neko_missing && flows.iter().any(|f| f.via_system_proxy) {
        out.push(("proxy_unseen", PROXY_UNSEEN));
    }
    let voice_probe = ctx
        .profile
        .probes
        .iter()
        .any(|p| p.kind == ProbeKind::DiscordIpDiscovery);
    let voice_seen = flows
        .iter()
        .any(|f| f.protocol == Protocol::Udp && port_of(f).is_some_and(voice_port));
    if voice_probe && !voice_seen {
        out.push(("no_voice_flow", NO_VOICE_FLOW));
    }
    out
}

/// Fails the way NekoBox sends it now (as `summarize` words its verdict).
fn proxied_fails(p: &Pattern) -> bool {
    match p {
        Pattern::R1ViaSystemProxy {
            direct_ok,
            proxy_ok,
            route,
            direct,
            rule,
            ..
        } => {
            if *direct {
                !direct_ok
            } else if route.is_some() || rule.is_some() {
                !proxy_ok
            } else {
                !direct_ok && !proxy_ok
            }
        }
        _ => false,
    }
}

fn observed(flows: &[FlowCounters]) -> Observed {
    let count = |f: &dyn Fn(&FlowCounters) -> bool| flows.iter().filter(|x| f(x)).count() as u32;
    Observed {
        secs: 0,
        flows: flows.len() as u32,
        tcp: count(&|f| f.protocol == Protocol::Tcp),
        udp: count(&|f| f.protocol == Protocol::Udp),
        via_router: count(&|f| f.via_router),
        via_system_proxy: count(&|f| f.via_system_proxy),
    }
}

/// Discord's own log outranks a quiet observation: no media session means the problem was not
/// reproduced; a session stuck before `Connected` means a clean watch missed it (a flow opened
/// before watching is invisible).
fn client_stage(code: VerdictCode, summary: &Summary, ctx: &Ctx) -> (VerdictCode, Option<String>) {
    let quiet = matches!(
        code,
        VerdictCode::NoFlows | VerdictCode::Clean | VerdictCode::Inconclusive
    );
    let client = &summary.agent.client;
    let media = match ctx.profile.id.as_str() {
        "discord" => client.voice.as_ref(),
        "discord-stream" => client.stream.as_ref(),
        _ => return (code, None),
    };
    // No log, no stage: the observation stands.
    if !quiet
        || summary
            .sources_missing
            .iter()
            .any(|s| source_code(s) == "discord_log")
    {
        return (code, None);
    }
    match media.map(|m| m.stage) {
        None | Some(VoiceStage::None) => (
            VerdictCode::NotReproducible,
            Some("no_media_session".into()),
        ),
        Some(VoiceStage::Connected | VoiceStage::DaveWaitingPeer) => (code, None),
        Some(stage) => (
            VerdictCode::Inconclusive,
            Some(format!("client_stage:{}", stage_code(stage))),
        ),
    }
}

fn stage_code(stage: VoiceStage) -> &'static str {
    match stage {
        VoiceStage::None => "none",
        VoiceStage::NoEndpoint => "no_endpoint",
        VoiceStage::WsFailed => "ws_failed",
        VoiceStage::UdpFailed => "udp_failed",
        VoiceStage::DaveWaitingPeer => "dave_waiting_peer",
        VoiceStage::DaveStalled => "dave_stalled",
        VoiceStage::Connected => "connected",
    }
}

/// The verdict code, how sure it is and why, and the observation in counts.
pub fn judge(
    summary: &Summary,
    flows: &[FlowCounters],
    evidence: &Evidence,
    ctx: &Ctx,
) -> (VerdictCode, Confidence, Observed) {
    let unseen = unseen(flows, evidence, ctx);
    let live_only = !ctx.profile.probes.is_empty()
        && ctx.profile.probes.iter().all(|p| p.kind == ProbeKind::Live)
        && !summary.groups.is_empty()
        && summary.groups.iter().all(|g| {
            g.results
                .iter()
                .all(|(_, s)| s.total == 0 && s.answer.is_none())
        });
    let code = if summary.patterns.contains(&Pattern::R5Whitelist) {
        VerdictCode::Whitelist
    } else {
        match culprits::verdict(flows) {
            Verdict::NoFlows => VerdictCode::NoFlows,
            Verdict::Clean if summary.patterns.iter().any(proxied_fails) => {
                VerdictCode::ProxyBroken
            }
            Verdict::Clean
                if summary
                    .patterns
                    .iter()
                    .any(|p| matches!(p, Pattern::R6DaveStalled { .. })) =>
            {
                VerdictCode::PeerSide
            }
            Verdict::Clean if !unseen.is_empty() => VerdictCode::Inconclusive,
            Verdict::Clean => VerdictCode::Clean,
            Verdict::Culprits(_) if live_only => VerdictCode::LiveOnly,
            Verdict::Culprits(_) => {
                let fixed = summary.groups.iter().filter(|g| g.best.is_some()).count();
                match fixed {
                    0 => VerdictCode::CulpritsUnfixed,
                    n if n == summary.groups.len() => VerdictCode::CulpritsFixed,
                    _ => VerdictCode::CulpritsPartial,
                }
            }
        }
    };
    let (code, stage_why) = client_stage(code, summary, ctx);
    let mut why: Vec<String> = stage_why.into_iter().collect();
    for (on, code) in [
        (summary.canceled, "canceled"),
        (summary.stopped, "stopped"),
        (summary.budget_hit, "budget_hit"),
        (code == VerdictCode::LiveOnly, "live_only"),
    ] {
        if on {
            why.push(code.into());
        }
    }
    // NekoBox's logs matter only for what went through the system proxy.
    let proxied = flows.iter().any(|f| f.via_system_proxy);
    for what in &summary.sources_missing {
        let source = source_code(what);
        if proxied || !source.starts_with("neko") {
            why.push(format!("source_missing:{source}"));
        }
    }
    let at_floor = summary.groups.iter().any(|g| {
        g.best
            .and_then(|i| g.results.get(i as usize))
            .is_some_and(|(_, s)| s.answer.is_none() && s.ok < s.total)
    });
    if at_floor {
        why.push("best_at_floor".into());
    }
    if unseen.iter().any(|(c, _)| *c == "no_voice_flow") {
        why.push("no_voice_flow".into());
    }
    if code == VerdictCode::Inconclusive {
        why.push("inconclusive".into());
    }
    let level = match code {
        _ if !why.is_empty() => Level::Low,
        VerdictCode::Clean | VerdictCode::CulpritsFixed | VerdictCode::PeerSide => Level::High,
        _ => Level::Medium,
    };
    (
        code,
        Confidence {
            level,
            why: unique(why),
        },
        observed(flows),
    )
}

fn fault_fact(key: &str, fault: &Fault, text: &str) -> Fact {
    let (code, nums): (&str, Vec<u32>) = match fault {
        Fault::SynUnanswered { syns } => ("syn_unanswered", vec![*syns]),
        Fault::ResetEarly => ("reset_early", vec![]),
        Fault::RedirectEarly => ("redirect_early", vec![]),
        Fault::DataRetrans { count } => ("data_retrans", vec![*count]),
        Fault::UdpNoReply { up, down } => ("udp_no_reply", vec![*up, *down]),
        Fault::Slow { median_ms } => ("slow", vec![*median_ms]),
        Fault::Lossy { loss_pct } => ("lossy", vec![*loss_pct]),
    };
    let mut args = vec![key.to_owned()];
    args.extend(nums.iter().map(u32::to_string));
    Fact {
        code: format!("fault:{code}"),
        args,
        text: text.to_owned(),
    }
}

fn pattern_fact(p: &Pattern) -> Fact {
    let s = |x: &dyn ToString| x.to_string();
    let (code, args) = match p {
        Pattern::R1RuleMissesProtocol { rule, has, needs } => (
            "r1_rule_misses_protocol",
            vec![rule.clone(), format!("{has:?}"), format!("{needs:?}")],
        ),
        Pattern::R1ViaSystemProxy {
            domain,
            direct_ok,
            proxy_ok,
            ..
        } => (
            "r1_via_system_proxy",
            vec![domain.clone(), s(direct_ok), s(proxy_ok)],
        ),
        Pattern::R2VoiceUdp { server, via_vpn } => {
            ("r2_voice_udp", vec![server.clone(), s(via_vpn)])
        }
        Pattern::R3SlowProxyDns {
            median_ms,
            direct_dns,
        } => ("r3_slow_proxy_dns", vec![s(median_ms), direct_dns.clone()]),
        Pattern::R4DnsSpoofed { domain } => ("r4_dns_spoofed", vec![domain.clone()]),
        Pattern::R5Whitelist => ("r5_whitelist", vec![]),
        Pattern::R6DaveStalled { control_route, .. } => {
            ("r6_dave_stalled", control_route.iter().cloned().collect())
        }
    };
    let mut texts = vec![];
    pattern_texts(std::slice::from_ref(p), &mut texts, &mut vec![]);
    Fact {
        code: format!("pattern:{code}"),
        args,
        text: texts.pop().unwrap_or_else(|| WHITELIST.into()),
    }
}

/// A group's note: what was left out, as a code; `None` for a note that is not about that.
fn note_code(note: &str) -> Option<&'static str> {
    let routed = routed_note("");
    let routed = routed.split('«').next().unwrap_or_default();
    Some(match note {
        n if n.starts_with(routed) => return None,
        n if n.ends_with(PROXY_NO_UDP) => "proxy_no_udp",
        n if n.ends_with(PROXY_SILENT) => "proxy_silent",
        n if n.ends_with(PROXY_UNSET) => "proxy_unset",
        n if n.ends_with(SYN_DEAD) => "syn_dead",
        NO_TRICKS_FOR_SYN => "no_tricks_for_syn",
        _ => "path_skipped",
    })
}

/// Fills `summary.agent`: the verdict, confidence and counts (`judge`), the facts behind them,
/// what was not checked; a clean run that missed part of the picture says so in its text.
pub fn fill(
    summary: &mut Summary,
    flows: &[FlowCounters],
    evidence: &Evidence,
    ctx: &Ctx,
    observe_secs: u32,
) {
    let (verdict, confidence, mut seen) = judge(summary, flows, evidence, ctx);
    seen.secs = observe_secs;
    let mut facts: Vec<Fact> = summary
        .groups
        .iter()
        .map(|g| fault_fact(&g.group.key, &g.group.fault, &g.seen))
        .collect();
    facts.extend(summary.patterns.iter().map(pattern_fact));
    if let Some(v) = &evidence.voice
        && discord::stalled(v)
        && v.peers.is_empty()
    {
        facts.push(Fact {
            code: "client:dave_waiting_peer".into(),
            args: vec![],
            text: DAVE_ALONE.into(),
        });
        summary.explanations.push(DAVE_ALONE.into());
    }
    let mut not_checked: Vec<Fact> = summary
        .sources_missing
        .iter()
        .map(|what| Fact {
            code: format!("source_missing:{}", source_code(what)),
            args: vec![],
            text: format!("нет источника: {what}"),
        })
        .collect();
    for g in &summary.groups {
        for note in &g.notes {
            let Some(code) = note_code(note) else {
                continue;
            };
            // Both proxy paths share a reason: one fact for them.
            let dup = code != "path_skipped"
                && not_checked
                    .iter()
                    .any(|f| f.code == code && f.args == [g.group.key.clone()]);
            if !dup {
                not_checked.push(Fact {
                    code: code.into(),
                    args: vec![g.group.key.clone()],
                    text: note.clone(),
                });
            }
        }
    }
    let unseen = unseen(flows, evidence, ctx);
    not_checked.extend(unseen.iter().map(|(code, text)| Fact {
        code: (*code).into(),
        args: vec![],
        text: (*text).into(),
    }));
    if verdict == VerdictCode::Inconclusive {
        let why: Vec<&str> = unseen.iter().map(|(_, t)| *t).collect();
        summary.verdict_text = format!(
            "соединения программы проходят, но видно не всё: {}",
            why.join("; ")
        );
    }
    let agent = &mut summary.agent;
    agent.verdict = verdict;
    agent.confidence = confidence;
    agent.observed = seen;
    agent.evidence = facts;
    agent.not_checked = not_checked;
    agent.context.proxy_udp = ctx.proxy_udp;
}

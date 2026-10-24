//! The sustained probes of a path (agent proposal 5, item 4): an IP discovery burst, the voice
//! WebSocket, a bulk exchange. Each reports per target; the 3-attempt probes stay in `attempts`.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use diag::culprits::Group;
use diag::evaluate::{ProbeScore, Score, median};
use diag::logs;
use diag::profiles::{DISCORD_GATEWAY, ProbeKind, ProbeSpec, discord_ws_host, voice_endpoint};
use probe::curl::{BulkStatus, Via};
use router::rules::Protocol;

use super::{ATTEMPTS, Target, ipv4_of, targets};

const BURST_PACKETS: u32 = 40;
const BURST_SPAN: Duration = Duration::from_secs(4);
/// A burst takes 5 s: at most this many representatives get one.
const BURST_TARGETS: usize = 2;
/// Of the log, enough for the last voice connection.
const LOG_BYTES: u64 = 1 << 20;

/// The score of a sustained probe; `None` for the other kinds.
pub(super) fn sustained(group: &Group, spec: &ProbeSpec) -> Option<Score> {
    let probes = match spec.kind {
        ProbeKind::DiscordIpDiscovery => burst(group, spec),
        ProbeKind::DiscordWs => ws(group),
        ProbeKind::HttpsBulk => bulk(group),
        _ => return None,
    };
    Some(total(spec.kind, probes))
}

/// The probes summed; loss for the burst only (HTTPS kinds measure none).
fn total(kind: ProbeKind, probes: Vec<ProbeScore>) -> Score {
    let ok = probes.iter().map(|p| p.ok).sum();
    let total = probes.iter().map(|p| p.total).sum::<u32>();
    Score {
        ok,
        total,
        median_ms: median(probes.iter().filter_map(|p| p.median_ms).collect()),
        loss_pct: (kind == ProbeKind::DiscordIpDiscovery && total > 0)
            .then(|| (total - ok) * 100 / total),
        answer: None,
        probes,
    }
}

fn burst(group: &Group, spec: &ProbeSpec) -> Vec<ProbeScore> {
    targets(group, spec)
        .into_iter()
        .filter_map(|t| match t {
            Target::Udp(dst) => Some(dst),
            _ => None,
        })
        .take(BURST_TARGETS)
        .map(|dst| {
            let b = probe::udp::ip_discovery_burst(dst, BURST_PACKETS, BURST_SPAN);
            ProbeScore {
                kind: ProbeKind::DiscordIpDiscovery,
                target: dst.to_string(),
                ok: b.received,
                total: b.sent,
                median_ms: b.median_ms,
                error: (b.received == 0).then(|| "no_reply".into()),
                kbps: None,
            }
        })
        .collect()
}

/// The newest voice server in the Discord logs, read now: the client may have joined during the
/// run.
fn endpoint() -> Option<(String, u16)> {
    logs::discord_log_paths()
        .iter()
        .filter_map(|p| logs::tail_from(p, 0, LOG_BYTES).ok().flatten())
        .find_map(|lines| voice_endpoint(&lines))
}

/// Representatives of a TCP group named `wanted`, with their address and port.
fn named(group: &Group, wanted: impl Fn(&str) -> bool) -> Vec<(String, IpAddr, u16)> {
    if group.protocol == Protocol::Udp {
        return Vec::new();
    }
    group
        .representatives
        .iter()
        .filter_map(|r| {
            let host = r.sni.clone().or_else(|| r.domain.clone())?;
            let port = r.dst.parse::<SocketAddr>().ok()?.port();
            (wanted(&host)).then_some((host, IpAddr::V4(ipv4_of(r)?), port))
        })
        .collect()
}

/// The WebSocket upgrade to each representative that is a Discord WebSocket host (the log's
/// voice server, else the gateway's name), pinned to its address. Through the proxy it goes on
/// the proxy paths, whose rule takes curl.
fn ws(group: &Group) -> Vec<ProbeScore> {
    let host = endpoint().map(|(h, _)| h);
    let host = host.as_deref().unwrap_or(DISCORD_GATEWAY);
    named(group, |name| discord_ws_host(name, Some(host)))
        .into_iter()
        .map(|(host, ip, port)| {
            let answers: Vec<Option<u32>> = (0..ATTEMPTS)
                .map(|_| probe::curl::ws_upgrade(&host, port, Some(ip), &Via::Direct))
                .collect();
            let ok = answers.iter().flatten().count() as u32;
            ProbeScore {
                kind: ProbeKind::DiscordWs,
                target: format!("{host}:{port}"),
                ok,
                total: ATTEMPTS as u32,
                median_ms: median(answers.into_iter().flatten().collect()),
                error: (ok == 0).then(|| "no_upgrade".into()),
                kbps: None,
            }
        })
        .collect()
}

/// The POST body, written once.
fn payload() -> Option<PathBuf> {
    let path = std::env::temp_dir().join("zapret-hub-diag-bulk.bin");
    (path.exists() || probe::curl::write_payload(&path).is_ok()).then_some(path)
}

/// The bulk exchange with each named representative.
fn bulk(group: &Group) -> Vec<ProbeScore> {
    let Some(payload) = payload() else {
        return Vec::new();
    };
    named(group, |_| true)
        .into_iter()
        .map(|(host, ip, port)| {
            let runs: Vec<_> = (0..ATTEMPTS)
                .map(|_| probe::curl::bulk(&host, port, Some(ip), &payload))
                .collect();
            bulk_score(format!("{host}:{port}"), &runs)
        })
        .collect()
}

/// Done runs count; a freeze names the probe `throttled`, nothing at all `no_transfer`.
fn bulk_score(target: String, runs: &[probe::curl::Bulk]) -> ProbeScore {
    let done: Vec<_> = runs.iter().filter(|b| b.status == BulkStatus::Ok).collect();
    let error = if runs.iter().any(|b| b.status == BulkStatus::Throttled) {
        Some("throttled".into())
    } else {
        done.is_empty().then(|| "no_transfer".into())
    };
    ProbeScore {
        kind: ProbeKind::HttpsBulk,
        target,
        ok: done.len() as u32,
        total: runs.len() as u32,
        median_ms: median(done.iter().filter_map(|b| b.time_ms).collect()),
        error,
        kbps: median(done.iter().filter_map(|b| b.kbps).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe::curl::Bulk;

    #[test]
    fn a_freeze_marks_the_bulk_probe_throttled() {
        let run = |status, kbps| Bulk {
            status,
            up_bytes: 65536,
            down_bytes: 0,
            time_ms: Some(500),
            kbps,
        };
        let s = bulk_score(
            "cdn.example.com:443".into(),
            &[
                run(BulkStatus::Ok, Some(200)),
                run(BulkStatus::Throttled, None),
                run(BulkStatus::Ok, Some(100)),
            ],
        );
        assert_eq!((s.ok, s.total, s.kbps), (2, 3, Some(100)));
        assert_eq!(s.error.as_deref(), Some("throttled"));
        let s = bulk_score("x:443".into(), &[run(BulkStatus::Fail, None)]);
        assert_eq!(s.error.as_deref(), Some("no_transfer"));
    }

    #[test]
    fn the_ws_probe_takes_named_discord_hosts_of_a_tcp_group() {
        use diag::culprits::Fault;
        use router::observe::FlowCounters;
        let rep = |dst: &str, sni: Option<&str>| FlowCounters {
            protocol: Protocol::Tcp,
            dst: dst.into(),
            sni: sni.map(str::to_owned),
            ..FlowCounters::default()
        };
        let mut g = Group {
            key: "203.0.113.0/24".into(),
            protocol: Protocol::Tcp,
            ports: vec![2096],
            members: vec![],
            representatives: vec![
                rep("203.0.113.5:2096", Some("c-ams1.discord.media")),
                rep("203.0.113.6:443", Some("cdn.example.com")),
                rep("203.0.113.7:443", None),
            ],
            fault: Fault::ResetEarly,
        };
        let ws = named(&g, |n| discord_ws_host(n, Some(DISCORD_GATEWAY)));
        assert_eq!(
            ws,
            [(
                "c-ams1.discord.media".to_owned(),
                "203.0.113.5".parse::<IpAddr>().unwrap(),
                2096
            )]
        );
        assert_eq!(named(&g, |_| true).len(), 2, "bulk: every named one");
        g.protocol = Protocol::Udp;
        assert!(named(&g, |_| true).is_empty());
    }

    #[test]
    fn a_burst_total_has_loss_and_https_kinds_do_not() {
        let p = |ok, total| ProbeScore {
            kind: ProbeKind::DiscordIpDiscovery,
            ok,
            total,
            median_ms: Some(30),
            ..ProbeScore::default()
        };
        let s = total(ProbeKind::DiscordIpDiscovery, vec![p(36, 40), p(40, 40)]);
        assert_eq!((s.ok, s.total, s.loss_pct), (76, 80, Some(5)));
        assert_eq!(s.probes.len(), 2);
        assert_eq!(total(ProbeKind::DiscordWs, vec![p(3, 3)]).loss_pct, None);
    }
}

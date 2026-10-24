//! What diagnostics see besides the paths (diag spec 5, 16): logs, DNS, the proxy's address.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use diag::culprits::Fault;
use diag::evaluate::works;
use diag::logs;
use diag::paths::Ctx;
use diag::patterns::Evidence;
use diag::run::Outcome;
use probe::curl::{CheckStatus, Via};
use router::observe::FlowCounters;

use super::UDP_TIMEOUT;
use crate::state::AppState;

/// At most this much of a log written during the diagnostics is read.
const LOG_BYTES: u64 = 4 << 20;
/// A plain DNS server the provider may answer for: its answer is compared with DoH.
const PLAIN_DNS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53);

/// The sizes of the logs when watching began: only what they get after it is read.
pub(super) struct Marks {
    discord: Vec<(PathBuf, u64)>,
    /// `discord-webrtc_0` and `_1` of each Discord; a file not there yet is marked at 0.
    pub(super) webrtc: Vec<(PathBuf, u64)>,
}

impl Marks {
    pub(super) fn now() -> Self {
        Marks {
            discord: logs::discord_log_paths()
                .into_iter()
                .map(|p| {
                    let at = logs::size(&p).unwrap_or(0);
                    (p, at)
                })
                .collect(),
            webrtc: super::client::webrtc_marks(),
        }
    }
}

/// Seconds since local midnight, as Discord stamps its log.
pub(super) fn wall_secs() -> f64 {
    let t = exec::LocalTime::now();
    f64::from(t.hour) * 3600.0 + f64::from(t.minute) * 60.0 + f64::from(t.second)
}

/// No address of the plain answer shares a /16 with DoH's: a CDN answers by location, but
/// rarely from another /16. An empty DoH answer proves nothing.
pub(super) fn differ(plain: &[Ipv4Addr], doh: &[Ipv4Addr]) -> bool {
    !doh.is_empty()
        && !plain
            .iter()
            .any(|a| doh.iter().any(|b| a.octets()[..2] == b.octets()[..2]))
}

/// ponytail: one plain server, not the system's resolvers; ask Windows for them if a provider
/// spoofs only its own resolver.
fn spoofed(name: &str, socks5: Option<&str>) -> bool {
    let Some(plain) = probe::udp::dns_udp(name, PLAIN_DNS, UDP_TIMEOUT) else {
        return false;
    };
    let doh = probe::curl::dns_doh(name, None)
        .or_else(|| socks5.and_then(|s| probe::curl::dns_doh(name, Some(s))));
    doh.is_some_and(|doh| differ(&plain, &doh))
}

/// The proxy's external address, as a site sees it through the proxy.
fn proxy_ip(socks5: &str) -> Option<String> {
    egress_ip(Some(socks5))
}

/// The external address a site sees, direct or through the proxy.
pub(super) fn egress_ip(socks5: Option<&str>) -> Option<String> {
    let mut cmd = probe::curl::get("https://api.ipify.org", socks5);
    let out = exec::run(&mut cmd, Duration::from_secs(15), None).ok()?;
    let ip = out.stdout.trim();
    (out.success() && ip.parse::<IpAddr>().is_ok()).then(|| ip.to_owned())
}

/// Every group's SYNs go unanswered, on every path.
pub(super) fn foreign_syn_dead(outcome: &Outcome) -> Option<bool> {
    (!outcome.per_group.is_empty()).then(|| {
        outcome.per_group.iter().all(|g| {
            matches!(g.group.fault, Fault::SynUnanswered { .. })
                && g.results.iter().all(|(_, s)| !works(s))
        })
    })
}

/// What was seen besides the paths, and the sources that were not there (diag spec 5).
pub(super) fn evidence(
    state: &AppState,
    ctx: &Ctx,
    flows: &[FlowCounters],
    outcome: &Outcome,
    marks: &Marks,
    network: bool,
) -> (Evidence, Vec<String>) {
    let socks5 = state.socks5();
    let mut missing = Vec::new();
    let read = |path: &Path, at: u64| logs::tail_from(path, at, LOG_BYTES).ok().flatten();
    let voices: Vec<logs::discord::Voice> = marks
        .discord
        .iter()
        .filter_map(|(path, at)| read(path, *at))
        .filter(|lines| !lines.is_empty())
        .map(|lines| logs::discord::parse(&lines, Some(wall_secs())))
        .collect();
    let voice = voices
        .iter()
        .find(|v| v.media_server.is_some())
        .or(voices.first())
        .cloned();
    // Missing is no file; a log without a voice session is a source that said nothing.
    if ctx.profile.id == "discord" && marks.discord.is_empty() {
        missing.push("журнал Discord".to_owned());
    }
    // The network part asks the network: not after a cancel.
    let socks5 = socks5.filter(|_| network);
    let proxy_ip = voice
        .as_ref()
        .filter(|v| v.local_address.is_some())
        .and(socks5.as_deref())
        .and_then(proxy_ip);
    // Side by side: each may wait for its timeouts.
    let dns_mismatch = std::thread::scope(|scope| {
        let checks: Vec<_> = ctx
            .profile
            .domains
            .iter()
            .filter(|_| network)
            .map(|d| (d, scope.spawn(|| spoofed(d, socks5.as_deref()))))
            .collect();
        checks
            .into_iter()
            .filter_map(|(d, check)| check.join().unwrap_or(false).then(|| d.clone()))
            .collect()
    });
    let ru_ok = network.then(|| {
        probe::curl::https_via("ya.ru", None, &Via::Direct, None).status == CheckStatus::Ok
    });
    (
        Evidence {
            flows: flows.to_vec(),
            voice,
            proxy_ip,
            dns_mismatch,
            ru_ok,
            foreign_syn_dead: foreign_syn_dead(outcome),
        },
        missing,
    )
}

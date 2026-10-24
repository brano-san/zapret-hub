//! Discord's media engine log, `discord-webrtc_0` / `_1` (agent proposal 3): the 5-second pings
//! by sequence number, failed UDP reconnections, the transport's packet counters, DAVE decrypt
//! failures and the region latency test. Only what was written during the diagnostics is read.

use std::collections::BTreeMap;

use crate::agent::{Fact, MediaState};

/// A few frames fail at every key change; this many is a peer that cannot be heard.
/// ponytail: a fixed count; per minute of the window if runs get long.
const DECRYPT_FAILS_MIN: u64 = 20;

#[derive(Debug, Default, Clone, PartialEq)]
struct Pings {
    /// Sequence numbers sent, in order.
    sent: Vec<u32>,
    /// Sequence number -> RTT in ms.
    got: BTreeMap<u32, u32>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Webrtc {
    /// By media server, `ip:port`.
    pings: BTreeMap<String, Pings>,
    /// Failed UDP reconnection attempts by media server.
    reconnects: BTreeMap<String, u32>,
    /// `packets sent`, `received` of `Transport stats` by connection (`default`, `stream`).
    transport: BTreeMap<String, Vec<(u64, u64)>>,
    /// `Decrypt failed, no valid cryptor` lines.
    no_cryptor: u32,
    /// `Failed audio` of `Decrypted audio` lines: the first and the last.
    failed_audio: Option<(u64, u64)>,
    /// `Average RTT for region X is Y`.
    regions: Vec<(String, f64)>,
}

fn num_after<T: std::str::FromStr>(s: &str, key: &str) -> Option<T> {
    let rest = &s[s.find(key)? + key.len()..];
    rest.split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()?
        .parse()
        .ok()
}

/// `[time] [thread] (file:line): message` -> (thread, message).
fn split(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?.split_once("] [")?.1;
    let (thread, rest) = rest.split_once(']')?;
    let msg = rest.split_once("): ").map_or(rest, |(_, m)| m);
    Some((thread, msg))
}

pub fn parse(lines: &[String]) -> Webrtc {
    let mut w = Webrtc::default();
    // The UDP socket's thread sends its pings and runs its reconnections.
    let mut thread_server: BTreeMap<&str, String> = BTreeMap::new();
    for l in lines {
        let Some((thread, msg)) = split(l) else {
            continue;
        };
        if let Some(rest) = msg.strip_prefix("Sending ping to ") {
            let Some((server, _)) = rest.split_once(", seq: ") else {
                continue;
            };
            if let Some(seq) = num_after(rest, "seq: ") {
                w.pings.entry(server.to_owned()).or_default().sent.push(seq);
            }
            thread_server.insert(thread, server.to_owned());
        } else if let Some(rest) = msg.strip_prefix("Ping received from ") {
            let Some((server, _)) = rest.split_once(", seq: ") else {
                continue;
            };
            if let (Some(seq), Some(rtt)) = (num_after(rest, "seq: "), num_after(rest, "rtt: ")) {
                w.pings
                    .entry(server.to_owned())
                    .or_default()
                    .got
                    .insert(seq, rtt);
            }
        } else if msg.starts_with("ReconnectionAttemptFinished: ") && msg.contains("succeeded: 0") {
            if let Some(server) = thread_server.get(thread) {
                *w.reconnects.entry(server.clone()).or_default() += 1;
            }
        } else if let Some(rest) = msg.strip_prefix('[')
            && let Some((tag, rest)) = rest.split_once("] Transport stats for user: ")
        {
            let Some((_, counters)) = rest.split_once("packets sent: ") else {
                continue;
            };
            let sent = num_after(counters, "").unwrap_or(0);
            let received = num_after(counters, "received: ").unwrap_or(0);
            w.transport
                .entry(tag.to_owned())
                .or_default()
                .push((sent, received));
        } else if msg.starts_with("Decrypt failed, no valid cryptor") {
            w.no_cryptor += 1;
        } else if msg.starts_with("Decrypted audio: ") {
            if let Some(failed) = num_after(msg, "Failed audio: ") {
                let first = w.failed_audio.map_or(failed, |(first, _)| first);
                w.failed_audio = Some((first, failed));
            }
        } else if let Some(rest) = msg.strip_prefix("Average RTT for region ")
            && let Some((region, rtt)) = rest.split_once(" is ")
            && let Ok(rtt) = rtt.trim().parse()
        {
            // a later test of a region replaces the earlier one
            w.regions.retain(|(r, _)| r != region);
            w.regions.push((region.to_owned(), rtt));
        }
    }
    w
}

impl Webrtc {
    /// Lost pings in percent; the newest unanswered ping may still be on its way.
    pub fn loss_pct(&self, server: &str) -> Option<u32> {
        let p = self.pings.get(server)?;
        let mut sent = p.sent.as_slice();
        if let Some((last, rest)) = sent.split_last()
            && !p.got.contains_key(last)
        {
            sent = rest;
        }
        if sent.is_empty() {
            return None;
        }
        let lost = sent.iter().filter(|s| !p.got.contains_key(s)).count();
        Some((lost * 100 / sent.len()) as u32)
    }

    /// The median RTT of the answered pings.
    pub fn rtt_ms(&self, server: &str) -> Option<u32> {
        let mut rtts: Vec<u32> = self.pings.get(server)?.got.values().copied().collect();
        rtts.sort_unstable();
        rtts.get(rtts.len() / 2).copied()
    }

    pub fn reconnects(&self, server: &str) -> u32 {
        self.reconnects.get(server).copied().unwrap_or(0)
    }

    /// The connection sent packets and got none back. One sample counts from zero.
    pub fn one_way(&self, tag: &str) -> bool {
        let Some(samples) = self.transport.get(tag) else {
            return false;
        };
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            return false;
        };
        let from = if samples.len() > 1 { *first } else { (0, 0) };
        last.0 > from.0 && last.1 == from.1
    }

    /// The counters of a connection whose stage the renderer log gave; `tag` is `default` or
    /// `stream`.
    pub fn fill(&self, media: &mut MediaState, tag: &str) {
        if let Some(server) = media.server.as_deref() {
            media.loss_pct = self.loss_pct(server);
            media.rtt_ms = self.rtt_ms(server);
            media.reconnects = self.reconnects(server);
        }
        media.one_way = self.one_way(tag);
    }

    /// DAVE decrypt failures and the best region.
    pub fn facts(&self) -> Vec<Fact> {
        let mut facts = Vec::new();
        let failed = self
            .failed_audio
            .map_or(0, |(first, last)| last.saturating_sub(first));
        if u64::from(self.no_cryptor).max(failed) >= DECRYPT_FAILS_MIN {
            facts.push(Fact {
                code: "dave_decrypt_failed".into(),
                args: vec![self.no_cryptor.to_string(), failed.to_string()],
                text: format!(
                    "Discord: звук собеседника не расшифровывается (DAVE), ошибок: {}",
                    u64::from(self.no_cryptor).max(failed)
                ),
            });
        }
        if let Some((best, rtt)) = self.regions.iter().min_by(|a, b| a.1.total_cmp(&b.1)) {
            let mut args = vec![best.clone(), format!("{rtt:.0}")];
            args.extend(self.regions.iter().map(|(r, t)| format!("{r}={t:.0}")));
            facts.push(Fact {
                code: "region_rtt".into(),
                args,
                text: format!("Discord: ближе всего регион {best} ({rtt:.0} мс)"),
            });
        }
        facts
    }
}

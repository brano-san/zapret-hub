//! A path's result and the ranking of paths (spec section 7).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::paths::PathKind;
use crate::profiles::{Goal, ProbeKind};

/// One probe of a path, on one target (agent proposal 5, item 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ProbeScore {
    pub kind: ProbeKind,
    pub target: String,
    pub ok: u32,
    pub total: u32,
    pub median_ms: Option<u32>,
    /// Why it failed: `throttled`, `no_upgrade`, ...
    #[serde(default)]
    pub error: Option<String>,
    /// Throughput of a bulk exchange, KiB/s.
    #[serde(default)]
    pub kbps: Option<u32>,
}

impl Default for ProbeScore {
    fn default() -> Self {
        ProbeScore {
            kind: ProbeKind::Https,
            target: String::new(),
            ok: 0,
            total: 0,
            median_ms: None,
            error: None,
            kbps: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub enum Answer {
    Yes,
    No,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "DiagScore")]
pub struct Score {
    pub ok: u32,
    pub total: u32,
    pub median_ms: Option<u32>,
    pub loss_pct: Option<u32>,
    /// The user's answer in the "with me" mode.
    pub answer: Option<Answer>,
    /// Per probe and target; empty in reports from before it.
    #[serde(default)]
    pub probes: Vec<ProbeScore>,
}

/// Even count: the lower middle. The one median of the diagnostics.
pub fn median(mut v: Vec<u32>) -> Option<u32> {
    v.sort_unstable();
    v.get(v.len().checked_sub(1)? / 2).copied()
}

/// The user's yes or no decides. With a per-probe breakdown, every probe kind that ran passes
/// (`kind_works`); without one (old reports), at least 2 of 3 attempts. No attempts: does not work.
pub fn works(s: &Score) -> bool {
    match s.answer {
        Some(Answer::Yes) => true,
        Some(Answer::No) => false,
        _ if s.probes.is_empty() => s.total > 0 && s.ok * 3 >= s.total * 2,
        _ => {
            let mut ran = false;
            for p in &s.probes {
                // Ping only measures latency.
                if p.kind == ProbeKind::Ping || p.total == 0 {
                    continue;
                }
                let (ok, total) = s
                    .probes
                    .iter()
                    .filter(|q| q.kind == p.kind)
                    .fold((0, 0), |(o, t), q| (o + q.ok, t + q.total));
                if !kind_works(p.kind, ok, total) {
                    return false;
                }
                ran = true;
            }
            ran
        }
    }
}

/// A sustained IP discovery burst: under 20 % loss and at least 3 replies. Others: 2 of 3.
pub fn kind_works(kind: ProbeKind, ok: u32, total: u32) -> bool {
    match kind {
        ProbeKind::DiscordIpDiscovery if total > 3 => ok >= 3 && (total - ok) * 5 < total,
        _ => total > 0 && ok * 3 >= total * 2,
    }
}

/// Medians within 15 % of the smaller are a tie.
fn close(a: u32, b: u32) -> bool {
    a.abs_diff(b) * 100 <= a.min(b) * 15
}

fn compare(a: &(PathKind, Score), b: &(PathKind, Score)) -> Ordering {
    let ((pa, sa), (pb, sb)) = (a, b);
    let loss = |s: &Score| s.loss_pct.unwrap_or(0);
    let by_works = works(sb).cmp(&works(sa));
    let by_median = match (sa.median_ms, sb.median_ms) {
        (Some(x), Some(y)) if close(x, y) => loss(sa)
            .cmp(&loss(sb))
            .then(pb.pointed().cmp(&pa.pointed()))
            .then(x.cmp(&y)),
        (Some(x), Some(y)) => x.cmp(&y),
        // Unmeasured last.
        (x, y) => x.is_none().cmp(&y.is_none()),
    };
    by_works
        .then(by_median)
        .then(loss(sa).cmp(&loss(sb)))
        .then(pb.pointed().cmp(&pa.pointed()))
}

/// Latency: works first, then the lower median RTT; loss and pointed paths only break exact ties.
fn compare_latency(a: &(PathKind, Score), b: &(PathKind, Score)) -> Ordering {
    let ((pa, sa), (pb, sb)) = (a, b);
    let loss = |s: &Score| s.loss_pct.unwrap_or(0);
    works(sb)
        .cmp(&works(sa))
        .then(sa.median_ms.is_none().cmp(&sb.median_ms.is_none()))
        .then(sa.median_ms.cmp(&sb.median_ms))
        .then(loss(sa).cmp(&loss(sb)))
        .then(pb.pointed().cmp(&pa.pointed()))
}

/// `rank`, or for `Goal::Latency` the fastest working path first.
pub fn rank_for(results: &mut [(PathKind, Score)], goal: Goal) {
    match goal {
        Goal::Reach => rank(results),
        Goal::Latency => results.sort_by(compare_latency),
    }
}

/// Works first; then by median; within a ±15 % tie by loss, then pointed paths first.
pub fn rank(results: &mut [(PathKind, Score)]) {
    // The ±15 % tie is not transitive and `sort_by` may panic on that: a stable insertion sort.
    for i in 1..results.len() {
        let mut j = i;
        while j > 0 && compare(&results[j - 1], &results[j]) == Ordering::Greater {
            results.swap(j - 1, j);
            j -= 1;
        }
    }
}

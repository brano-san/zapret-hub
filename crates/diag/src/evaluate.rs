//! A path's result and the ranking of paths (spec section 7).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::paths::PathKind;

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
}

/// Even count: the lower middle. The one median of the diagnostics.
pub fn median(mut v: Vec<u32>) -> Option<u32> {
    v.sort_unstable();
    v.get(v.len().checked_sub(1)? / 2).copied()
}

/// The user's yes or no decides; else at least 2 of 3 attempts. No attempts: does not work.
pub fn works(s: &Score) -> bool {
    match s.answer {
        Some(Answer::Yes) => true,
        Some(Answer::No) => false,
        _ => s.total > 0 && s.ok * 3 >= s.total * 2,
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

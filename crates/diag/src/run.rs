//! Trying the paths of every culprit within a budget, with rollback at the end (spec 3, 6, 7, 9).

use std::time::Duration;

use anyhow::ensure;
use router::observe::FlowCounters;
use serde::{Deserialize, Serialize};

use crate::agent::Fact;
use crate::culprits::{Fault, Group, fault};
use crate::evaluate::{Answer, ProbeScore, Score, median, rank_for, works};
use crate::paths::{Ctx, PathKind, plan};
use crate::profiles::{Goal, ProbeKind, ProbeSpec};

/// A step of the diagnostics, for the page. Named for TypeScript: `tester` has a `Progress` too, and
/// specta renames no enum.
pub type Progress = DiagStep;

/// What the diagnostics runs on: the hub, or a fake in tests.
pub trait Env {
    fn set_path(&mut self, path: &PathKind, group: &Group) -> anyhow::Result<()>;
    /// 3 attempts, aggregated.
    fn probe(&mut self, group: &Group, probe: &ProbeSpec) -> Score;
    fn ask(&mut self, hint: &str, path: &PathKind) -> Answer;
    fn observe(&mut self, secs: u32) -> Vec<FlowCounters>;
    fn rollback(&mut self) -> anyhow::Result<()>;
    fn cancelled(&self) -> bool;
    /// «Остановить»: no more paths, but the measured ones make the report (unlike a cancel).
    fn stopped(&self) -> bool;
    fn elapsed(&self) -> Duration;
    fn progress(&mut self, step: Progress);
    /// The finished paths of the group being tried, after each one; empty when a group starts.
    fn results(&mut self, results: &[(PathKind, Score)]);
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "step")]
pub enum DiagStep {
    Observing {
        secs_left: u32,
        flows: Vec<FlowCounters>,
    },
    Culprits {
        groups: Vec<Group>,
    },
    Path {
        group: u32,
        /// The group's key, for progress lines.
        key: String,
        index: u32,
        total: u32,
        label: String,
    },
    Question {
        label: String,
        hint: String,
    },
    Rechecking,
    RollingBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Ask,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub budget: Duration,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct GroupResult {
    pub group: Group,
    /// Ranked.
    pub results: Vec<(PathKind, Score)>,
    pub skipped: Vec<String>,
    pub not_checked: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Outcome {
    pub per_group: Vec<GroupResult>,
    pub canceled: bool,
    /// Stopped by the user: the paths measured so far are the result.
    pub stopped: bool,
    pub budget_hit: bool,
    pub rollback_error: Option<String>,
}

/// What the user asked for: a profile or a program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "DiagRequest")]
pub struct Request {
    pub profile: Option<String>,
    pub app: Option<String>,
    pub mode: Mode,
    pub observe_secs: Option<u32>,
    pub budget_min: Option<u32>,
    /// None: the profile's goal (`reach`, `latency` for games).
    #[serde(default)]
    pub goal: Option<Goal>,
}

impl Request {
    pub fn check(&self) -> anyhow::Result<()> {
        ensure!(
            self.profile.is_some() != self.app.is_some(),
            "нужен либо профиль, либо программа"
        );
        if let Some(s) = self.observe_secs {
            ensure!((10..=120).contains(&s), "наблюдение — от 10 до 120 с");
        }
        if let Some(m) = self.budget_min {
            ensure!((1..=60).contains(&m), "бюджет — от 1 до 60 мин");
        }
        Ok(())
    }
}

/// A running diagnostics, as the UI sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "DiagLive")]
pub struct Live {
    pub request: Request,
    pub progress: Option<Progress>,
    pub question: Option<String>,
    pub choosing: Option<Vec<Group>>,
    /// The finished paths of the group being tried, in the order tried (spec 10: the live table).
    #[serde(default)]
    pub results: Vec<(PathKind, Score)>,
}

/// The group's other paths, when «Как сейчас» already passes the probes.
pub const CURRENT_WORKS: &str =
    "остальные пути не проверялись: «Как сейчас» проходит пробы, чинить нечего";

/// `current_works` for each group stopped at a working «Как сейчас» (proposal 5, item 4).
pub fn not_checked_facts(outcome: &Outcome) -> Vec<Fact> {
    outcome
        .per_group
        .iter()
        .filter(|g| g.not_checked.iter().any(|n| n == CURRENT_WORKS))
        .map(|g| Fact {
            code: "current_works".into(),
            args: vec![g.group.key.clone()],
            text: format!("{}: {CURRENT_WORKS}", g.group.key),
        })
        .collect()
}

pub const SYN_DEAD: &str = "не проверялось: адреса не отвечают на SYN ни напрямую, ни через прокси, стратегии это не исправят";

/// Representatives seen in a short observation, judged by the section 5 criteria; the median of
/// their RTT samples is the path's latency to the culprit.
fn live(env: &mut dyn Env, group: &Group) -> Score {
    let flows = env.observe(12);
    let mut s = Score::default();
    let mut rtts = vec![];
    for rep in &group.representatives {
        let mut seen = flows.iter().filter(|f| f.dst == rep.dst).peekable();
        if seen.peek().is_none() {
            continue;
        }
        s.total += 1;
        let seen: Vec<_> = seen.collect();
        rtts.extend(seen.iter().flat_map(|f| f.rtt_ms.iter().copied()));
        if seen.iter().all(|f| fault(f).is_none()) {
            s.ok += 1;
        }
    }
    s.median_ms = median(rtts);
    s
}

/// Probes whose time is a round trip to the culprit, not a handshake: the latency goal ranks by
/// them.
fn rtt_kind(kind: ProbeKind) -> bool {
    matches!(
        kind,
        ProbeKind::Ping | ProbeKind::DiscordIpDiscovery | ProbeKind::Stun | ProbeKind::Live
    )
}

/// The profile's probes; for the latency goal also a ping of the culprit, if none is there.
pub fn probes_for(profile: &crate::profiles::Profile) -> Vec<ProbeSpec> {
    let mut probes = profile.probes.clone();
    if profile.goal == Goal::Latency && !probes.iter().any(|p| p.kind == ProbeKind::Ping) {
        probes.push(ProbeSpec {
            kind: ProbeKind::Ping,
            target: "observed".into(),
        });
    }
    probes
}

/// The path's score, or `None` when canceled or stopped during it.
fn evaluate(env: &mut dyn Env, ctx: &Ctx, group: &Group) -> Option<Score> {
    let mut s = Score::default();
    let (mut medians, mut rtts) = (vec![], vec![]);
    // Loss over every attempt that can be lost (not HTTPS, not live), so one probe kind that never
    // answers cannot make 100 % of a path that half works.
    let (mut lost, mut sent) = (0, 0);
    for probe in &probes_for(&ctx.profile) {
        let p = match probe.kind {
            ProbeKind::Live => live(env, group),
            _ => env.probe(group, probe),
        };
        // Ping only measures latency.
        if probe.kind != ProbeKind::Ping {
            s.ok += p.ok;
            s.total += p.total;
        }
        medians.extend(p.median_ms);
        if rtt_kind(probe.kind) {
            rtts.extend(p.median_ms);
        }
        if p.loss_pct.is_some() {
            lost += p.total - p.ok;
            sent += p.total;
        }
        // The env's own breakdown by target, else one line for the probe.
        if p.probes.is_empty() {
            s.probes.push(ProbeScore {
                kind: probe.kind,
                target: probe.target.clone(),
                ok: p.ok,
                total: p.total,
                median_ms: p.median_ms,
                ..ProbeScore::default()
            });
        } else {
            s.probes.extend(p.probes);
        }
        if env.cancelled() || env.stopped() {
            return None;
        }
    }
    s.median_ms = match ctx.profile.goal {
        Goal::Latency if !rtts.is_empty() => median(rtts),
        _ => median(medians),
    };
    s.loss_pct = (sent > 0).then(|| lost * 100 / sent);
    Some(s)
}

pub fn run(env: &mut dyn Env, ctx: &Ctx, groups: &[Group], settings: &Settings) -> Outcome {
    let mut out = Outcome {
        per_group: vec![],
        canceled: false,
        stopped: false,
        budget_hit: false,
        rollback_error: None,
    };
    let mut stopped = false;
    // Spec 16 Р5: every culprit SYN-dead looks like a whitelist; strategies cannot help there.
    let syn_dead = groups
        .iter()
        .all(|g| matches!(g.fault, Fault::SynUnanswered { .. }));
    for (gi, group) in groups.iter().enumerate() {
        let (paths, skipped) = plan(group, ctx);
        let mut gr = GroupResult {
            group: group.clone(),
            results: vec![],
            skipped,
            not_checked: vec![],
        };
        env.results(&gr.results);
        for (i, path) in paths.iter().enumerate() {
            let label = path.label();
            if !stopped && env.cancelled() {
                out.canceled = true;
                stopped = true;
            }
            if !stopped && env.stopped() {
                out.stopped = true;
                stopped = true;
            }
            if !stopped && env.elapsed() >= settings.budget {
                out.budget_hit = true;
                stopped = true;
            }
            if stopped {
                gr.not_checked.push(label);
                continue;
            }
            // After «Как сейчас», «Напрямую» and the proxy all failed (the proxy too: an address
            // blocked by IP looks the same until then, and the proxy is its fix).
            if syn_dead
                && matches!(path, PathKind::Strategy { .. } | PathKind::Trick { .. })
                && gr.results.iter().all(|(_, s)| !works(s))
            {
                gr.not_checked.push(format!("{label}: {SYN_DEAD}"));
                continue;
            }
            env.progress(Progress::Path {
                group: gi as u32,
                key: group.key.clone(),
                index: i as u32,
                total: paths.len() as u32,
                label: label.clone(),
            });
            if let Err(e) = env.set_path(path, group) {
                // Not working; the next path sets its own configuration.
                gr.not_checked
                    .push(format!("{label}: не удалось поставить: {e:#}"));
                gr.results.push((path.clone(), Score::default()));
                env.results(&gr.results);
                continue;
            }
            let Some(mut s) = evaluate(env, ctx, group) else {
                let why = if env.cancelled() {
                    out.canceled = true;
                    "прервано"
                } else {
                    out.stopped = true;
                    "перебор остановлен"
                };
                stopped = true;
                gr.not_checked.push(format!("{label}: {why}"));
                continue;
            };
            if settings.mode == Mode::Ask {
                let hint = ctx.profile.hint.clone();
                env.progress(Progress::Question {
                    label: label.clone(),
                    hint: hint.clone(),
                });
                s.answer = Some(env.ask(&hint, path));
            }
            let answered = matches!(s.answer, Some(Answer::Yes | Answer::No));
            if s.total == 0 && !answered {
                gr.not_checked
                    .push(format!("{label}: не проверен: нет попыток программы"));
            }
            let current_works = *path == PathKind::Current && works(&s);
            gr.results.push((path.clone(), s));
            env.results(&gr.results);
            if current_works {
                gr.not_checked.push(CURRENT_WORKS.into());
                break;
            }
        }
        rank_for(&mut gr.results, ctx.profile.goal);
        out.per_group.push(gr);
    }
    env.progress(Progress::RollingBack);
    out.rollback_error = env.rollback().err().map(|e| format!("{e:#}"));
    out
}

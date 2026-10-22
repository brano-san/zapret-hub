//! Trying the paths of every culprit within a budget, with rollback at the end (spec 3, 6, 7, 9).

use std::time::Duration;

use anyhow::ensure;
use router::observe::FlowCounters;
use serde::{Deserialize, Serialize};

use crate::culprits::{Group, fault};
use crate::evaluate::{Answer, Score, rank};
use crate::paths::{Ctx, PathKind, plan};
use crate::profiles::{ProbeKind, ProbeSpec};

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
    fn elapsed(&self) -> Duration;
    fn progress(&mut self, step: Progress);
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
    /// 10..=120.
    pub observe_secs: u32,
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
}

/// Even count: the lower middle, as in `culprits::fault`.
fn median(mut v: Vec<u32>) -> Option<u32> {
    v.sort_unstable();
    v.get(v.len().checked_sub(1)? / 2).copied()
}

/// Representatives seen in a short observation, judged by the section 5 criteria.
fn live(env: &mut dyn Env, group: &Group) -> Score {
    let flows = env.observe(12);
    let mut s = Score::default();
    for rep in &group.representatives {
        let mut seen = flows.iter().filter(|f| f.dst == rep.dst).peekable();
        if seen.peek().is_none() {
            continue;
        }
        s.total += 1;
        if seen.all(|f| fault(f).is_none()) {
            s.ok += 1;
        }
    }
    s
}

/// The path's score, or `None` when canceled during it.
fn evaluate(env: &mut dyn Env, ctx: &Ctx, group: &Group) -> Option<Score> {
    let mut s = Score::default();
    let (mut medians, mut losses) = (vec![], vec![]);
    for probe in &ctx.profile.probes {
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
        losses.extend(p.loss_pct);
        if env.cancelled() {
            return None;
        }
    }
    s.median_ms = median(medians);
    s.loss_pct = losses.into_iter().max();
    Some(s)
}

pub fn run(env: &mut dyn Env, ctx: &Ctx, groups: &[Group], settings: &Settings) -> Outcome {
    let mut out = Outcome {
        per_group: vec![],
        canceled: false,
        budget_hit: false,
        rollback_error: None,
    };
    let mut stopped = false;
    for (gi, group) in groups.iter().enumerate() {
        let (paths, skipped) = plan(group, ctx);
        let mut gr = GroupResult {
            group: group.clone(),
            results: vec![],
            skipped,
            not_checked: vec![],
        };
        for (i, path) in paths.iter().enumerate() {
            let label = path.label();
            if !stopped && env.cancelled() {
                out.canceled = true;
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
            env.progress(Progress::Path {
                group: gi as u32,
                index: i as u32,
                total: paths.len() as u32,
                label: label.clone(),
            });
            if let Err(e) = env.set_path(path, group) {
                // Not working; the next path sets its own configuration.
                gr.not_checked
                    .push(format!("{label}: не удалось поставить: {e:#}"));
                gr.results.push((path.clone(), Score::default()));
                continue;
            }
            let Some(mut s) = evaluate(env, ctx, group) else {
                out.canceled = true;
                stopped = true;
                gr.not_checked.push(format!("{label}: прервано"));
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
            gr.results.push((path.clone(), s));
        }
        rank(&mut gr.results);
        out.per_group.push(gr);
    }
    env.progress(Progress::RollingBack);
    out.rollback_error = env.rollback().err().map(|e| format!("{e:#}"));
    out
}

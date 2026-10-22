//! Diagnostics in the app (diag spec 3–10): the job, the hub as `diag::run::Env`, the rollback,
//! the recheck of the other profiles and «Применить».

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs as _};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use diag::circular_store;
use diag::culprits::{self, Group, Verdict};
use diag::evaluate::{Answer, Score, works};
use diag::paths::{Ctx, PathKind};
use diag::patterns;
use diag::profiles::{self, ProbeKind, ProbeSpec, Profile};
use diag::report;
use diag::run::{Env, Live, Mode, Outcome, Progress, Request};
use diag::summary::{self, Fix, Summary};
use probe::curl::{CheckStatus, Via};
use router::RouterState;
use router::observe::{FlowCounters, Observer};
use router::rules::{Protocol, Rule};
use tauri::{AppHandle, Manager as _};
use tauri_specta::Event as _;
use tester::engine::{Launch, Running};
use zapret::folder::Folder;
use zapret::service::{SERVICE, SERVICE2};
use zapret::snapshot::{RouterSnap, Snapshot, pending_path};

use crate::actions::{self, state};
use crate::dto::{DiagFinished, DiagProgress, NoticeLevel};
use crate::state::{AppState, DiagEnds};
use crate::{notify, routing, texts, window};

mod apply;
mod evidence;

pub use apply::apply;
use apply::hub_text;
use evidence::{Marks, evidence};

/// What the page sends to diagnostics that wait for it.
pub enum Reply {
    Answer(Answer),
    /// Keys of the culprit groups to check.
    Choice(Vec<String>),
}

const RECHECK_CUT: &str = "перепроверка других профилей прервана: время диагностики вышло";
/// The page gets this long to untick culprits; then all are checked.
const CHOOSE_WAIT: Duration = Duration::from_secs(120);
/// "Works?" waits for the user or a cancel.
const ANSWER_WAIT: Duration = Duration::from_secs(24 * 60 * 60);
const UDP_TIMEOUT: Duration = Duration::from_secs(2);
const PING_TIMEOUT_MS: u32 = 2000;
const ATTEMPTS: usize = 3;
/// The name of the temporary rule of the proxy paths: never one of the user's.
const CHECK_RULE: &str = "diag-check";

fn profiles_dir(state: &AppState) -> PathBuf {
    state.paths.diag().join("profiles")
}

fn circular_path(state: &AppState) -> PathBuf {
    state.paths.diag().join("circular.toml")
}

pub fn profiles(state: &AppState) -> anyhow::Result<Vec<Profile>> {
    profiles::load(&profiles_dir(state))
}

fn profile_of(state: &AppState, request: &Request) -> anyhow::Result<Profile> {
    if let Some(exe) = &request.app {
        return Ok(profiles::any_program(exe));
    }
    let id = request.profile.as_deref().unwrap_or_default();
    profiles(state)?
        .into_iter()
        .find(|p| p.id == id)
        .with_context(|| format!("no diagnostics profile {id}"))
}

/// `question` or `choosing` changed after their step was reported: the step again, so the page
/// reads `diag_live` and shows or drops the card.
fn live_changed(app: &AppHandle, change: impl FnOnce(&mut Live)) {
    let state = state(app);
    state.update_diag_live(change);
    if let Some(step) = state.diag_live().and_then(|live| live.progress)
        && let Err(err) = DiagProgress(step).emit(app)
    {
        log::warn!("cannot report diagnostics progress: {err}");
    }
}

/// Starts diagnostics in their own thread; the page follows `DiagProgress` and `DiagFinished`.
pub fn start_diag(app: &AppHandle, request: Request) -> anyhow::Result<()> {
    request.check()?;
    let state = state(app);
    let profile = profile_of(&state, &request)?;
    state.begin_diag(Live {
        request: request.clone(),
        progress: None,
        question: None,
        choosing: None,
        results: Vec::new(),
    })?;
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = actions::state(&handle);
        let ends = state.diag_ends();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job(&handle, &request, profile)
        }))
        .unwrap_or_else(|panic| {
            let message = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
            Err(anyhow::anyhow!(
                "the diagnostics panicked: {}",
                message.unwrap_or("no message")
            ))
        });
        finish(&handle, outcome, ends);
    });
    actions::changed(app);
    Ok(())
}

fn window_hidden(app: &AppHandle) -> bool {
    app.get_webview_window(window::MAIN)
        .is_none_or(|w| !w.is_visible().unwrap_or(false))
}

fn progress(app: &AppHandle, step: Progress) {
    // A question is coming: a reply from before it must not answer it.
    if matches!(step, Progress::Question { .. } | Progress::Culprits { .. }) {
        state(app).clear_diag_reply();
    }
    state(app).update_diag_live(|live| live.progress = Some(step.clone()));
    if let Err(err) = DiagProgress(step).emit(app) {
        log::warn!("cannot report diagnostics progress: {err}");
    }
}

/// `ends` is dropped once, before `DiagFinished`: ending twice could end the next run.
fn finish(app: &AppHandle, outcome: anyhow::Result<Summary>, ends: DiagEnds<'_>) {
    let id = match outcome {
        Ok(summary) => {
            if let Some(err) = &summary.rollback_error {
                actions::notice(
                    app,
                    NoticeLevel::Error,
                    texts::RESTORE_AFTER_DIAG,
                    err.clone(),
                );
            }
            if window_hidden(app) {
                let title = if summary.canceled {
                    texts::DIAG_CANCELED
                } else {
                    texts::DIAG_DONE
                };
                notify::show(app, title, &summary.verdict_text);
            }
            Some(summary.id)
        }
        Err(err) => {
            let text = format!("{err:#}");
            if window_hidden(app) {
                notify::show(app, texts::DIAG_FAILED, &text);
            }
            actions::notice(app, NoticeLevel::Error, texts::DIAG_FAILED, text);
            None
        }
    };
    drop(ends);
    if let Err(err) = (DiagFinished { id }).emit(app) {
        log::warn!("cannot report the end of diagnostics: {err}");
    }
    actions::changed(app);
}

/// The programs' flows over `secs`; `show` reports them each second.
fn watch(
    app: &AppHandle,
    apps: &[String],
    secs: u32,
    show: bool,
) -> anyhow::Result<Vec<FlowCounters>> {
    let state = state(app);
    let dll = routing::dll(&state)?;
    let proxy_port = router::parse_proxy(state.socks5().as_deref())
        .ok()
        .map(|a| a.port());
    let observer = Observer::start(&dll, apps.to_vec(), proxy_port)?;
    for left in (0..secs).rev() {
        std::thread::sleep(Duration::from_secs(1));
        if state.diag_canceled() {
            break;
        }
        if show {
            progress(
                app,
                Progress::Observing {
                    secs_left: left,
                    flows: observer.snapshot(),
                },
            );
        }
    }
    Ok(observer.stop())
}

/// The groups to check: all of them in "auto", without a window, or when no choice comes in time.
fn choose(app: &AppHandle, mode: Mode, all: &[Group]) -> Vec<Group> {
    progress(
        app,
        Progress::Culprits {
            groups: all.to_vec(),
        },
    );
    if mode == Mode::Auto || app.get_webview_window(window::MAIN).is_none() {
        return all.to_vec();
    }
    let state = state(app);
    live_changed(app, |live| live.choosing = Some(all.to_vec()));
    if window_hidden(app) {
        notify::show(app, texts::DIAG_CHOOSE, texts::DIAG_CHOOSE_TEXT);
    }
    let keys = state.wait_diag_reply(CHOOSE_WAIT, |reply| match reply {
        Reply::Choice(keys) => Some(keys.clone()),
        Reply::Answer(_) => None,
    });
    live_changed(app, |live| live.choosing = None);
    picked(all, keys.as_deref(), state.diag_canceled())
}

/// No choice in time: all of them; a cancel: none (no snapshot, nothing restarted).
fn picked(all: &[Group], keys: Option<&[String]>, canceled: bool) -> Vec<Group> {
    match keys {
        _ if canceled => Vec::new(),
        Some(keys) => all
            .iter()
            .filter(|g| keys.contains(&g.key))
            .cloned()
            .collect(),
        None => all.to_vec(),
    }
}

/// The router section of a snapshot: whether routing was on and paused (diag spec 9).
pub fn router_after_restore(snapshot: &Snapshot) -> Option<(bool, bool)> {
    snapshot.router.as_ref().map(|r| (r.enabled, r.paused))
}

fn job(app: &AppHandle, request: &Request, profile: Profile) -> anyhow::Result<Summary> {
    let state = state(app);
    // The whole diagnostics are one turn of the queue, like a test run.
    let _turn = state.turn();
    let settings = state.settings();
    let folder = state.folder_if_set()?;
    let engine2 = state.engine2().ok().flatten();
    let observe_secs = request.observe_secs.unwrap_or(settings.diag_observe_secs);
    let run_settings = diag::run::Settings {
        observe_secs,
        budget: Duration::from_secs(
            u64::from(request.budget_min.unwrap_or(settings.diag_budget_min)) * 60,
        ),
        mode: request.mode,
    };
    let ctx = context(&state, folder.as_ref(), engine2.as_ref(), profile);
    let started = Instant::now();
    let marks = Marks::now(&settings);
    // Watching changes nothing: no snapshot yet.
    let flows = watch(app, &ctx.profile.apps, observe_secs, true)?;
    let verdict = culprits::verdict(&flows);
    let groups = match &verdict {
        Verdict::Culprits(all) if !state.diag_canceled() => choose(app, request.mode, all),
        _ => Vec::new(),
    };
    let pending = pending_path(&state.paths.data());
    let mut env = None;
    let outcome = if groups.is_empty() {
        Outcome {
            per_group: Vec::new(),
            canceled: state.diag_canceled(),
            budget_hit: false,
            rollback_error: None,
        }
    } else {
        let mut snapshot = zapret::snapshot::begin(folder.as_ref(), "diag", &pending)?;
        snapshot.router = Some(RouterSnap {
            enabled: settings.router_enabled,
            paused: state.router.status().state == RouterState::Paused,
        });
        snapshot.save(&pending)?;
        let env = env.insert(AppEnv {
            app: app.clone(),
            folder: folder.clone(),
            engine2: engine2.clone(),
            snapshot,
            pending: pending.clone(),
            started,
            apps: ctx.profile.apps.clone(),
            running: None,
            zapret_as_is: true,
            dirty: false,
        });
        diag::run::run(env, &ctx, &groups, &run_settings)
    };
    let (evidence, missing) = evidence(&state, &ctx, &flows, &outcome, &marks, !outcome.canceled);
    let patterns = patterns::detect(&evidence, &outcome, &ctx);
    let mut summary = summary::summarize(
        &verdict,
        &outcome,
        &patterns,
        &ctx,
        exec::LocalTime::now().stamp(),
        observe_secs,
    );
    summary.sources_missing = missing;
    if let Some(env) = env.as_mut()
        && !outcome.canceled
        && outcome.rollback_error.is_none()
    {
        summary.warnings = recheck(env, &state, &ctx, &summary.fixes, run_settings.budget);
        // Only when the recheck changed something: a rollback restarts the services.
        if env.dirty
            && let Err(err) = env.rollback()
        {
            summary.rollback_error = Some(format!("{err:#}"));
        }
    }
    // First: put back is done, and a report that cannot be saved must not leave it pending.
    if env.is_some() && summary.rollback_error.is_none() {
        Snapshot::clear(&pending)?;
    }
    report::save(&state.paths.diag(), &summary)?;
    Ok(summary)
}

/// The rules, strategies and ranking the paths are planned from.
fn context(
    state: &AppState,
    folder: Option<&Folder>,
    engine2: Option<&zapret2::Engine>,
    profile: Profile,
) -> Ctx {
    let proxy_ok = state.socks5().is_some_and(|addr| {
        router::parse_proxy(Some(&addr)).is_ok() && probe::socks::check(&addr).ok
    });
    let ranking = state.best(usize::MAX);
    let strategies = ranking
        .iter()
        .filter_map(|name| Some((name.clone(), args_of(folder, engine2, name).ok()?)))
        .collect();
    let zapret2 = engine2
        .and_then(|engine| zapret2::strategies::list(engine.root()).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.name != circular_store::NAME)
        .map(|s| (s.full_name(), s.text))
        .collect();
    let app_rules = state
        .router
        .rules()
        .rules
        .into_iter()
        .map(|row| row.rule)
        .filter(|rule| !rule.apps.is_empty())
        .collect();
    Ctx {
        proxy_ok,
        ranking,
        strategies,
        zapret2,
        app_rules,
        profile,
    }
}

/// What `strategy_covers` reads: Flowseal's final args, zapret2's args.
fn args_of(
    folder: Option<&Folder>,
    engine2: Option<&zapret2::Engine>,
    name: &str,
) -> anyhow::Result<String> {
    match zapret2::strip_prefix(name) {
        Some(short) => {
            let engine = engine2.context("the zapret2 folder is not set")?;
            let strategy = zapret2::strategies::find(engine.root(), short)?;
            Ok(zapret2::strategies::args(&strategy.text))
        }
        None => {
            let folder = folder.context(zapret::folder::NOT_SET)?;
            let file = zapret::strategies::find(folder, name)?;
            Ok(zapret::batparse::parse_file(folder, &file.path)?.final_args)
        }
    }
}

/// The hub as the diagnostics see it: paths set through the services, winws and the router.
struct AppEnv {
    app: AppHandle,
    folder: Option<Folder>,
    engine2: Option<zapret2::Engine>,
    snapshot: Snapshot,
    pending: PathBuf,
    started: Instant,
    apps: Vec<String>,
    /// The engine a strategy or trick path started.
    running: Option<Running>,
    /// The services and winws are as the snapshot found them.
    zapret_as_is: bool,
    /// Something changed since the last rollback.
    dirty: bool,
}

impl AppEnv {
    fn state(&self) -> tauri::State<'_, AppState> {
        state(&self.app)
    }

    fn stop_running(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }

    /// The services and winws as they were: the snapshot puts them back.
    fn zapret_as_is(&mut self) -> anyhow::Result<()> {
        if !self.zapret_as_is {
            self.snapshot.restore(self.folder.as_ref())?;
            self.zapret_as_is = true;
        }
        Ok(())
    }

    /// No zapret: both services stopped (the snapshot restarts them), every winws ended.
    fn zapret_off(&mut self) -> anyhow::Result<()> {
        if self.zapret_as_is {
            // First: a stop that fails halfway must still be undone by `zapret_as_is`.
            self.zapret_as_is = false;
            for service in [SERVICE, SERVICE2] {
                zapret::service::stop(service)?;
            }
            // As a test run does: strategies with `fooling=ts` need them.
            if zapret::service::tcp_timestamps() == Some(false)
                && let Err(err) = zapret::service::enable_tcp_timestamps()
            {
                log::warn!("TCP timestamps stay off for the diagnostics: {err:#}");
            }
        }
        tester::engine::stop_engines()
    }

    fn start(&mut self, launch: anyhow::Result<Launch>) -> anyhow::Result<()> {
        self.zapret_off()?;
        // The engine may bring its own driver: the router lets go of it first.
        self.state().router.pause();
        self.running = Some(tester::engine::start(&launch?)?);
        Ok(())
    }

    /// Routing as the user left it, without the check rule.
    fn router_as_is(&self) {
        let state = self.state();
        routing::sync(&self.app);
        let paused = router_after_restore(&self.snapshot).is_some_and(|(_, paused)| paused);
        if paused {
            state.router.pause();
        } else {
            state.router.resume();
        }
    }

    /// The router running with `rule` on top. The rule also takes the hub and curl, and the
    /// addresses the probes pin, so the probes go the way the program would.
    fn router_with(&self, mut rule: Rule, group: &Group) -> anyhow::Result<()> {
        let state = self.state();
        rule.name = CHECK_RULE.into();
        rule.enabled = true;
        if let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        {
            rule.apps.push(exe);
        }
        rule.apps.push("curl.exe".into());
        for ip in group.representatives.iter().filter_map(ipv4_of) {
            let ip = ip.to_string();
            if !rule.ips.contains(&ip) {
                rule.ips.push(ip);
            }
        }
        state.router.set_temporary(Some(rule))?;
        state.router.start(routing::setup(&state)?)?;
        state.router.resume();
        match state.router.status().state {
            RouterState::Running => Ok(()),
            RouterState::Unavailable { reason } => {
                anyhow::bail!("the router did not start: {reason}")
            }
            other => anyhow::bail!("the router did not start: {other:?}"),
        }
    }

    /// The services, winws and routing as the snapshot found them; the check rule gone.
    fn put_back(&mut self) -> anyhow::Result<()> {
        self.stop_running();
        let restored = self.snapshot.restore(self.folder.as_ref());
        self.zapret_as_is = true;
        let rule = self.state().router.set_temporary(None);
        self.router_as_is();
        self.dirty = false;
        restored.and(rule)
    }
}

impl Env for AppEnv {
    fn set_path(&mut self, path: &PathKind, group: &Group) -> anyhow::Result<()> {
        self.dirty = true;
        self.stop_running();
        self.state().router.set_temporary(None)?;
        match path {
            PathKind::Current => {
                self.zapret_as_is()?;
                self.router_as_is();
                Ok(())
            }
            PathKind::Direct => {
                self.zapret_off()?;
                self.state().router.pause();
                Ok(())
            }
            PathKind::ProxyPoint { rule } => {
                self.zapret_as_is()?;
                self.router_with(rule.clone(), group)
            }
            PathKind::ProxyNoZapret { rule } => {
                self.zapret_off()?;
                self.router_with(rule.clone(), group)
            }
            PathKind::ExtendRule { rule, protocol } => {
                self.zapret_as_is()?;
                let found = self
                    .state()
                    .router
                    .rules()
                    .rules
                    .into_iter()
                    .map(|row| row.rule)
                    .find(|r| &r.name == rule)
                    .with_context(|| format!("no rule named {rule}"))?;
                self.router_with(
                    Rule {
                        protocol: *protocol,
                        ..found
                    },
                    group,
                )
            }
            PathKind::Strategy { name } => {
                let launch =
                    tester::engine::launch(self.folder.as_ref(), self.engine2.as_ref(), name);
                self.start(launch)
            }
            PathKind::Trick { candidate } => {
                let launch = self
                    .engine2
                    .as_ref()
                    .context("the zapret2 folder is not set: choose it in Settings")
                    .and_then(|engine| {
                        tester::engine::launch_text(engine, self.folder.as_ref(), &candidate.text)
                    });
                self.start(launch)
            }
        }
    }

    fn probe(&mut self, group: &Group, probe: &ProbeSpec) -> Score {
        attempts(group, probe)
    }

    fn ask(&mut self, hint: &str, path: &PathKind) -> Answer {
        let state = self.state();
        let question = texts::diag_question(hint, &path.label());
        live_changed(&self.app, |live| live.question = Some(question.clone()));
        if window_hidden(&self.app) {
            notify::show(&self.app, texts::DIAG_QUESTION, &question);
        }
        let answer = state.wait_diag_reply(ANSWER_WAIT, |reply| match reply {
            Reply::Answer(answer) => Some(answer.clone()),
            Reply::Choice(_) => None,
        });
        live_changed(&self.app, |live| live.question = None);
        answer.unwrap_or(Answer::Unknown)
    }

    fn observe(&mut self, secs: u32) -> Vec<FlowCounters> {
        watch(&self.app, &self.apps, secs, false).unwrap_or_else(|err| {
            log::warn!("diagnostics: cannot watch the program: {err:#}");
            Vec::new()
        })
    }

    fn rollback(&mut self) -> anyhow::Result<()> {
        self.put_back()
    }

    fn cancelled(&self) -> bool {
        self.state().diag_canceled()
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn progress(&mut self, step: Progress) {
        progress(&self.app, step);
    }

    fn results(&mut self, results: &[(PathKind, Score)]) {
        live_changed(&self.app, |live| live.results = results.to_vec());
    }
}

/// A panic halfway still puts zapret and routing back and, once they are, removes
/// `pending.json` as the normal end does; failing that, it stays for the next start.
impl Drop for AppEnv {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }
        match self.put_back() {
            Ok(()) => {
                if let Err(err) = Snapshot::clear(&self.pending) {
                    log::error!("diagnostics: {err:#}");
                }
            }
            Err(err) => log::error!("diagnostics: the state was not put back: {err:#}"),
        }
    }
}

fn ipv4_of(f: &FlowCounters) -> Option<Ipv4Addr> {
    match f.dst.parse() {
        Ok(SocketAddr::V4(a)) => Some(*a.ip()),
        _ => None,
    }
}

/// Where one attempt goes: a host (pinned to an address or not) or a UDP endpoint.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Https { host: String, ip: Option<IpAddr> },
    Udp(SocketAddr),
    Ping(Ipv4Addr),
}

/// One target per representative (or one, for a named host); none when the probe does not fit
/// the group: HTTPS is for TCP, IP discovery and STUN for UDP.
fn targets(group: &Group, probe: &ProbeSpec) -> Vec<Target> {
    let observed = probe.target == "observed";
    let reps = &group.representatives;
    match probe.kind {
        ProbeKind::Https if group.protocol != Protocol::Udp => {
            // The culprit itself (spec 7): a representative by its own name and address; one
            // without a name gets the profile's host pinned to its address, so a valid SNI
            // reaches the culprit. Observed probes have no host to lend.
            reps.iter()
                .filter_map(|r| {
                    let host = r
                        .sni
                        .clone()
                        .or_else(|| r.domain.clone())
                        .or_else(|| (!observed).then(|| probe.target.clone()))?;
                    Some(Target::Https {
                        host,
                        ip: ipv4_of(r).map(IpAddr::V4),
                    })
                })
                .collect()
        }
        ProbeKind::DiscordIpDiscovery | ProbeKind::Stun if group.protocol != Protocol::Tcp => {
            if observed {
                reps.iter()
                    .filter_map(|r| r.dst.parse().ok().map(Target::Udp))
                    .collect()
            } else {
                probe
                    .target
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .map(Target::Udp)
                    .into_iter()
                    .collect()
            }
        }
        ProbeKind::Ping => reps.iter().filter_map(ipv4_of).map(Target::Ping).collect(),
        _ => Vec::new(),
    }
}

/// One attempt: the round trip in ms (0 when unmeasured), or `None` without an answer.
fn attempt(kind: ProbeKind, target: &Target) -> Option<u32> {
    match target {
        Target::Https { host, ip } => {
            let r = probe::curl::https_via(host, *ip, &Via::Direct, None);
            (r.status == CheckStatus::Ok).then(|| r.time_ms.unwrap_or(0))
        }
        Target::Udp(dst) if kind == ProbeKind::Stun => probe::udp::stun(*dst, UDP_TIMEOUT),
        Target::Udp(dst) => probe::udp::ip_discovery(*dst, 1, UDP_TIMEOUT),
        Target::Ping(ip) => probe::ping::ping(&ip.to_string(), PING_TIMEOUT_MS),
    }
}

/// 3 attempts over the targets in turn. HTTPS measures no loss; the others count the misses.
fn attempts(group: &Group, probe: &ProbeSpec) -> Score {
    let targets = targets(group, probe);
    if targets.is_empty() {
        return Score::default();
    }
    let answers: Vec<Option<u32>> = (0..ATTEMPTS)
        .map(|i| attempt(probe.kind, &targets[i % targets.len()]))
        .collect();
    score(probe.kind, &answers)
}

fn score(kind: ProbeKind, answers: &[Option<u32>]) -> Score {
    let mut times: Vec<u32> = answers.iter().flatten().copied().collect();
    times.sort_unstable();
    let ok = times.len() as u32;
    let total = answers.len() as u32;
    Score {
        ok,
        total,
        // Even count: the lower middle.
        median_ms: times.get(times.len().saturating_sub(1) / 2).copied(),
        loss_pct: (kind != ProbeKind::Https && total > 0).then(|| (total - ok) * 100 / total),
        answer: None,
    }
}

/// `(<profile>: <host>, works)` for the other profiles' HTTPS probes to a named host (the
/// addresses they would observe are not known here).
fn other_probes(profiles: &[Profile]) -> Vec<(String, ProbeSpec)> {
    profiles
        .iter()
        .flat_map(|p| {
            p.probes
                .iter()
                .filter(|s| s.kind == ProbeKind::Https && s.target != "observed")
                .map(move |s| (format!("{}: {}", p.name, s.target), s.clone()))
        })
        .collect()
}

/// Side by side: one pass waits for the slowest probe's timeouts, not for all of them.
fn probe_all(probes: &[(String, ProbeSpec)]) -> Vec<(String, bool)> {
    std::thread::scope(|scope| {
        let passes: Vec<_> = probes
            .iter()
            .map(|(key, spec)| {
                let pass = scope.spawn(move || {
                    let target = Target::Https {
                        host: spec.target.clone(),
                        ip: None,
                    };
                    let answers: Vec<Option<u32>> =
                        (0..ATTEMPTS).map(|_| attempt(spec.kind, &target)).collect();
                    works(&score(spec.kind, &answers))
                });
                (key, pass)
            })
            .collect();
        passes
            .into_iter()
            .map(|(key, pass)| (key.clone(), pass.join().unwrap_or(false)))
            .collect()
    })
}

/// Spec 8: the other profiles as they are and under each fix that changes the strategy; what
/// works now and stops under a fix is a warning. Rules affect only their programs: not checked.
fn recheck(
    env: &mut AppEnv,
    state: &AppState,
    ctx: &Ctx,
    fixes: &[Fix],
    budget: Duration,
) -> Vec<String> {
    let profile = &ctx.profile;
    let candidates: Vec<(String, anyhow::Result<Launch>)> = fixes
        .iter()
        .filter_map(|fix| match fix {
            Fix::ApplyStrategy { name } => Some((
                name.clone(),
                tester::engine::launch(env.folder.as_ref(), env.engine2.as_ref(), name),
            )),
            Fix::ApplyHubCircular { tricks, filter } => {
                let launch = env
                    .engine2
                    .as_ref()
                    .context("the zapret2 folder is not set")
                    .and_then(|engine| {
                        let mut store = circular_store::load(&circular_path(state))?;
                        circular_store::add(&mut store, filter, tricks.clone());
                        let text = hub_text(engine, &store)?;
                        tester::engine::launch_text(engine, env.folder.as_ref(), &text)
                    });
                Some((
                    format!("{}{}", zapret2::PREFIX, circular_store::NAME),
                    launch,
                ))
            }
            _ => None,
        })
        .collect();
    if candidates.is_empty() || env.cancelled() {
        return Vec::new();
    }
    let others: Vec<Profile> = profiles(state)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.id != profile.id)
        .collect();
    let probes = other_probes(&others);
    if probes.is_empty() {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let over = |env: &AppEnv, warnings: &mut Vec<String>| {
        let over = env.elapsed() >= budget;
        if over {
            warnings.push(RECHECK_CUT.into());
        }
        over
    };
    if over(env, &mut warnings) {
        return warnings;
    }
    env.progress(Progress::Rechecking);
    // After the run's rollback: as the user has it.
    let before = probe_all(&probes);
    for (name, launch) in candidates {
        if env.cancelled() || over(env, &mut warnings) {
            break;
        }
        env.dirty = true;
        env.stop_running();
        if let Err(err) = env.start(launch) {
            warnings.push(format!(
                "{name}: не проверено, как она скажется на других профилях: {err:#}"
            ));
            continue;
        }
        // Starting the strategy took time too.
        if env.cancelled() || over(env, &mut warnings) {
            break;
        }
        let after = probe_all(&probes);
        warnings.extend(
            summary::warn_breaks(&before, &after)
                .into_iter()
                .map(|w| format!("{name} — {w}")),
        );
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::apply::{BASE_LINE, base, circular_base};
    use super::evidence::{differ, foreign_syn_dead};
    use diag::culprits::Fault;

    use super::*;

    fn flow(dst: &str, sni: Option<&str>) -> FlowCounters {
        FlowCounters {
            protocol: Protocol::Tcp,
            local_port: 50000,
            dst: dst.into(),
            domain: None,
            sni: sni.map(str::to_owned),
            ipv6: false,
            via_system_proxy: false,
            up_packets: 5,
            down_packets: 0,
            up_bytes: 0.0,
            down_bytes: 0.0,
            syns: 3,
            syn_answered: false,
            syn_ack_ms: None,
            rst_early: false,
            redirect_early: false,
            data_retrans: 0,
            udp_first_reply_ms: None,
            udp_windows: 0,
            udp_silent_windows: 0,
            rtt_ms: Vec::new(),
        }
    }

    fn group(protocol: Protocol, reps: Vec<FlowCounters>) -> Group {
        Group {
            key: "discord.com".into(),
            protocol,
            ports: vec![443],
            members: reps.clone(),
            representatives: reps,
            fault: Fault::SynUnanswered { syns: 3 },
        }
    }

    fn spec(kind: ProbeKind, target: &str) -> ProbeSpec {
        ProbeSpec {
            kind,
            target: target.into(),
        }
    }

    #[test]
    fn recover_restores_router_section() {
        let mut snapshot = Snapshot {
            operation: "diag".into(),
            service: None,
            strategy: None,
            manual_winws: Vec::new(),
            service2: None,
            strategy2: None,
            manual_winws2: Vec::new(),
            zapret2: true,
            ipset: None,
            ipset_backup: false,
            game_filter: None,
            router: None,
        };
        assert_eq!(
            router_after_restore(&snapshot),
            None,
            "an older pending.json"
        );
        snapshot.router = Some(RouterSnap {
            enabled: true,
            paused: false,
        });
        assert_eq!(router_after_restore(&snapshot), Some((true, false)));
    }

    #[test]
    fn probes_fit_the_group() {
        let reps = vec![
            flow("162.159.1.1:443", Some("discord.com")),
            flow("162.159.1.2:443", None),
        ];
        let tcp = group(Protocol::Tcp, reps.clone());
        // Every representative is probed at its own address; a nameless one under the named host.
        assert_eq!(
            targets(&tcp, &spec(ProbeKind::Https, "discord.gg")),
            [
                Target::Https {
                    host: "discord.com".into(),
                    ip: Some("162.159.1.1".parse().unwrap())
                },
                Target::Https {
                    host: "discord.gg".into(),
                    ip: Some("162.159.1.2".parse().unwrap())
                }
            ]
        );
        // Observed: representatives with a name only.
        assert_eq!(targets(&tcp, &spec(ProbeKind::Https, "observed")).len(), 1);
        assert!(targets(&tcp, &spec(ProbeKind::Stun, "observed")).is_empty());
        let udp = group(Protocol::Udp, reps);
        assert!(targets(&udp, &spec(ProbeKind::Https, "discord.com")).is_empty());
        assert_eq!(
            targets(&udp, &spec(ProbeKind::DiscordIpDiscovery, "observed")),
            [
                Target::Udp("162.159.1.1:443".parse().unwrap()),
                Target::Udp("162.159.1.2:443".parse().unwrap())
            ]
        );
        assert_eq!(targets(&udp, &spec(ProbeKind::Ping, "observed")).len(), 2);
    }

    #[test]
    fn a_named_probe_hits_the_culprit() {
        let tcp = Group {
            key: "x.googlevideo.com".into(),
            ..group(
                Protocol::Tcp,
                vec![flow("173.194.1.5:443", Some("x.googlevideo.com"))],
            )
        };
        assert_eq!(
            targets(&tcp, &spec(ProbeKind::Https, "www.youtube.com")),
            [Target::Https {
                host: "x.googlevideo.com".into(),
                ip: Some("173.194.1.5".parse().unwrap())
            }]
        );
    }

    #[test]
    fn a_score_of_three_attempts() {
        let s = score(ProbeKind::Stun, &[Some(40), None, Some(20)]);
        assert_eq!(
            (s.ok, s.total, s.median_ms, s.loss_pct),
            (2, 3, Some(20), Some(33))
        );
        assert!(works(&s));
        let s = score(ProbeKind::Https, &[None, None, Some(90)]);
        assert_eq!((s.ok, s.loss_pct), (1, None));
        assert!(!works(&s));
    }

    #[test]
    fn dns_answers_differ_only_across_networks() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert!(differ(&[ip("10.10.34.35")], &[ip("162.159.137.232")]));
        assert!(
            differ(&[], &[ip("162.159.137.232")]),
            "an empty plain answer"
        );
        assert!(!differ(&[ip("162.159.128.233")], &[ip("162.159.137.232")]));
        assert!(!differ(&[ip("1.2.3.4")], &[]), "DoH said nothing");
    }

    #[test]
    fn only_ticked_groups_go_on() {
        let reps = vec![flow("1.1.1.1:443", None)];
        let a = group(Protocol::Tcp, reps.clone());
        let b = Group {
            key: "1.2.3.0/24".into(),
            ..group(Protocol::Udp, reps)
        };
        let all = [a.clone(), b.clone()];
        assert_eq!(picked(&all, None, false), all);
        assert_eq!(picked(&all, Some(&["1.2.3.0/24".to_owned()]), false), [b]);
        assert!(picked(&all, Some(&[]), false).is_empty());
        assert!(
            picked(&all, None, true).is_empty(),
            "canceled while choosing"
        );
    }

    #[test]
    fn a_syn_dead_network_only_when_every_group_is() {
        use diag::run::GroupResult;
        let reps = vec![flow("1.1.1.1:443", None)];
        let dead = GroupResult {
            group: group(Protocol::Tcp, reps.clone()),
            results: vec![(PathKind::Current, Score::default())],
            skipped: Vec::new(),
            not_checked: Vec::new(),
        };
        let outcome = |per_group| Outcome {
            per_group,
            canceled: false,
            budget_hit: false,
            rollback_error: None,
        };
        assert_eq!(foreign_syn_dead(&outcome(vec![])), None);
        assert_eq!(foreign_syn_dead(&outcome(vec![dead.clone()])), Some(true));
        let mut alive = dead.clone();
        alive.results[0].1 = Score {
            ok: 3,
            total: 3,
            ..Score::default()
        };
        assert_eq!(foreign_syn_dead(&outcome(vec![dead, alive])), Some(false));
    }

    #[test]
    fn hub_circular_builds_on_the_strategy_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for file in [
            "winws2.exe",
            "cygwin1.dll",
            "WinDivert.dll",
            "WinDivert64.sys",
        ] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        std::fs::create_dir_all(root.join("lua")).unwrap();
        std::fs::write(
            root.join("lua").join("zapret-lib.lua"),
            "NFQWS2_COMPAT_VER_REQUIRED=6
",
        )
        .unwrap();
        std::fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
        let engine = zapret2::Engine::open(root).unwrap();
        zapret2::strategies::write(
            root,
            "mine",
            "--filter-tcp=443
",
        )
        .unwrap();
        let name = |active: Option<&str>| base(&engine, active).0;
        assert_eq!(name(None), "");
        assert_eq!(name(Some("zapret2:mine")), "mine");
        assert_eq!(name(Some("general")), "general (Flowseal)");
        assert_eq!(name(Some("general (ALT)")), "general (ALT)");
        assert_eq!(name(Some("my own flowseal")), "", "no copy of it");
        assert!(circular_base(&engine, Some("my own flowseal")).is_err());
        assert!(
            circular_base(&engine, None).is_ok(),
            "nothing active, nothing lost"
        );
        assert!(circular_base(&engine, Some("general")).is_ok());
        // hub-circular names its base, and is never its own.
        let hub = format!(
            "{BASE_LINE}mine
--filter-udp=443
"
        );
        zapret2::strategies::write(root, circular_store::NAME, &hub).unwrap();
        assert_eq!(name(Some("zapret2:hub-circular")), "mine");
        let own = format!(
            "{BASE_LINE}hub-circular
"
        );
        zapret2::strategies::write(root, circular_store::NAME, &own).unwrap();
        assert_eq!(name(Some("zapret2:hub-circular")), "");
    }

    #[test]
    fn other_profiles_are_probed_by_named_hosts() {
        let profiles = profiles::builtin();
        let keys: Vec<String> = other_probes(&profiles)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            keys,
            [
                "Discord: discord.com",
                "Dead by Daylight: steam.live.bhvrdbd.com",
                "YouTube: www.youtube.com"
            ]
        );
    }
}

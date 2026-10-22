use std::collections::HashSet;
use std::time::Duration;

use diag::culprits::{Fault, Group};
use diag::evaluate::{Answer, Score, rank, works};
use diag::paths::{Ctx, NO_TRICKS_FOR_SYN, PathKind, plan};
use diag::profiles::{ProbeKind, ProbeSpec, Profile};
use diag::run::{Env, Mode, Outcome, Progress, Request, SYN_DEAD, Settings, run};
use router::observe::FlowCounters;
use router::rules::{Protocol, Rule};

const ALT11: &str = include_str!("fixtures/ALT11.txt");
const DBD: &str = "DeadByDaylight-Win64-Shipping.exe";

fn group(protocol: Protocol, port: u16) -> Group {
    Group {
        key: "1.2.3.0/24".into(),
        protocol,
        ports: vec![port],
        members: vec![],
        representatives: vec![],
        fault: Fault::ResetEarly,
    }
}

fn profile(kind: ProbeKind) -> Profile {
    Profile {
        id: "dbd".into(),
        name: "Dead by Daylight".into(),
        apps: vec![DBD.into()],
        hint: "начни поиск матча".into(),
        domains: vec![],
        probes: vec![ProbeSpec {
            kind,
            target: "observed".into(),
        }],
    }
}

fn ctx(proxy_ok: bool) -> Ctx {
    Ctx {
        proxy_ok,
        ranking: vec!["A".into(), "B".into(), "C".into()],
        strategies: vec![
            ("A".into(), "--filter-tcp=443 --dpi-desync=fake".into()),
            ("B".into(), "--filter-udp=443 --dpi-desync=fake".into()),
            ("C".into(), "--filter-tcp=80,443 --dpi-desync=split".into()),
        ],
        zapret2: vec![("zapret2/ALT11".into(), ALT11.into())],
        app_rules: vec![],
        profile: profile(ProbeKind::Https),
    }
}

fn order(p: &PathKind) -> u32 {
    match p {
        PathKind::Current => 0,
        PathKind::Direct => 1,
        PathKind::ProxyPoint { .. } => 2,
        PathKind::ProxyNoZapret { .. } => 3,
        PathKind::ExtendRule { .. } => 4,
        PathKind::Strategy { .. } => 5,
        PathKind::Trick { .. } => 6,
    }
}

#[derive(Default)]
struct Fake {
    works: HashSet<String>,
    cancel_at: Option<usize>,
    fail: Option<String>,
    answer: Option<Answer>,
    flows: Vec<FlowCounters>,
    sets: Vec<String>,
    rolled_back: u32,
    clock: Duration,
    /// How many finished paths each `results` call carried.
    results: Vec<usize>,
}

impl Env for Fake {
    fn set_path(&mut self, path: &PathKind, _: &Group) -> anyhow::Result<()> {
        self.sets.push(path.label());
        if self.fail.as_deref() == Some(&path.label()) {
            anyhow::bail!("winws не запустился");
        }
        Ok(())
    }
    fn probe(&mut self, _: &Group, _: &ProbeSpec) -> Score {
        self.clock += Duration::from_secs(10);
        let ok = if self.works.contains(self.sets.last().unwrap()) {
            3
        } else {
            0
        };
        Score {
            ok,
            total: 3,
            median_ms: Some(50),
            loss_pct: None,
            answer: None,
        }
    }
    fn ask(&mut self, _: &str, _: &PathKind) -> Answer {
        self.answer.clone().unwrap()
    }
    fn observe(&mut self, _: u32) -> Vec<FlowCounters> {
        self.flows.clone()
    }
    fn rollback(&mut self) -> anyhow::Result<()> {
        self.rolled_back += 1;
        Ok(())
    }
    fn cancelled(&self) -> bool {
        self.cancel_at.is_some_and(|n| self.sets.len() >= n)
    }
    fn elapsed(&self) -> Duration {
        self.clock
    }
    fn progress(&mut self, _: Progress) {}
    fn results(&mut self, results: &[(PathKind, Score)]) {
        self.results.push(results.len());
    }
}

fn settings(mode: Mode, budget_secs: u64) -> Settings {
    Settings {
        observe_secs: 30,
        budget: Duration::from_secs(budget_secs),
        mode,
    }
}

fn go(env: &mut Fake, mode: Mode, budget_secs: u64) -> Outcome {
    run(
        env,
        &ctx(true),
        &[group(Protocol::Tcp, 443)],
        &settings(mode, budget_secs),
    )
}

#[test]
fn order_follows_spec() {
    let (paths, skipped) = plan(&group(Protocol::Tcp, 443), &ctx(true));
    let kinds: Vec<u32> = paths.iter().map(order).collect();
    assert_eq!(kinds[..4], [0, 1, 2, 3]);
    assert!(kinds.is_sorted());
    let strategies: Vec<&str> = paths
        .iter()
        .filter_map(|p| match p {
            PathKind::Strategy { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(strategies, ["A", "C"]);
    assert!(kinds.contains(&6));
    assert!(skipped.iter().any(|s| s.contains('B')));
    let PathKind::ProxyPoint { rule } = &paths[2] else {
        panic!()
    };
    assert_eq!(rule.name, "diag-dbd");
    assert_eq!(rule.ips, ["1.2.3.0/24"]);
    assert_eq!(rule.ports, ["443"]);
    assert!(
        rule.apps.is_empty(),
        "the address picks the culprit, not the program"
    );
}

#[test]
fn no_tricks_for_an_unanswered_syn() {
    let g = Group {
        fault: Fault::SynUnanswered { syns: 3 },
        ..group(Protocol::Tcp, 443)
    };
    let (paths, skipped) = plan(&g, &ctx(true));
    assert!(!paths.iter().any(|p| matches!(p, PathKind::Trick { .. })));
    assert!(skipped.iter().any(|s| s == NO_TRICKS_FOR_SYN));
    assert!(paths.iter().any(|p| matches!(p, PathKind::Strategy { .. })));
}

#[test]
fn proxy_paths_skipped_when_proxy_down() {
    let (paths, skipped) = plan(&group(Protocol::Tcp, 443), &ctx(false));
    assert!(!paths.iter().any(|p| matches!(
        p,
        PathKind::ProxyPoint { .. } | PathKind::ProxyNoZapret { .. }
    )));
    assert!(skipped.iter().any(|s| s.contains("прокси не отвечает")));
}

#[test]
fn extend_rule_for_udp_only_app_rule() {
    let mut c = ctx(true);
    c.app_rules = vec![Rule {
        name: "DBD Process".into(),
        enabled: true,
        protocol: Protocol::Udp,
        ports: vec![],
        ips: vec![],
        domains: vec![],
        apps: vec![DBD.to_lowercase()],
    }];
    let (paths, _) = plan(&group(Protocol::Tcp, 443), &c);
    assert!(paths.contains(&PathKind::ExtendRule {
        rule: "DBD Process".into(),
        protocol: Protocol::Both,
    }));
    let (paths, _) = plan(&group(Protocol::Udp, 443), &c);
    assert!(
        !paths
            .iter()
            .any(|p| matches!(p, PathKind::ExtendRule { .. }))
    );
}

fn score(ok: u32, total: u32, median: u32) -> Score {
    Score {
        ok,
        total,
        median_ms: Some(median),
        loss_pct: None,
        answer: None,
    }
}

#[test]
fn two_of_three_works() {
    assert!(works(&score(2, 3, 0)));
    assert!(!works(&score(1, 3, 0)));
    assert!(!works(&score(0, 0, 0)));
}

#[test]
fn rank_prefers_pointed_within_15pct() {
    let strategy = PathKind::Strategy { name: "A".into() };
    let (point, _) = plan(&group(Protocol::Tcp, 443), &ctx(true));
    let point = point[2].clone();
    let mut r = vec![
        (PathKind::Direct, score(0, 3, 10)),
        (strategy.clone(), score(3, 3, 100)),
        (point.clone(), score(3, 3, 110)),
    ];
    rank(&mut r);
    assert_eq!(
        [&r[0].0, &r[1].0, &r[2].0],
        [&point, &strategy, &PathKind::Direct]
    );
    let mut r = vec![
        (point.clone(), score(3, 3, 120)),
        (strategy.clone(), score(3, 3, 100)),
    ];
    rank(&mut r);
    assert_eq!(r[0].0, strategy);
    let lossy = |p: &PathKind, loss| {
        (
            p.clone(),
            Score {
                loss_pct: Some(loss),
                ..score(3, 3, 100)
            },
        )
    };
    let mut r = vec![lossy(&point, 40), lossy(&strategy, 0)];
    rank(&mut r);
    assert_eq!(r[0].0, strategy);
}

#[test]
fn budget_stops_and_lists_rest() {
    let mut env = Fake::default();
    let out = go(&mut env, Mode::Auto, 25);
    assert!(out.budget_hit);
    let g = &out.per_group[0];
    assert_eq!(g.results.len(), 3);
    assert!(g.not_checked.iter().any(|s| s.contains("Стратегия A")));
    assert_eq!(env.rolled_back, 1);
}

#[test]
fn cancel_mid_path_rolls_back_and_keeps_partial() {
    let mut env = Fake {
        works: HashSet::from(["Как сейчас".to_owned()]),
        cancel_at: Some(2),
        ..Fake::default()
    };
    let out = go(&mut env, Mode::Auto, 600);
    assert!(out.canceled);
    assert_eq!(env.rolled_back, 1);
    let g = &out.per_group[0];
    assert_eq!(g.results, [(PathKind::Current, score(3, 3, 50))]);
    assert!(g.not_checked.iter().any(|s| s.contains("Напрямую")));
}

#[test]
fn set_path_error_recorded_and_rollback_still_runs() {
    let mut env = Fake {
        fail: Some("Напрямую мимо всего".into()),
        ..Fake::default()
    };
    let out = go(&mut env, Mode::Auto, 600);
    assert_eq!(env.rolled_back, 1);
    let g = &out.per_group[0];
    assert!(g.results.len() > 3);
    assert!(env.sets.iter().any(|s| s == "Прокси точечно"));
    let failed = g
        .results
        .iter()
        .find(|(p, _)| *p == PathKind::Direct)
        .unwrap();
    assert_eq!(failed.1, Score::default());
    assert!(!works(&failed.1));
    assert!(
        g.not_checked
            .iter()
            .any(|s| s.contains("Напрямую")
                && s.contains("не удалось поставить: winws не запустился"))
    );
}

#[test]
fn ask_mode_answer_overrides_probes() {
    let mut env = Fake {
        works: HashSet::from(["Как сейчас".to_owned()]),
        answer: Some(Answer::No),
        cancel_at: Some(2),
        ..Fake::default()
    };
    let out = go(&mut env, Mode::Ask, 600);
    let (_, s) = &out.per_group[0].results[0];
    assert_eq!((s.ok, s.answer.clone()), (3, Some(Answer::No)));
    assert!(!works(s));
}

#[test]
fn live_probe_judges_representatives() {
    let rep = |dst: &str| FlowCounters {
        protocol: Protocol::Udp,
        local_port: 50000,
        dst: dst.into(),
        domain: None,
        sni: None,
        ipv6: false,
        via_system_proxy: false,
        up_packets: 10,
        down_packets: 10,
        up_bytes: 0.0,
        down_bytes: 0.0,
        syns: 0,
        syn_answered: false,
        syn_ack_ms: None,
        rst_early: false,
        redirect_early: false,
        data_retrans: 0,
        udp_first_reply_ms: None,
        udp_windows: 0,
        udp_silent_windows: 0,
        rtt_ms: vec![],
    };
    let mut g = group(Protocol::Udp, 7777);
    g.representatives = vec![rep("1.2.3.4:7777"), rep("1.2.3.5:7777")];
    let mut c = ctx(true);
    c.profile = profile(ProbeKind::Live);
    let broken = FlowCounters {
        down_packets: 0,
        ..rep("1.2.3.5:7777")
    };
    let mut env = Fake {
        flows: vec![rep("1.2.3.4:7777"), broken, rep("9.9.9.9:1")],
        cancel_at: Some(2),
        ..Fake::default()
    };
    let out = run(&mut env, &c, &[g.clone()], &settings(Mode::Auto, 600));
    assert_eq!(out.per_group[0].results[0].1.ok, 1);
    assert_eq!(out.per_group[0].results[0].1.total, 2);

    let mut env = Fake {
        cancel_at: Some(2),
        ..Fake::default()
    };
    let out = run(&mut env, &c, &[g], &settings(Mode::Auto, 600));
    let gr = &out.per_group[0];
    assert_eq!(gr.results[0].1.total, 0);
    assert!(
        gr.not_checked
            .iter()
            .any(|s| s.contains("нет попыток программы"))
    );
}

#[test]
fn request_check() {
    let r = Request {
        profile: Some("dbd".into()),
        app: None,
        mode: Mode::Auto,
        observe_secs: Some(30),
        budget_min: Some(10),
    };
    assert!(r.check().is_ok());
    assert!(
        Request {
            app: Some("x.exe".into()),
            ..r.clone()
        }
        .check()
        .is_err()
    );
    assert!(
        Request {
            profile: None,
            ..r.clone()
        }
        .check()
        .is_err()
    );
    assert!(
        Request {
            observe_secs: Some(9),
            ..r.clone()
        }
        .check()
        .is_err()
    );
    assert!(
        Request {
            budget_min: Some(61),
            ..r.clone()
        }
        .check()
        .is_err()
    );
}

#[test]
fn mode_is_lowercase() {
    assert_eq!(serde_json::to_string(&Mode::Auto).unwrap(), "\"auto\"");
    assert_eq!(serde_json::from_str::<Mode>("\"ask\"").unwrap(), Mode::Ask);
}

#[test]
fn each_finished_path_reaches_the_env_and_a_new_group_starts_empty() {
    let mut env = Fake::default();
    let groups = [group(Protocol::Tcp, 443), group(Protocol::Tcp, 80)];
    let out = run(&mut env, &ctx(true), &groups, &settings(Mode::Auto, 3600));
    let (first, second) = (
        out.per_group[0].results.len(),
        out.per_group[1].results.len(),
    );
    let expected: Vec<usize> = (0..=first).chain(0..=second).collect();
    assert_eq!(env.results, expected);
}

#[test]
fn syn_dead_everywhere_stops_before_strategies() {
    let dead = Group {
        fault: Fault::SynUnanswered { syns: 3 },
        ..group(Protocol::Tcp, 443)
    };
    let mut env = Fake::default();
    let out = run(
        &mut env,
        &ctx(true),
        &[dead.clone()],
        &settings(Mode::Auto, 600),
    );
    assert_eq!(
        env.sets,
        [
            "Как сейчас",
            "Напрямую мимо всего",
            "Прокси точечно",
            "Прокси без zapret"
        ]
    );
    let gr = &out.per_group[0];
    assert!(
        gr.not_checked
            .iter()
            .any(|n| n == &format!("Стратегия A: {SYN_DEAD}")),
        "{:?}",
        gr.not_checked
    );
    // One answer is enough to go on.
    let mut env = Fake::default();
    env.works.insert("Прокси точечно".into());
    run(
        &mut env,
        &ctx(true),
        &[dead.clone()],
        &settings(Mode::Auto, 600),
    );
    assert!(
        env.sets.iter().any(|s| s == "Стратегия A"),
        "{:?}",
        env.sets
    );
    // Not every group SYN-dead: the whole search.
    let mut env = Fake::default();
    run(
        &mut env,
        &ctx(true),
        &[dead, group(Protocol::Tcp, 443)],
        &settings(Mode::Auto, 600),
    );
    assert_eq!(env.sets.iter().filter(|s| *s == "Стратегия A").count(), 2);
}

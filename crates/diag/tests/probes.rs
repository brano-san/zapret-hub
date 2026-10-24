//! The sustained probes' scoring, the latency goal, and the trick grouping (agent proposal 5, item 4).

use std::time::Duration;

use diag::culprits::{Fault, Group};
use diag::evaluate::{Answer, ProbeScore, Score, kind_works, rank_for, works};
use diag::paths::{Ctx, PathKind, action_key, current_first, plan};
use diag::profiles::{
    Goal, ProbeKind, ProbeSpec, Profile, builtin, discord_ws_host, voice_endpoint,
};
use diag::run::{Env, Mode, Progress, Request, Settings, probes_for, run};
use router::observe::FlowCounters;
use router::rules::Protocol;

fn probe(kind: ProbeKind, ok: u32, total: u32) -> ProbeScore {
    ProbeScore {
        kind,
        target: "203.0.113.1:50001".into(),
        ok,
        total,
        median_ms: Some(30),
        ..ProbeScore::default()
    }
}

fn scored(probes: Vec<ProbeScore>, median_ms: Option<u32>) -> Score {
    Score {
        ok: probes.iter().map(|p| p.ok).sum(),
        total: probes.iter().map(|p| p.total).sum(),
        median_ms,
        probes,
        ..Score::default()
    }
}

#[test]
fn a_burst_works_under_20_pct_loss_with_3_replies() {
    use ProbeKind::DiscordIpDiscovery as Ip;
    assert!(kind_works(Ip, 33, 40));
    assert!(!kind_works(Ip, 32, 40), "20 % loss is too much");
    assert!(kind_works(Ip, 2, 3), "the old three single packets");
    assert!(!kind_works(Ip, 3, 40), "a cut after 3 packets");
}

#[test]
fn every_probe_kind_that_ran_must_pass() {
    // A cut flow: 75 % of the burst, enough for 2 of 3, not for the burst rule.
    let s = scored(vec![probe(ProbeKind::DiscordIpDiscovery, 30, 40)], None);
    assert!(!works(&s));
    let s = scored(
        vec![
            probe(ProbeKind::DiscordIpDiscovery, 40, 40),
            probe(ProbeKind::DiscordWs, 0, 3),
        ],
        None,
    );
    assert!(!works(&s), "the voice WebSocket fails");
    // A probe without targets and ping do not count.
    let s = scored(
        vec![
            probe(ProbeKind::Https, 0, 0),
            probe(ProbeKind::Ping, 0, 3),
            probe(ProbeKind::HttpsBulk, 2, 3),
        ],
        None,
    );
    assert!(works(&s));
    assert!(!works(&scored(vec![probe(ProbeKind::Https, 0, 0)], None)));
    // The user's answer still decides.
    let mut s = scored(vec![probe(ProbeKind::Https, 0, 3)], None);
    s.answer = Some(Answer::Yes);
    assert!(works(&s));
}

#[test]
fn an_old_score_loads_and_keeps_the_2_of_3_rule() {
    let s: Score =
        serde_json::from_str(r#"{"ok":2,"total":3,"median_ms":40,"loss_pct":null,"answer":null}"#)
            .unwrap();
    assert!(s.probes.is_empty());
    assert!(works(&s));
}

#[test]
fn latency_ranks_working_paths_by_rtt() {
    let ok = |ms| {
        scored(
            vec![probe(ProbeKind::Ping, 3, 3), probe(ProbeKind::Live, 2, 2)],
            ms,
        )
    };
    let point = PathKind::Trick {
        candidate: diag::candidates::Candidate {
            source: "zapret2:x".into(),
            profile: 0,
            text: String::new(),
        },
    };
    let mut r = vec![
        (
            PathKind::Current,
            scored(vec![probe(ProbeKind::Live, 0, 2)], Some(5)),
        ),
        (point.clone(), ok(Some(110))),
        (PathKind::Direct, ok(Some(100))),
        (PathKind::Strategy { name: "A".into() }, ok(None)),
    ];
    rank_for(&mut r, Goal::Latency);
    assert_eq!(r[0].0, PathKind::Direct, "10 % faster wins, pointed or not");
    assert_eq!(r[1].0, point);
    assert_eq!(r[3].0, PathKind::Current, "not working: last");
    // Reach keeps the ±15 % tie that favors pointed paths.
    rank_for(&mut r, Goal::Reach);
    assert_eq!(r[0].0, point);
}

#[test]
fn the_latency_goal_adds_a_ping_and_dbd_has_it() {
    let dbd = builtin().into_iter().find(|p| p.id == "dbd").unwrap();
    assert_eq!(dbd.goal, Goal::Latency);
    assert!(
        probes_for(&dbd)
            .iter()
            .any(|p| p.kind == ProbeKind::Ping && p.target == "observed")
    );
    let discord = builtin().remove(0);
    assert_eq!(probes_for(&discord), discord.probes);
}

#[test]
fn a_request_without_a_goal_takes_the_profiles() {
    let r: Request = serde_json::from_str(
        r#"{"profile":"dbd","app":null,"mode":"auto","observe_secs":null,"budget_min":null}"#,
    )
    .unwrap();
    assert_eq!(r.goal, None);
    let json = serde_json::to_string(&Request {
        goal: Some(Goal::Latency),
        ..r
    })
    .unwrap();
    assert!(json.contains(r#""goal":"latency""#), "{json}");
}

#[test]
fn the_voice_endpoint_is_the_newest_connect() {
    let lines: Vec<String> = [
        "[2026-10-24 12:00:00.000] [info]  [RTCControlSocket(default)] [CONNECT] wss://c-ams07-1.discord.media:2083/",
        "[2026-10-24 12:00:01.000] [info]  [RTCControlSocket(stream)] [CONNECT] wss://stream1.discord.media:443/",
        "[2026-10-24 12:01:00.000] [info]  [RTCControlSocket(default)] [CONNECT] wss://finland1.discord.media:443/",
        "[2026-10-24 12:01:00.200] [info]  [RTCControlSocket(default)] [CONNECTED] wss://finland1.discord.media:443/ in 215 ms",
    ]
    .map(String::from)
    .into();
    assert_eq!(
        voice_endpoint(&lines),
        Some(("finland1.discord.media".into(), 443))
    );
    assert_eq!(voice_endpoint(&lines[..1]).unwrap().1, 2083);
    assert_eq!(voice_endpoint(&[]), None);
    assert!(discord_ws_host("Finland1.discord.media", None));
    assert!(discord_ws_host("gateway-us-east1-b.discord.gg", None));
    assert!(discord_ws_host(
        "voice.example.net",
        Some("voice.example.net")
    ));
    assert!(!discord_ws_host("discord.com", Some("gateway.discord.gg")));
}

const SAME_FILE_A: &str = "--blob=tls_a:@\"%BIN%tls_google.bin\"\n\
--filter-tcp=443 --filter-l7=tls --hostlist=\"%LISTS%a.txt\"\n\
--lua-desync=fake:blob=tls_a:repeats=8\n--new\n\
--filter-tcp=443 --filter-l7=tls --hostlist=\"%LISTS%b.txt\"\n\
--lua-desync=multisplit:pos=1\n";
const SAME_FILE_B: &str = "--blob=tls_g:@\"%BIN%TLS_GOOGLE.bin\"\n\
--filter-tcp=443 --filter-l7=tls --ipset=\"%LISTS%c.txt\"\n\
--lua-desync=fake:blob=tls_g:repeats=8\n";
const OTHER_FILE: &str = "--blob=tls_g:@\"%BIN%tls_max.bin\"\n\
--filter-tcp=443 --filter-l7=tls\n--lua-desync=fake:blob=tls_g:repeats=8\n";

fn tcp_group() -> Group {
    Group {
        key: "203.0.113.0/24".into(),
        protocol: Protocol::Tcp,
        ports: vec![443],
        members: vec![],
        representatives: vec![],
        fault: Fault::ResetEarly,
    }
}

fn ctx(zapret2: Vec<(String, String)>) -> Ctx {
    Ctx {
        proxy_set: false,
        proxy_ok: false,
        ranking: vec![],
        strategies: vec![],
        zapret2,
        rules: vec![],
        profile: builtin().remove(0),
    }
}

fn tricks(c: &Ctx) -> Vec<String> {
    plan(&tcp_group(), c)
        .0
        .into_iter()
        .filter_map(|p| match p {
            PathKind::Trick { candidate } => {
                Some(format!("{}#{}", candidate.source, candidate.profile))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn tricks_that_do_the_same_are_tried_once() {
    let key = |t: &str| action_key(&zapret2::strategies::args(t));
    assert_eq!(key(SAME_FILE_A).matches("tls_google.bin").count(), 1);
    let c = ctx(vec![
        ("zapret2:A".into(), SAME_FILE_A.into()),
        ("zapret2:B".into(), SAME_FILE_B.into()),
        ("zapret2:C".into(), OTHER_FILE.into()),
    ]);
    assert_eq!(tricks(&c), ["zapret2:A#0", "zapret2:A#1", "zapret2:C#0"]);
    let skipped = plan(&tcp_group(), &c).1;
    assert!(
        skipped
            .iter()
            .any(|s| s.contains("zapret2:B")
                && s.contains("то же действие, что zapret2:A, профиль 1")),
        "{skipped:?}"
    );
}

#[test]
fn the_strategy_in_use_comes_first() {
    let mut c = ctx(vec![
        ("zapret2:A".into(), SAME_FILE_A.into()),
        ("zapret2:B".into(), SAME_FILE_B.into()),
    ]);
    c.ranking = vec!["general".into(), "zapret2:B".into()];
    current_first(&mut c, Some("zapret2:B"));
    assert_eq!(c.ranking, ["zapret2:B", "general"]);
    assert_eq!(tricks(&c)[0], "zapret2:B#0");
    current_first(&mut c, None);
    assert_eq!(c.ranking[0], "zapret2:B");
}

/// A fake whose live observation carries RTT samples.
#[derive(Default)]
struct Rtt {
    sets: Vec<String>,
}

impl Env for Rtt {
    fn set_path(&mut self, path: &PathKind, _: &Group) -> anyhow::Result<()> {
        self.sets.push(path.label());
        Ok(())
    }
    fn probe(&mut self, _: &Group, spec: &ProbeSpec) -> Score {
        // A TLS handshake takes several round trips.
        let ms = if spec.kind == ProbeKind::Ping {
            90
        } else {
            400
        };
        Score {
            ok: 3,
            total: 3,
            median_ms: Some(ms),
            ..Score::default()
        }
    }
    fn ask(&mut self, _: &str, _: &PathKind) -> Answer {
        Answer::Unknown
    }
    fn observe(&mut self, _: u32) -> Vec<FlowCounters> {
        vec![]
    }
    fn rollback(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn stopped(&self) -> bool {
        false
    }
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn progress(&mut self, _: Progress) {}
    fn results(&mut self, _: &[(PathKind, Score)]) {}
}

#[test]
fn the_latency_goal_measures_the_ping_not_the_handshake() {
    let mut c = ctx(vec![]);
    c.profile = Profile {
        probes: vec![ProbeSpec {
            kind: ProbeKind::Https,
            target: "observed".into(),
        }],
        goal: Goal::Latency,
        ..c.profile
    };
    let mut env = Rtt::default();
    let out = run(
        &mut env,
        &c,
        &[tcp_group()],
        &Settings {
            budget: Duration::from_secs(600),
            mode: Mode::Auto,
        },
    );
    // «Как сейчас» works and ends the group; its latency is the ping's 90, not HTTPS's 400.
    let (path, s) = &out.per_group[0].results[0];
    assert_eq!(*path, PathKind::Current);
    assert_eq!(s.median_ms, Some(90));
    assert_eq!(s.probes.len(), 2);
}

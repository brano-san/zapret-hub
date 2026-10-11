use probe::curl::{CheckKind, CheckResult, CheckStatus, DpiResult, DpiStatus};
use tester::model::{DpiTargetResult, ProfileScore, StrategyResult, Suite, TargetResult, TestRun};
use tester::rank::{extra_targets, median, score_dpi, score_standard, sort};

use CheckStatus::{Error, Ok as Pass, Ssl, Unsup};

fn target(name: &str, url: &str, statuses: [CheckStatus; 3], ms: u32) -> TargetResult {
    let checks = CheckKind::ALL
        .iter()
        .zip(statuses)
        .map(|(kind, status)| CheckResult {
            kind: *kind,
            status,
            http_code: if status == Pass {
                "200".into()
            } else {
                "000".into()
            },
            time_ms: Some(ms),
            stderr: String::new(),
        })
        .collect();
    TargetResult {
        name: name.into(),
        url: Some(url.into()),
        extra: false,
        checks,
        ping_ms: Some(7),
    }
}

fn strategy(name: &str, targets: Vec<TargetResult>) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: "x".into(),
        launch_error: None,
        score: score_standard(&targets),
        passes: Vec::new(),
        targets,
        dpi: Vec::new(),
    }
}

#[test]
fn unsup_and_ssl_count_as_failures() {
    let score = score_standard(&[target("A", "https://a.com", [Pass, Unsup, Ssl], 100)]);
    assert_eq!((score.ok, score.total, score.not_ok()), (1, 3, 2));
    assert_eq!((score.unsup, score.ssl, score.errors), (1, 1, 0));
    assert_eq!(score.median_ms, Some(100));
}

#[test]
fn ranking_is_failures_then_latency_then_menu_order() {
    let fast = |name| {
        strategy(
            name,
            vec![target("A", "https://a.com", [Pass, Pass, Pass], 80)],
        )
    };
    let mut results = vec![
        strategy(
            "general (ALT3)",
            vec![target("A", "https://a.com", [Pass, Pass, Error], 10)],
        ),
        fast("general (ALT10)"),
        strategy(
            "general (ALT1)",
            vec![target("A", "https://a.com", [Pass, Pass, Pass], 120)],
        ),
        StrategyResult {
            launch_error: Some("winws exited".into()),
            ..fast("general (ALT0)")
        },
        fast("general (ALT2)"),
    ];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "general (ALT2)",
            "general (ALT10)",
            "general (ALT1)",
            "general (ALT3)",
            "general (ALT0)"
        ]
    );
}

#[test]
fn a_mixed_run_ranks_both_engines_by_the_same_rules() {
    let fast = |name| {
        strategy(
            name,
            vec![target("A", "https://a.com", [Pass, Pass, Pass], 80)],
        )
    };
    let mut results = vec![
        fast("zapret2:mine"),
        strategy(
            "general",
            vec![target("A", "https://a.com", [Pass, Pass, Error], 10)],
        ),
        StrategyResult {
            launch_error: Some("winws2 printed LUA ERROR".into()),
            ..fast("zapret2:general")
        },
        fast("general (ALT2)"),
    ];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    // A tie keeps Flowseal first: `zapret2:` sorts after its names.
    assert_eq!(
        names,
        [
            "general (ALT2)",
            "zapret2:mine",
            "general",
            "zapret2:general"
        ]
    );
}

#[test]
fn a_dead_network_ranks_by_menu_order() {
    let dead = |name| {
        strategy(
            name,
            vec![target("A", "https://a.com", [Error, Error, Error], 2000)],
        )
    };
    let mut results = vec![
        dead("general"),
        dead("general (ALT10)"),
        dead("general (ALT2)"),
    ];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["general (ALT2)", "general (ALT10)", "general"]);
}

#[test]
fn extra_profile_targets_show_but_do_not_count() {
    let targets = vec![
        target("DiscordMain", "https://discord.com", [Pass, Pass, Pass], 50),
        TargetResult {
            extra: true,
            ..target(
                "steam.live.bhvrdbd.com",
                "https://steam.live.bhvrdbd.com",
                [Error, Error, Error],
                0,
            )
        },
    ];
    let score = score_standard(&targets);
    assert_eq!((score.ok, score.total), (3, 3));
    assert_eq!(
        score.discord,
        ProfileScore {
            ok: 3,
            total: 3,
            median_ms: Some(50)
        }
    );
    assert_eq!((score.dbd.ok, score.dbd.total), (0, 3));
    assert_eq!(score.youtube.total, 0);
}

#[test]
fn extra_targets_are_the_profile_hosts_targets_txt_lacks() {
    let extra = extra_targets(&zapret::targets::defaults());
    let hosts: Vec<&str> = extra.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        hosts,
        [
            "steam.live.bhvrdbd.com",
            "api.epicgames.dev",
            "store.steampowered.com"
        ]
    );
    assert_eq!(
        extra[0].url.as_deref(),
        Some("https://steam.live.bhvrdbd.com")
    );
}

#[test]
fn dpi_ranks_blocked_then_failed() {
    let dpi = |name: &str, statuses: &[DpiStatus]| {
        let checks = statuses
            .iter()
            .map(|status| DpiResult {
                kind: CheckKind::Tls12,
                status: *status,
                code: "200".into(),
                up_bytes: 0,
                down_bytes: 0,
                time_s: 0.1,
            })
            .collect();
        let targets = vec![DpiTargetResult {
            id: "X".into(),
            provider: "P".into(),
            country: String::new(),
            host: "x.example".into(),
            checks,
        }];
        StrategyResult {
            name: name.into(),
            args_sha256: "x".into(),
            launch_error: None,
            targets: Vec::new(),
            score: score_dpi(&targets),
            passes: Vec::new(),
            dpi: targets,
        }
    };
    let mut results = vec![
        dpi("A", &[DpiStatus::LikelyBlocked, DpiStatus::Ok]),
        dpi("B", &[DpiStatus::Fail, DpiStatus::Fail]),
        dpi("C", &[DpiStatus::Fail, DpiStatus::Ok]),
    ];
    sort(Suite::Dpi, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["C", "B", "A"]);
    assert_eq!(
        (results[2].score.likely_blocked, results[2].score.total),
        (1, 2)
    );
}

#[test]
fn median_is_the_lower_middle() {
    assert_eq!(median(&mut [4, 1, 3, 2]), Some(2));
    assert_eq!(median(&mut [5]), Some(5));
    assert_eq!(median(&mut []), None);
}

#[test]
fn report_reads_like_test_zapret_ps1() {
    let targets = vec![
        target("DiscordMain", "https://discord.com", [Pass, Ssl, Unsup], 50),
        TargetResult {
            name: "CloudflareDNS1111".into(),
            url: None,
            extra: false,
            checks: Vec::new(),
            ping_ms: None,
        },
    ];
    let run = TestRun {
        id: "2026-10-10_05-03-34".into(),
        suite: Suite::Standard,
        flowseal_version: Some("1.10.3".into()),
        zapret2_version: None,
        canceled: false,
        restore_error: None,
        strategies: vec![strategy("general (ALT11)", targets)],
    };
    assert_eq!(
        tester::report::to_text(&run),
        concat!(
            "Config: general (ALT11).bat (Type: standard)\r\n",
            "  DiscordMain : HTTP:OK    TLS1.2:SSL   TLS1.3:UNSUP | Ping: 7 ms\r\n",
            "  CloudflareDNS1111 :  | Ping: Timeout\r\n",
            "\r\n",
            "=== ANALYTICS ===\r\n",
            "general (ALT11).bat : HTTP OK:   1, ERR:   1, UNSUP:   1, Ping OK:   1, Fail:   1\r\n",
            "Best strategy: general (ALT11).bat\r\n",
        )
    );
}

#[test]
fn passes_sum_their_counts_and_take_the_median_latency() {
    let host = "https://a.example";
    let pass = |statuses, ms| strategy("general", vec![target("A", host, statuses, ms)]);
    let one = pass([Pass, Pass, Pass], 100);
    assert_eq!(
        tester::rank::aggregate(vec![one.clone()]),
        one,
        "a single pass is the result as it is"
    );
    let all = tester::rank::aggregate(vec![
        pass([Pass, Pass, Pass], 300),
        pass([Pass, Error, Unsup], 100),
        pass([Pass, Pass, Ssl], 200),
    ]);
    let s = &all.score;
    assert_eq!((s.ok, s.total, s.not_ok()), (6, 9, 3));
    assert_eq!((s.errors, s.unsup, s.ssl), (1, 1, 1));
    assert_eq!(s.median_ms, Some(200), "the median of 300, 100, 200");
    assert_eq!((s.ping_ok, s.ping_total, s.ping_median_ms), (3, 3, Some(7)));
    assert_eq!(all.passes.len(), 3);
    assert_eq!(all.passes[1].score.median_ms, Some(100));
    assert_eq!(all.targets, all.passes[2].targets, "the last pass is shown");
    assert_eq!(all.launch_error, None);
}

#[test]
fn a_launch_error_in_any_pass_ranks_the_strategy_last() {
    let host = "https://a.example";
    let good = strategy("general", vec![target("A", host, [Pass, Pass, Pass], 100)]);
    let mut broken = good.clone();
    broken.launch_error = Some("winws exited (exit code: 1)".into());
    broken.score = Default::default();
    let all = tester::rank::aggregate(vec![good.clone(), broken, good.clone()]);
    assert_eq!(
        all.launch_error.as_deref(),
        Some("pass 2 of 3: winws exited (exit code: 1)")
    );
    let steady = tester::rank::aggregate(vec![
        strategy(
            "general (ALT)",
            vec![target("A", host, [Error, Error, Error], 900)],
        ),
        strategy(
            "general (ALT)",
            vec![target("A", host, [Error, Error, Error], 900)],
        ),
        strategy(
            "general (ALT)",
            vec![target("A", host, [Error, Error, Error], 900)],
        ),
    ]);
    let mut ranked = vec![all, steady];
    sort(Suite::Standard, &mut ranked);
    assert_eq!(ranked[0].name, "general (ALT)");
}

#[test]
fn ranking_with_repeats_uses_the_aggregate() {
    let host = "https://a.example";
    let run = |name: &str, passes: &[([CheckStatus; 3], u32)]| {
        tester::rank::aggregate(
            passes
                .iter()
                .map(|(statuses, ms)| strategy(name, vec![target("A", host, *statuses, *ms)]))
                .collect(),
        )
    };
    // Lucky once: the best single pass, but worse over three.
    let lucky = run(
        "general",
        &[
            ([Pass, Pass, Pass], 50),
            ([Error, Pass, Pass], 400),
            ([Error, Error, Pass], 400),
        ],
    );
    let steady = run(
        "general (ALT)",
        &[
            ([Pass, Pass, Error], 150),
            ([Pass, Pass, Pass], 160),
            ([Pass, Pass, Pass], 170),
        ],
    );
    let mut ranked = vec![lucky, steady];
    sort(Suite::Standard, &mut ranked);
    assert_eq!(ranked[0].name, "general (ALT)");
    assert_eq!(ranked[0].score.median_ms, Some(160));
}

#[test]
fn a_canceled_run_keeps_only_whole_passes() {
    use tester::run::whole_passes;
    let r = |name: &str| strategy(name, Vec::new());
    // Canceled in pass 2 after the first strategy: pass 2 is dropped.
    let kept = whole_passes(vec![vec![r("a"), r("a")], vec![r("b")], vec![r("c")]]);
    assert_eq!(kept.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 1]);
    // Canceled in pass 1: the strategies it reached are the run.
    let kept = whole_passes(vec![vec![r("a")], vec![r("b")], Vec::new()]);
    assert_eq!(kept.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn dpi_passes_sum_blocked_and_failed() {
    let pass = |status| StrategyResult {
        dpi: Vec::new(),
        score: score_dpi(&[DpiTargetResult {
            id: "US.X".into(),
            provider: "p".into(),
            country: "us".into(),
            host: "x.example".into(),
            checks: vec![DpiResult {
                kind: CheckKind::Tls12,
                status,
                code: "NA".into(),
                up_bytes: 0,
                down_bytes: 0,
                time_s: -1.0,
            }],
        }]),
        ..strategy("general", Vec::new())
    };
    let all = tester::rank::aggregate(vec![
        pass(DpiStatus::LikelyBlocked),
        pass(DpiStatus::Ok),
        pass(DpiStatus::Fail),
    ]);
    let s = &all.score;
    assert_eq!((s.ok, s.total, s.likely_blocked, s.fail), (1, 3, 1, 1));
}

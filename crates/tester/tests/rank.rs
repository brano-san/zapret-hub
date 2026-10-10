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

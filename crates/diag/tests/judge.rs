use diag::agent::{Level, MediaState, VerdictCode, VoiceStage};
use diag::culprits::{Fault, Group};
use diag::evaluate::{Answer, Score};
use diag::judge::fill;
use diag::logs::discord::{Dave, Voice};
use diag::logs::neko_routes::Routes;
use diag::paths::{Ctx, NO_TRICKS_FOR_SYN, PathKind};
use diag::patterns::{Evidence, Pattern};
use diag::profiles::{ProbeKind, ProbeSpec, Profile};
use diag::run::SYN_DEAD;
use diag::summary::{GroupSummary, Summary};
use router::observe::FlowCounters;
use router::rules::Protocol;

fn profile(kinds: &[ProbeKind]) -> Profile {
    Profile {
        id: "discord".into(),
        name: "Discord".into(),
        apps: vec!["Discord.exe".into()],
        hint: String::new(),
        domains: vec![],
        probes: kinds
            .iter()
            .map(|&kind| ProbeSpec {
                kind,
                target: "observed".into(),
            })
            .collect(),
        goal: Default::default(),
    }
}

fn ctx(kinds: &[ProbeKind]) -> Ctx {
    Ctx {
        proxy_set: true,
        proxy_ok: true,
        proxy_udp: Some(false),
        ranking: vec![],
        strategies: vec![],
        zapret2: vec![],
        rules: vec![],
        profile: profile(kinds),
    }
}

fn https() -> Ctx {
    ctx(&[ProbeKind::Https])
}

/// A TCP flow that worked.
fn tcp_ok() -> FlowCounters {
    FlowCounters {
        protocol: Protocol::Tcp,
        dst: "203.0.113.5:443".into(),
        syns: 1,
        syn_answered: true,
        up_packets: 10,
        down_packets: 10,
        ..FlowCounters::default()
    }
}

/// Voice UDP; `down` replies.
fn voice_flow(down: u32) -> FlowCounters {
    FlowCounters {
        protocol: Protocol::Udp,
        dst: "203.0.113.7:50004".into(),
        up_packets: 10,
        down_packets: down,
        ..FlowCounters::default()
    }
}

fn proxied() -> FlowCounters {
    FlowCounters {
        via_system_proxy: true,
        dst: "127.0.0.1:2080".into(),
        ..tcp_ok()
    }
}

fn score(ok: u32, total: u32) -> Score {
    Score {
        ok,
        total,
        ..Score::default()
    }
}

fn gs(key: &str, fault: Fault, best: Option<u32>, results: Vec<(PathKind, Score)>) -> GroupSummary {
    GroupSummary {
        group: Group {
            key: key.into(),
            protocol: Protocol::Udp,
            fault,
            ..Group::default()
        },
        seen: "видно".into(),
        results,
        best,
        ..GroupSummary::default()
    }
}

/// As if Discord's log showed a connected session, so the flows alone decide.
fn run(summary: &mut Summary, flows: &[FlowCounters], e: &Evidence, c: &Ctx) {
    summary.agent.client.voice.get_or_insert(MediaState {
        stage: VoiceStage::Connected,
        ..MediaState::default()
    });
    fill(summary, flows, e, c, 30);
}

fn with_stage(stage: Option<VoiceStage>) -> Summary {
    let mut s = Summary::default();
    s.agent.client.voice = stage.map(|stage| MediaState {
        stage,
        ..MediaState::default()
    });
    s
}

#[test]
fn no_media_session_in_the_log_is_not_reproduced() {
    let mut s = with_stage(None);
    fill(
        &mut s,
        &[tcp_ok()],
        &neko_seen(),
        &ctx(&[ProbeKind::DiscordIpDiscovery]),
        30,
    );
    assert_eq!(s.agent.verdict, VerdictCode::NotReproducible);
    assert!(
        s.agent
            .confidence
            .why
            .contains(&"no_media_session".to_string())
    );
}

#[test]
fn a_stuck_stage_makes_a_quiet_watch_inconclusive() {
    let mut s = with_stage(Some(VoiceStage::UdpFailed));
    fill(
        &mut s,
        &[tcp_ok()],
        &neko_seen(),
        &ctx(&[ProbeKind::DiscordIpDiscovery]),
        30,
    );
    assert_eq!(s.agent.verdict, VerdictCode::Inconclusive);
    assert!(
        s.agent
            .confidence
            .why
            .contains(&"client_stage:udp_failed".to_string())
    );
}

#[test]
fn without_the_discord_log_the_flows_decide() {
    let mut s = with_stage(None);
    s.sources_missing = vec!["журнал Discord".into()];
    fill(
        &mut s,
        &[],
        &neko_seen(),
        &ctx(&[ProbeKind::DiscordIpDiscovery]),
        30,
    );
    assert_eq!(s.agent.verdict, VerdictCode::NoFlows);
}

fn codes(facts: &[diag::agent::Fact]) -> Vec<&str> {
    facts.iter().map(|f| f.code.as_str()).collect()
}

fn neko_seen() -> Evidence {
    Evidence {
        neko: Some(vec![]),
        neko_routes: Some(Routes::default()),
        ..Evidence::default()
    }
}

#[test]
fn no_flows() {
    let mut s = Summary::default();
    run(&mut s, &[], &Evidence::default(), &https());
    assert_eq!(s.agent.verdict, VerdictCode::NoFlows);
    assert_eq!(s.agent.confidence.level, Level::Medium);
    assert_eq!(s.agent.observed.secs, 30);
}

#[test]
fn clean_when_everything_was_seen() {
    let mut s = Summary::default();
    let flows = [tcp_ok(), proxied(), voice_flow(10)];
    let c = ctx(&[ProbeKind::DiscordIpDiscovery]);
    run(&mut s, &flows, &neko_seen(), &c);
    assert_eq!(s.agent.verdict, VerdictCode::Clean);
    assert_eq!(s.agent.confidence.level, Level::High);
    assert!(s.agent.confidence.why.is_empty());
    let o = &s.agent.observed;
    assert_eq!(
        (o.flows, o.tcp, o.udp, o.via_router, o.via_system_proxy),
        (3, 2, 1, 0, 1)
    );
    assert_eq!(s.agent.context.proxy_udp, Some(false));
}

#[test]
fn proxied_flows_without_nekobox_logs_are_inconclusive() {
    let mut s = Summary {
        sources_missing: vec!["журнал NekoBox".into()],
        verdict_text: "соединения программы проходят; если проблема есть, она не в сети".into(),
        ..Summary::default()
    };
    run(
        &mut s,
        &[tcp_ok(), proxied()],
        &Evidence::default(),
        &https(),
    );
    assert_eq!(s.agent.verdict, VerdictCode::Inconclusive);
    assert_eq!(s.agent.confidence.level, Level::Low);
    assert_eq!(
        s.agent.confidence.why,
        ["source_missing:neko_log", "inconclusive"]
    );
    assert_eq!(
        codes(&s.agent.not_checked),
        ["source_missing:neko_log", "proxy_unseen"]
    );
    assert!(
        s.verdict_text.contains("не разобраны"),
        "{}",
        s.verdict_text
    );
}

#[test]
fn a_missing_nekobox_log_does_not_matter_without_proxied_flows() {
    let mut s = Summary {
        sources_missing: vec!["журнал NekoBox".into(), "маршруты NekoBox".into()],
        ..Summary::default()
    };
    run(&mut s, &[tcp_ok()], &Evidence::default(), &https());
    assert_eq!(s.agent.verdict, VerdictCode::Clean);
    assert!(s.agent.confidence.why.is_empty());
    assert_eq!(
        codes(&s.agent.not_checked),
        ["source_missing:neko_log", "source_missing:neko_routes"]
    );
}

#[test]
fn a_voice_profile_without_a_voice_flow_is_inconclusive() {
    let mut s = Summary {
        sources_missing: vec!["журнал Discord".into()],
        ..Summary::default()
    };
    let c = ctx(&[ProbeKind::Https, ProbeKind::DiscordIpDiscovery]);
    run(&mut s, &[tcp_ok()], &Evidence::default(), &c);
    assert_eq!(s.agent.verdict, VerdictCode::Inconclusive);
    assert_eq!(
        s.agent.confidence.why,
        [
            "source_missing:discord_log",
            "no_voice_flow",
            "inconclusive"
        ]
    );
    assert!(codes(&s.agent.not_checked).contains(&"no_voice_flow"));
    assert!(s.verdict_text.contains("голосового UDP-потока не было"));
}

#[test]
fn culprits_fixed_partial_unfixed() {
    let flows = [voice_flow(0)];
    let fixed = || {
        gs(
            "203.0.113.0/24",
            Fault::UdpNoReply { up: 10, down: 0 },
            Some(0),
            vec![(PathKind::Direct, score(3, 3))],
        )
    };
    let unfixed = || {
        gs(
            "198.51.100.0/24",
            Fault::UdpNoReply { up: 10, down: 0 },
            None,
            vec![(PathKind::Direct, score(0, 3))],
        )
    };
    let c = ctx(&[ProbeKind::DiscordIpDiscovery]);
    let verdict = |groups: Vec<GroupSummary>| {
        let mut s = Summary {
            groups,
            ..Summary::default()
        };
        run(&mut s, &flows, &Evidence::default(), &c);
        s.agent
    };
    let a = verdict(vec![fixed()]);
    assert_eq!(
        (a.verdict, a.confidence.level),
        (VerdictCode::CulpritsFixed, Level::High)
    );
    assert_eq!(
        verdict(vec![fixed(), unfixed()]).verdict,
        VerdictCode::CulpritsPartial
    );
    let a = verdict(vec![unfixed()]);
    assert_eq!(
        (a.verdict, a.confidence.level),
        (VerdictCode::CulpritsUnfixed, Level::Medium)
    );
    // None picked: nothing fixed.
    assert_eq!(verdict(vec![]).verdict, VerdictCode::CulpritsUnfixed);
}

#[test]
fn a_best_path_at_two_of_three_is_low() {
    let mut s = Summary {
        groups: vec![gs(
            "203.0.113.0/24",
            Fault::UdpNoReply { up: 10, down: 0 },
            Some(0),
            vec![(PathKind::Direct, score(2, 3))],
        )],
        stopped: true,
        budget_hit: true,
        canceled: true,
        ..Summary::default()
    };
    run(&mut s, &[voice_flow(0)], &Evidence::default(), &https());
    assert_eq!(s.agent.confidence.level, Level::Low);
    assert_eq!(
        s.agent.confidence.why,
        ["canceled", "stopped", "budget_hit", "best_at_floor"]
    );
    // The user's answer is not a 2-of-3 floor.
    s.groups[0].results[0].1.answer = Some(Answer::Yes);
    s.stopped = false;
    s.budget_hit = false;
    s.canceled = false;
    run(&mut s, &[voice_flow(0)], &Evidence::default(), &https());
    assert!(s.agent.confidence.why.is_empty());
}

#[test]
fn live_only() {
    let mut s = Summary {
        groups: vec![gs(
            "203.0.113.0/24",
            Fault::UdpNoReply { up: 10, down: 0 },
            None,
            vec![(PathKind::Direct, Score::default())],
        )],
        ..Summary::default()
    };
    run(
        &mut s,
        &[voice_flow(0)],
        &Evidence::default(),
        &ctx(&[ProbeKind::Live]),
    );
    assert_eq!(s.agent.verdict, VerdictCode::LiveOnly);
    assert_eq!(s.agent.confidence.why, ["live_only"]);
}

#[test]
fn whitelist_proxy_broken_peer_side() {
    let with = |patterns: Vec<Pattern>, flows: &[FlowCounters]| {
        let mut s = Summary {
            patterns,
            ..Summary::default()
        };
        run(&mut s, flows, &neko_seen(), &https());
        s.agent
    };
    let a = with(vec![Pattern::R5Whitelist], &[voice_flow(0)]);
    assert_eq!(a.verdict, VerdictCode::Whitelist);
    assert_eq!(codes(&a.evidence), ["pattern:r5_whitelist"]);
    let r1 = |direct_ok| Pattern::R1ViaSystemProxy {
        domain: "a.example".into(),
        direct_ok,
        proxy_ok: false,
        route: Some("напрямую".into()),
        direct: true,
        rule: None,
    };
    assert_eq!(
        with(vec![r1(false)], &[tcp_ok(), proxied()]).verdict,
        VerdictCode::ProxyBroken
    );
    // Sent direct, and direct works: NekoBox's way passes.
    assert_eq!(
        with(vec![r1(true)], &[tcp_ok(), proxied()]).verdict,
        VerdictCode::Clean
    );
    let r6 = Pattern::R6DaveStalled {
        alone: false,
        control_route: None,
        direct: false,
    };
    let a = with(vec![r6], &[tcp_ok()]);
    assert_eq!(
        (a.verdict, a.confidence.level),
        (VerdictCode::PeerSide, Level::High)
    );
}

#[test]
fn a_fact_per_fault() {
    let faults = [
        (Fault::SynUnanswered { syns: 3 }, "fault:syn_unanswered"),
        (Fault::ResetEarly, "fault:reset_early"),
        (Fault::RedirectEarly, "fault:redirect_early"),
        (Fault::DataRetrans { count: 4 }, "fault:data_retrans"),
        (Fault::UdpNoReply { up: 10, down: 0 }, "fault:udp_no_reply"),
        (Fault::Slow { median_ms: 300 }, "fault:slow"),
        (Fault::Lossy { loss_pct: 40 }, "fault:lossy"),
    ];
    let mut s = Summary {
        groups: faults
            .iter()
            .map(|(f, _)| gs("k", f.clone(), None, vec![]))
            .collect(),
        ..Summary::default()
    };
    run(&mut s, &[voice_flow(0)], &Evidence::default(), &https());
    let want: Vec<&str> = faults.iter().map(|(_, c)| *c).collect();
    assert_eq!(codes(&s.agent.evidence), want);
    assert_eq!(s.agent.evidence[4].args, ["k", "10", "0"]);
    assert_eq!(s.agent.evidence[4].text, "видно");
}

#[test]
fn a_fact_per_pattern() {
    let patterns = vec![
        Pattern::R1RuleMissesProtocol {
            rule: "r".into(),
            has: Protocol::Tcp,
            needs: Protocol::Udp,
        },
        Pattern::R1ViaSystemProxy {
            domain: "a.example".into(),
            direct_ok: false,
            proxy_ok: true,
            route: None,
            direct: false,
            rule: None,
        },
        Pattern::R2VoiceUdp {
            server: "203.0.113.0/24".into(),
            via_vpn: false,
        },
        Pattern::R3SlowProxyDns {
            median_ms: 900,
            direct_dns: "203.0.113.53".into(),
        },
        Pattern::R4DnsSpoofed {
            domain: "a.example".into(),
        },
        Pattern::R5Whitelist,
        Pattern::R6DaveStalled {
            alone: false,
            control_route: Some("через VPN".into()),
            direct: false,
        },
    ];
    let mut s = Summary {
        patterns,
        ..Summary::default()
    };
    run(&mut s, &[tcp_ok()], &neko_seen(), &https());
    assert_eq!(
        codes(&s.agent.evidence),
        [
            "pattern:r1_rule_misses_protocol",
            "pattern:r1_via_system_proxy",
            "pattern:r2_voice_udp",
            "pattern:r3_slow_proxy_dns",
            "pattern:r4_dns_spoofed",
            "pattern:r5_whitelist",
            "pattern:r6_dave_stalled",
        ]
    );
    assert_eq!(s.agent.evidence[2].args, ["203.0.113.0/24", "false"]);
    assert!(
        s.agent.evidence.iter().all(|f| !f.text.is_empty()),
        "{:?}",
        s.agent.evidence
    );
}

#[test]
fn alone_dave_waits_without_a_fault() {
    let e = Evidence {
        voice: Some(Voice {
            media_server: Some("203.0.113.7:50004".into()),
            local_address: None,
            own_ssrc: Some(1),
            peers: vec![],
            dave: Dave::KeyPackageSent { secs_waiting: 30 },
            no_voice_state: false,
            ws_closed: vec![],
        }),
        ..neko_seen()
    };
    let mut s = Summary::default();
    let c = ctx(&[ProbeKind::DiscordIpDiscovery]);
    run(&mut s, &[voice_flow(10)], &e, &c);
    assert_eq!(s.agent.verdict, VerdictCode::Clean);
    assert_eq!(codes(&s.agent.evidence), ["client:dave_waiting_peer"]);
    assert!(s.explanations[0].contains("это не сбой"));
    assert!(s.fixes.is_empty());
}

#[test]
fn skipped_paths_as_codes() {
    let mut g = gs(
        "203.0.113.0/24",
        Fault::UdpNoReply { up: 10, down: 0 },
        None,
        vec![],
    );
    g.notes = vec![
        "Прокси точечно: прокси не пропускает UDP".into(),
        "Прокси без zapret: прокси не пропускает UDP".into(),
        "Прокси точечно: прокси не отвечает".into(),
        "Прокси точечно: прокси не задан в настройках".into(),
        format!("Стратегия A: {SYN_DEAD}"),
        NO_TRICKS_FOR_SYN.into(),
        "Стратегия B: фильтр не покрывает UDP 50004".into(),
        diag::paths::routed_note("Discord"),
    ];
    let mut s = Summary {
        groups: vec![g],
        ..Summary::default()
    };
    run(&mut s, &[voice_flow(0)], &Evidence::default(), &https());
    assert_eq!(
        codes(&s.agent.not_checked),
        [
            "proxy_no_udp",
            "proxy_silent",
            "proxy_unset",
            "syn_dead",
            "no_tricks_for_syn",
            "path_skipped",
        ]
    );
    assert_eq!(s.agent.not_checked[0].args, ["203.0.113.0/24"]);
}

#[test]
fn unobserved_on_linux_is_at_most_inconclusive_and_says_why() {
    let mut s = Summary::default();
    run(&mut s, &[], &Evidence::default(), &https());
    assert_eq!(s.agent.verdict, VerdictCode::NoFlows);
    diag::judge::unobserved_on_linux(&mut s);
    assert_eq!(s.agent.verdict, VerdictCode::Inconclusive);
    assert_eq!(s.agent.confidence.level, Level::Low);
    assert!(
        s.agent
            .confidence
            .why
            .contains(&"platform:linux".to_string())
    );
    assert!(s.verdict_text.contains("Linux"), "{}", s.verdict_text);
    let linux: Vec<&str> = s
        .agent
        .not_checked
        .iter()
        .filter(|f| f.code == "platform:linux")
        .map(|f| f.args[0].as_str())
        .collect();
    assert_eq!(linux, ["flows", "dns", "paths"]);

    // A verdict from the client's own log stays: it does not need the observation.
    let mut s = with_stage(None);
    fill(
        &mut s,
        &[],
        &neko_seen(),
        &ctx(&[ProbeKind::DiscordIpDiscovery]),
        0,
    );
    assert_eq!(s.agent.verdict, VerdictCode::NotReproducible);
    let text = s.verdict_text.clone();
    diag::judge::unobserved_on_linux(&mut s);
    assert_eq!(s.agent.verdict, VerdictCode::NotReproducible);
    assert_eq!(s.verdict_text, text);
    assert_eq!(
        codes(&s.agent.not_checked)
            .iter()
            .filter(|c| **c == "platform:linux")
            .count(),
        3
    );
}

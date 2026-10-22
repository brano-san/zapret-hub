use diag::candidates::Candidate;
use diag::culprits::{Fault, Group, Verdict};
use diag::evaluate::Score;
use diag::logs::discord::{Dave, Voice};
use diag::logs::neko::{Conn, Route};
use diag::paths::{Ctx, PathKind};
use diag::patterns::{Evidence, Pattern, detect};
use diag::profiles::{ProbeKind, ProbeSpec, Profile};
use diag::report;
use diag::run::{GroupResult, Outcome};
use diag::summary::{Fix, summarize, warn_breaks};
use router::observe::FlowCounters;
use router::rules::{Protocol, Rule};

const ID: &str = "2026-10-22_12-00-00";

fn flow(protocol: Protocol, dst: &str) -> FlowCounters {
    FlowCounters {
        protocol,
        local_port: 50000,
        dst: dst.into(),
        domain: None,
        sni: None,
        ipv6: false,
        via_system_proxy: false,
        up_packets: 10,
        down_packets: 0,
        up_bytes: 1000.0,
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
    }
}

fn group(protocol: Protocol, dst: &str) -> Group {
    let f = flow(protocol, dst);
    let (ip, port) = dst.rsplit_once(':').unwrap();
    let [a, b, c, _]: [&str; 4] = ip.split('.').collect::<Vec<_>>().try_into().unwrap();
    Group {
        key: format!("{a}.{b}.{c}.0/24"),
        protocol,
        ports: vec![port.parse().unwrap()],
        members: vec![f.clone()],
        representatives: vec![f],
        fault: Fault::UdpNoReply { up: 10, down: 0 },
    }
}

fn ok() -> Score {
    Score {
        ok: 3,
        total: 3,
        median_ms: Some(40),
        ..Score::default()
    }
}

fn bad() -> Score {
    Score {
        ok: 0,
        total: 3,
        ..Score::default()
    }
}

fn profile(kind: ProbeKind) -> Profile {
    Profile {
        id: "discord".into(),
        name: "Discord".into(),
        apps: vec!["Discord.exe".into()],
        hint: String::new(),
        domains: vec![],
        probes: vec![ProbeSpec {
            kind,
            target: "observed".into(),
        }],
    }
}

fn ctx() -> Ctx {
    Ctx {
        proxy_ok: true,
        ranking: vec![],
        strategies: vec![],
        zapret2: vec![],
        app_rules: vec![Rule {
            name: "DBD Process".into(),
            enabled: true,
            protocol: Protocol::Udp,
            ports: vec![],
            ips: vec![],
            domains: vec![],
            apps: vec!["Discord.exe".into()],
        }],
        profile: profile(ProbeKind::DiscordIpDiscovery),
    }
}

fn outcome(group: Group, results: Vec<(PathKind, Score)>) -> Outcome {
    Outcome {
        per_group: vec![GroupResult {
            group,
            results,
            skipped: vec![],
            not_checked: vec![],
        }],
        canceled: false,
        budget_hit: false,
        rollback_error: None,
    }
}

fn evidence() -> Evidence {
    Evidence {
        flows: vec![],
        neko: None,
        neko_routes: None,
        voice: None,
        proxy_ip: None,
        dns_mismatch: vec![],
        ru_ok: None,
        foreign_syn_dead: None,
    }
}

fn voice(peers: Vec<u32>, secs_waiting: u32) -> Voice {
    Voice {
        media_server: Some("c-fra.discord.media".into()),
        local_address: Some("5.6.7.8".into()),
        own_ssrc: Some(1),
        peers,
        dave: Dave::KeyPackageSent { secs_waiting },
        no_voice_state: false,
        ws_closed: vec![],
    }
}

fn point_rule() -> Rule {
    Rule {
        name: "whatever".into(),
        enabled: true,
        protocol: Protocol::Udp,
        ports: vec!["19300".into()],
        ips: vec!["104.29.1.0/24".into()],
        domains: vec![],
        apps: vec!["Discord.exe".into()],
    }
}

fn empty() -> Outcome {
    Outcome {
        per_group: vec![],
        canceled: false,
        budget_hit: false,
        rollback_error: None,
    }
}

#[test]
fn r1_from_extend_rule() {
    let g = group(Protocol::Tcp, "9.9.9.9:443");
    let extend = PathKind::ExtendRule {
        rule: "DBD Process".into(),
        protocol: Protocol::Both,
    };
    let out = outcome(
        g.clone(),
        vec![(extend.clone(), ok()), (PathKind::Current, bad())],
    );
    let p = detect(&evidence(), &out, &ctx());
    assert_eq!(
        p,
        vec![Pattern::R1RuleMissesProtocol {
            rule: "DBD Process".into(),
            has: Protocol::Udp,
            needs: Protocol::Tcp,
        }]
    );
    let s = summarize(&Verdict::Culprits(vec![g]), &out, &p, &ctx(), ID.into(), 30);
    assert_eq!(
        s.fixes,
        vec![Fix::EditRule {
            name: "DBD Process".into(),
            protocol: Protocol::Both
        }]
    );
    // An extend rule that does not work is no R1.
    let out = outcome(group(Protocol::Tcp, "9.9.9.9:443"), vec![(extend, bad())]);
    assert!(detect(&evidence(), &out, &ctx()).is_empty());
}

#[test]
fn r2_voice_via_vpn() {
    let out = outcome(group(Protocol::Udp, "104.29.1.5:19300"), vec![]);
    let mut e = evidence();
    e.voice = Some(voice(vec![], 0));
    e.proxy_ip = Some("5.6.7.8".into());
    assert_eq!(
        detect(&e, &out, &ctx()),
        vec![Pattern::R2VoiceUdp {
            server: "104.29.1.0/24".into(),
            via_vpn: true
        }]
    );
    e.proxy_ip = None;
    assert_eq!(
        detect(&e, &out, &ctx()),
        vec![Pattern::R2VoiceUdp {
            server: "104.29.1.0/24".into(),
            via_vpn: false
        }]
    );
    // Elsewhere, or TCP: not R2.
    let out = outcome(group(Protocol::Udp, "8.8.8.8:19300"), vec![]);
    assert!(detect(&e, &out, &ctx()).is_empty());
    let out = outcome(group(Protocol::Tcp, "35.217.1.1:443"), vec![]);
    assert!(detect(&e, &out, &ctx()).is_empty());
}

#[test]
fn r3_slow_dns() {
    let conn = |ms| Conn {
        host: "discord.com".into(),
        port: 443,
        route: Route::Proxy,
        outbound_ms: Some(ms),
    };
    let mut e = evidence();
    e.neko = Some(vec![
        conn(5000),
        conn(6000),
        conn(7000),
        conn(100),
        conn(8000),
    ]);
    e.neko_routes = Some(diag::logs::neko_routes::Routes {
        direct_dns: "https://doh.pub/dns-query".into(),
        ..Default::default()
    });
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        vec![Pattern::R3SlowProxyDns {
            median_ms: 6000,
            direct_dns: "https://doh.pub/dns-query".into()
        }]
    );
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(s.explanations.iter().any(|x| x
        == "NekoBox долго ищет адреса через свой прямой DNS; каждое соединение ждёт"));
    assert!(
        matches!(&s.fixes[..], [Fix::Manual { text }] if text.contains("https://doh.pub/dns-query"))
    );
}

#[test]
fn r4_mismatch() {
    let mut e = evidence();
    e.dns_mismatch = vec!["youtube.com".into(), "googlevideo.com".into()];
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        vec![
            Pattern::R4DnsSpoofed {
                domain: "youtube.com".into()
            },
            Pattern::R4DnsSpoofed {
                domain: "googlevideo.com".into()
            }
        ]
    );
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    // One explanation and one advice for all domains.
    assert_eq!(s.explanations.len(), 1);
    assert!(s.explanations[0].contains("провайдер подменяет DNS"));
    assert_eq!(s.fixes.len(), 1);
}

#[test]
fn r5_whitelist_skips_paths() {
    let mut e = evidence();
    e.ru_ok = Some(true);
    e.foreign_syn_dead = Some(true);
    let g = group(Protocol::Udp, "1.2.3.4:443");
    let out = outcome(g.clone(), vec![(PathKind::Direct, ok())]);
    let p = detect(&e, &out, &ctx());
    assert_eq!(p, vec![Pattern::R5Whitelist]);
    let s = summarize(&Verdict::Culprits(vec![g]), &out, &p, &ctx(), ID.into(), 30);
    assert_eq!(
        s.verdict_text,
        "провайдер пускает только в разрешённые сети; ни zapret, ни прокси это не обходят"
    );
    assert!(s.fixes.is_empty());
    assert_eq!(s.groups[0].best, None);
    e.foreign_syn_dead = Some(false);
    assert!(detect(&e, &out, &ctx()).is_empty());
}

#[test]
fn r6_with_peer_blames_peer() {
    let mut e = evidence();
    e.voice = Some(voice(vec![42], 20));
    e.neko = Some(vec![Conn {
        host: "c-fra.discord.media".into(),
        port: 2096,
        route: Route::Proxy,
        outbound_ms: Some(100),
    }]);
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        vec![Pattern::R6DaveStalled {
            alone: false,
            control_route: Some("через VPN".into())
        }]
    );
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(s.explanations.iter().any(|x| x
        == "сеть в порядке: соединение с голосовым сервером есть, но собеседник не прислал ключи шифрования"));
    assert_eq!(
        s.fixes,
        vec![Fix::Manual {
            text: "пусть собеседник перезайдёт или перезапустит Discord; проверить себя — зайти в канал одному".into()
        }]
    );
    // Not long enough yet.
    e.voice = Some(voice(vec![42], 10));
    assert!(detect(&e, &empty(), &ctx()).is_empty());
}

#[test]
fn r6_alone_blames_us() {
    let mut e = evidence();
    e.voice = Some(voice(vec![], 20));
    e.neko = Some(vec![Conn {
        host: "c-fra.discord.media".into(),
        port: 2096,
        route: Route::Direct,
        outbound_ms: Some(100),
    }]);
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        vec![Pattern::R6DaveStalled {
            alone: true,
            control_route: Some("напрямую".into())
        }]
    );
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(!s.explanations.iter().any(|x| x.contains("собеседник")));
    assert!(s.fixes.contains(&Fix::Manual {
        text: "Настройки → Маршрутизация → Basic: убрать `discord.media` из Direct, добавить в Proxy"
            .into()
    }));
}

#[test]
fn several_patterns_at_once() {
    let out = outcome(group(Protocol::Udp, "104.29.1.5:19300"), vec![]);
    let mut e = evidence();
    e.voice = Some(voice(vec![42], 30));
    let p = detect(&e, &out, &ctx());
    assert_eq!(p.len(), 2);
    assert!(matches!(p[0], Pattern::R2VoiceUdp { .. }));
    assert!(matches!(p[1], Pattern::R6DaveStalled { alone: false, .. }));
}

#[test]
fn only_proxy_works_text() {
    let g = group(Protocol::Udp, "104.29.1.5:19300");
    let out = outcome(
        g.clone(),
        vec![
            (PathKind::ProxyPoint { rule: point_rule() }, ok()),
            (PathKind::Current, bad()),
            (PathKind::Direct, bad()),
        ],
    );
    let s = summarize(
        &Verdict::Culprits(vec![g]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    let gs = &s.groups[0];
    assert_eq!(gs.best, Some(0));
    assert_eq!(gs.seen, "UDP уходит, ответов нет (10 туда, 0 обратно)");
    assert_eq!(
        gs.explanation,
        "провайдер блокирует адрес; zapret не помогает; через прокси работает"
    );
    let Fix::AddRule { rule } = &s.fixes[0] else {
        panic!("{:?}", s.fixes)
    };
    assert_eq!(rule.name, "diag-discord");
    assert_eq!(s.profile, "Discord");
    assert_eq!(s.id, ID);
}

#[test]
fn nothing_works_text() {
    let g = group(Protocol::Udp, "104.29.1.5:19300");
    let out = outcome(
        g.clone(),
        vec![(PathKind::Current, bad()), (PathKind::Direct, bad())],
    );
    let s = summarize(
        &Verdict::Culprits(vec![g]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    assert_eq!(s.groups[0].best, None);
    assert!(s.groups[0].explanation.starts_with(
        "адрес недоступен ни одним путём, включая прокси: вероятно, проблема на стороне сервиса или прокси"
    ));
    assert!(s.groups[0].explanation.contains("Напрямую мимо всего"));
    assert!(s.fixes.is_empty());
}

#[test]
fn no_flows_text() {
    let s = summarize(&Verdict::NoFlows, &empty(), &[], &ctx(), ID.into(), 45);
    assert_eq!(
        s.verdict_text,
        "программа не соединялась за 45 с: запустите её и повторите"
    );
    let s = summarize(&Verdict::Clean, &empty(), &[], &ctx(), ID.into(), 45);
    assert_eq!(
        s.verdict_text,
        "соединения программы проходят; если проблема есть, она не в сети"
    );
}

#[test]
fn other_templates_and_fixes() {
    let g = group(Protocol::Udp, "104.29.1.5:19300");
    let v = Verdict::Culprits(vec![g.clone()]);
    // Direct only.
    let out = outcome(
        g.clone(),
        vec![(PathKind::Direct, ok()), (PathKind::Current, bad())],
    );
    let s = summarize(&v, &out, &[], &ctx(), ID.into(), 30);
    assert_eq!(
        s.groups[0].explanation,
        "мешает текущая стратегия/маршрутизация"
    );
    assert!(matches!(s.fixes[..], [Fix::TurnOff { .. }]));
    // A strategy.
    let out = outcome(
        g.clone(),
        vec![(
            PathKind::Strategy {
                name: "ALT11".into(),
            },
            ok(),
        )],
    );
    let s = summarize(&v, &out, &[], &ctx(), ID.into(), 30);
    assert_eq!(
        s.groups[0].explanation,
        "DPI провайдера; помогает приём ALT11"
    );
    assert_eq!(
        s.fixes,
        vec![Fix::ApplyStrategy {
            name: "ALT11".into()
        }]
    );
    // A zapret2 trick: every working trick of the group, ranked.
    let trick = |n: u32| PathKind::Trick {
        candidate: Candidate {
            source: "zapret2/ALT11".into(),
            profile: n,
            text: format!(
                "--wf-udp=1 --filter-udp=19294-19344 --filter-l7=discord --lua-desync=fake{n}"
            ),
        },
    };
    let out = outcome(
        g,
        vec![(trick(1), ok()), (trick(2), ok()), (trick(3), bad())],
    );
    let s = summarize(&v, &out, &[], &ctx(), ID.into(), 30);
    let [Fix::ApplyHubCircular { tricks, filter }] = &s.fixes[..] else {
        panic!("{:?}", s.fixes)
    };
    assert_eq!(filter, "--filter-udp=19294-19344 --filter-l7=discord");
    assert_eq!(tricks.len(), 2);
    assert_eq!(tricks[0].found, "2026-10-22");
    assert_eq!(tricks[0].source, "zapret2/ALT11");
    assert!(tricks[0].text.ends_with("fake1"));
    assert_eq!(
        (tricks[0].ok, tricks[0].total, tricks[0].median_ms),
        (3, 3, Some(40))
    );
}

#[test]
fn live_only_auto_not_checked() {
    let mut c = ctx();
    c.profile = profile(ProbeKind::Live);
    let g = group(Protocol::Udp, "1.2.3.4:7777");
    let out = outcome(
        g.clone(),
        vec![
            (PathKind::Current, Score::default()),
            (PathKind::Direct, Score::default()),
        ],
    );
    let s = summarize(&Verdict::Culprits(vec![g]), &out, &[], &c, ID.into(), 30);
    assert!(
        s.explanations
            .contains(&"не проверяется автоматически: нужен режим „со мной“".to_owned())
    );
}

#[test]
fn warn_breaks_only_regressions() {
    let before = vec![
        ("YouTube: googlevideo.com".to_owned(), true),
        ("Discord: discord.com".to_owned(), true),
        ("DbD: bhvrdbd.com".to_owned(), false),
    ];
    let after = vec![
        ("YouTube: googlevideo.com".to_owned(), false),
        ("Discord: discord.com".to_owned(), true),
        ("DbD: bhvrdbd.com".to_owned(), false),
    ];
    assert_eq!(
        warn_breaks(&before, &after),
        vec!["сломает: YouTube (googlevideo.com не отвечает)".to_owned()]
    );
}

fn summary(id: String, profile: &str) -> diag::summary::Summary {
    let mut s = summarize(&Verdict::Clean, &empty(), &[], &ctx(), id, 30);
    s.profile = profile.into();
    s
}

#[test]
fn keeps_last_20() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    assert_eq!(dir, tmp.path().join("diag"));
    for i in 0..25 {
        report::save(
            &dir,
            &summary(format!("2026-10-22_12-00-{i:02}"), "Discord"),
        )
        .unwrap();
    }
    let list = report::list(&dir).unwrap();
    assert_eq!(list.len(), 20);
    assert_eq!(list[0].0, "2026-10-22_12-00-24");
    assert_eq!(list[19].0, "2026-10-22_12-00-05");
    let files = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(files, 20);
    let s = report::load(&dir, "2026-10-22_12-00-07").unwrap();
    assert_eq!(s.id, "2026-10-22_12-00-07");
    assert!(report::load(&dir, "..\\x").is_err());
}

#[test]
fn list_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    report::save(&dir, &summary("2026-10-21_09-00-00".into(), "YouTube")).unwrap();
    report::save(&dir, &summary("2026-10-22_08-00-00".into(), "Discord")).unwrap();
    report::save(&dir, &summary("2026-09-30_23-59-59".into(), "DbD")).unwrap();
    assert_eq!(
        report::list(&dir).unwrap(),
        vec![
            ("2026-10-22_08-00-00".to_owned(), "Discord".to_owned()),
            ("2026-10-21_09-00-00".to_owned(), "YouTube".to_owned()),
            ("2026-09-30_23-59-59".to_owned(), "DbD".to_owned()),
        ]
    );
}

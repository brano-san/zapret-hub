use diag::candidates::Candidate;
use diag::culprits::{Fault, Group, Verdict};
use diag::evaluate::Score;
use diag::logs::discord::{Dave, Voice};
use diag::logs::neko::{Conn, Route};
use diag::paths::{Ctx, PathKind};
use diag::patterns::{Evidence, Pattern, Proxied, detect, proxied_domains};
use diag::profiles::{ProbeKind, ProbeSpec, Profile};
use diag::report;
use diag::run::{GroupResult, Outcome};
use diag::summary::{Fix, TurnOffTarget, add_rule, summarize, warn_breaks};
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
        via_router: false,
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
        proxy_set: true,
        proxy_ok: true,
        ranking: vec![],
        strategies: vec![],
        zapret2: vec![],
        rules: vec![Rule {
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
        stopped: false,
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
        proxied: vec![],
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
        stopped: false,
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
        from_port: None,
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
        from_port: None,
    }]);
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        vec![Pattern::R6DaveStalled {
            alone: false,
            control_route: Some("через VPN".into()),
            direct: false,
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
fn r6_alone_is_not_a_fault() {
    // Alone, DAVE waits for a second participant: normal (domain review, 3 local sessions).
    let mut e = evidence();
    e.voice = Some(voice(vec![], 20));
    e.neko = Some(vec![Conn {
        host: "c-fra.discord.media".into(),
        port: 2096,
        route: Route::Direct,
        outbound_ms: Some(100),
        from_port: None,
    }]);
    let p = detect(&e, &empty(), &ctx());
    assert!(p.is_empty(), "{p:?}");
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(s.fixes.is_empty());
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
        vec![
            (PathKind::Current, bad()),
            (PathKind::Direct, bad()),
            (PathKind::ProxyPoint { rule: point_rule() }, bad()),
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
    assert_eq!(s.groups[0].best, None);
    assert!(s.groups[0].explanation.starts_with(
        "адрес недоступен ни одним путём, включая прокси: вероятно, проблема на стороне сервиса или прокси"
    ));
    assert!(s.groups[0].explanation.contains("Напрямую мимо всего"));
    assert!(s.fixes.is_empty());
}

#[test]
fn nothing_works_proxy_not_checked_text() {
    let g = group(Protocol::Udp, "104.29.1.5:19300");
    let mut out = outcome(
        g.clone(),
        vec![(PathKind::Current, bad()), (PathKind::Direct, bad())],
    );
    out.per_group[0].skipped = vec!["Прокси точечно: прокси не отвечает".into()];
    let s = summarize(
        &Verdict::Culprits(vec![g.clone()]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    assert_eq!(
        s.groups[0].explanation,
        "адрес недоступен ни одним проверенным путём; путь через прокси не проверен: прокси не отвечает. Проверено: Как сейчас, Напрямую мимо всего"
    );
    // No proxy in the settings, and a strategy left out: the reason is the setting.
    out.per_group[0].skipped = vec![
        "Стратегия B: фильтр не покрывает UDP 19300".into(),
        "Прокси точечно: прокси не задан в настройках".into(),
    ];
    let s = summarize(
        &Verdict::Culprits(vec![g.clone()]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    assert!(
        s.groups[0]
            .explanation
            .starts_with("адрес недоступен ни одним проверенным путём; путь через прокси не проверен: прокси не задан в настройках."),
        "{}",
        s.groups[0].explanation
    );
    // Proxy measured and failing: the full text.
    let out = outcome(
        g.clone(),
        vec![
            (PathKind::Current, bad()),
            (PathKind::ProxyPoint { rule: point_rule() }, bad()),
        ],
    );
    let s = summarize(
        &Verdict::Culprits(vec![g.clone()]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    assert!(s.groups[0].explanation.contains("включая прокси"));
    // The proxy measured, another path left out: still the full text.
    let mut out = outcome(
        g.clone(),
        vec![(PathKind::ProxyPoint { rule: point_rule() }, bad())],
    );
    out.per_group[0].skipped = vec!["Стратегия B: фильтр не покрывает UDP 19300".into()];
    let s = summarize(
        &Verdict::Culprits(vec![g]),
        &out,
        &[],
        &ctx(),
        ID.into(),
        30,
    );
    assert!(s.groups[0].explanation.contains("включая прокси"));
}

#[test]
fn a_report_that_cannot_take_its_place_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    std::fs::create_dir_all(dir.join(format!("{ID}.json"))).unwrap();
    let s = summarize(&Verdict::Clean, &empty(), &[], &ctx(), ID.into(), 30);
    assert!(report::save(&dir, &s).is_err());
}

#[test]
fn same_pattern_once() {
    let extend = PathKind::ExtendRule {
        rule: "DBD Process".into(),
        protocol: Protocol::Both,
    };
    let mut out = outcome(
        group(Protocol::Tcp, "9.9.9.9:443"),
        vec![(extend.clone(), ok())],
    );
    out.per_group.push(GroupResult {
        group: group(Protocol::Tcp, "8.8.8.8:443"),
        results: vec![(extend, ok())],
        skipped: vec![],
        not_checked: vec![],
    });
    let p = detect(&evidence(), &out, &ctx());
    assert_eq!(p.len(), 1);
    let s = summarize(&Verdict::Clean, &out, &p, &ctx(), ID.into(), 30);
    assert_eq!(s.explanations.len(), 1);
    assert_eq!(s.fixes.len(), 1);
}

#[test]
fn no_flows_text() {
    let s = summarize(&Verdict::NoFlows, &empty(), &[], &ctx(), ID.into(), 45);
    assert_eq!(
        s.verdict_text,
        "программа не открывала новых соединений за 45 с: запустите её и повторите проблему во время наблюдения"
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
    assert!(matches!(
        s.fixes[..],
        [Fix::TurnOff {
            target: TurnOffTarget::Current,
            ..
        }]
    ));
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

#[test]
fn list_skips_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    report::save(&dir, &summary("2026-10-21_09-00-00".into(), "YouTube")).unwrap();
    std::fs::write(dir.join("2026-10-22_08-00-00.json"), "{ broken").unwrap();
    assert_eq!(
        report::list(&dir).unwrap(),
        vec![("2026-10-21_09-00-00".to_owned(), "YouTube".to_owned())]
    );
    assert!(!dir.join("2026-10-21_09-00-00.json.tmp").exists());
}

#[test]
fn save_keeps_the_saved_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    for i in 0..20 {
        report::save(
            &dir,
            &summary(format!("2026-10-22_12-00-{i:02}"), "Discord"),
        )
        .unwrap();
    }
    // Older than all 20 (a clock set back): kept anyway.
    report::save(&dir, &summary("2026-01-01_00-00-00".into(), "Discord")).unwrap();
    assert!(report::load(&dir, "2026-01-01_00-00-00").is_ok());
}

#[test]
fn a_report_with_fewer_fields_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    std::fs::create_dir_all(&dir).unwrap();
    let text = r#"{"id": "2026-10-22_12-00-00", "profile": "Discord",
        "groups": [{"seen": "UDP", "best": null}]}"#;
    std::fs::write(dir.join("2026-10-22_12-00-00.json"), text).unwrap();
    let s = report::load(&dir, "2026-10-22_12-00-00").unwrap();
    assert_eq!(
        (s.profile.as_str(), s.groups[0].seen.as_str()),
        ("Discord", "UDP")
    );
    assert!(s.fixes.is_empty() && !s.canceled);
}

#[test]
fn a_report_from_before_via_router_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    std::fs::create_dir_all(&dir).unwrap();
    let text = r#"{"id": "2026-10-22_12-00-00", "profile": "Discord",
        "groups": [{"group": {"key": "35.217.19.0/24",
            "members": [{"dst": "35.217.19.82:50004", "up_packets": 285}],
            "representatives": [{"dst": "35.217.19.82:50004"}]}}]}"#;
    std::fs::write(dir.join("2026-10-22_12-00-00.json"), text).unwrap();
    let s = report::load(&dir, "2026-10-22_12-00-00").unwrap();
    let member = &s.groups[0].group.members[0];
    assert_eq!((member.up_packets, member.via_router), (285, false));
}

#[test]
fn an_old_report_that_stays_does_not_fail_the_save() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = report::dir(tmp.path());
    // Cannot be removed as a file.
    std::fs::create_dir_all(dir.join("2026-01-01_00-00-00.json")).unwrap();
    for i in 0..21 {
        report::save(
            &dir,
            &summary(format!("2026-10-22_12-00-{i:02}"), "Discord"),
        )
        .unwrap();
    }
    assert!(dir.join("2026-01-01_00-00-00.json").exists());
    assert_eq!(report::list(&dir).unwrap().len(), 20);
}

#[test]
fn apply_unites_with_the_rule_of_that_name() {
    let rule = |protocol, port: &str, ip: &str| Rule {
        name: "diag-discord".into(),
        enabled: true,
        protocol,
        ports: vec![port.into()],
        ips: vec![ip.into()],
        domains: vec![],
        apps: vec![],
    };
    let mut list = vec![Rule {
        enabled: false,
        ..rule(Protocol::Udp, "19294", "104.29.152.0/24")
    }];
    add_rule(&mut list, rule(Protocol::Udp, "19300", "104.29.160.0/24"));
    add_rule(&mut list, rule(Protocol::Tcp, "443", "104.29.160.0/24"));
    assert_eq!(list.len(), 1);
    let r = &list[0];
    assert!(r.enabled);
    assert_eq!(r.protocol, Protocol::Both);
    assert_eq!(r.ports, ["19294", "19300", "443"]);
    assert_eq!(r.ips, ["104.29.152.0/24", "104.29.160.0/24"]);
    add_rule(
        &mut list,
        Rule {
            name: "diag-game1".into(),
            ..rule(Protocol::Udp, "7777", "5.6.7.0/24")
        },
    );
    assert_eq!(list.len(), 2);
}

#[test]
fn r1_a_proxied_domain_sent_direct() {
    const DBD_API: &str = "steam.live.bhvrdbd.com";
    let conn = |host: &str, from_port| Conn {
        host: host.into(),
        port: 443,
        route: Route::Direct,
        outbound_ms: Some(20),
        from_port,
    };
    let conns = vec![
        conn(DBD_API, Some(63510)),
        conn(DBD_API, Some(63511)),
        conn("1.2.3.4", Some(63511)),
        conn("other.example.com", Some(1)),
        conn("other.example.com", None),
    ];
    let proxied = |port| FlowCounters {
        local_port: port,
        via_system_proxy: true,
        via_router: false,
        ..flow(Protocol::Tcp, "127.0.0.1:18082")
    };
    let mut flows = vec![proxied(63510), proxied(63511)];
    flows.push(flow(Protocol::Tcp, "1.1.1.1:443"));
    assert_eq!(proxied_domains(&flows, &conns), [DBD_API]);
    let many: Vec<Conn> = (0..9)
        .map(|i| conn(&format!("h{i}.example.com"), Some(63510)))
        .collect();
    assert_eq!(proxied_domains(&flows, &many).len(), 5);

    let mut e = evidence();
    e.neko = Some(conns);
    e.neko_routes = Some(diag::logs::neko_routes::Routes {
        direct: vec!["domain:bhvrdbd.com".into()],
        ..Default::default()
    });
    e.proxied = vec![
        Proxied {
            domain: DBD_API.into(),
            direct_ok: false,
            proxy_ok: true,
        },
        Proxied {
            domain: "fine.example.com".into(),
            direct_ok: true,
            proxy_ok: true,
        },
    ];
    let p = detect(&e, &empty(), &ctx());
    assert_eq!(
        p,
        [Pattern::R1ViaSystemProxy {
            domain: DBD_API.into(),
            direct_ok: false,
            proxy_ok: true,
            route: Some("напрямую".into()),
            direct: true,
            rule: Some("domain:bhvrdbd.com".into()),
        }]
    );
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(s.verdict_text.contains(DBD_API), "{}", s.verdict_text);
    assert!(
        matches!(&s.fixes[..], [Fix::Manual { text }] if text.contains("убрать «domain:bhvrdbd.com» из Direct")),
        "{:?}",
        s.fixes
    );
    // Via the proxy it fails too: nothing to move in NekoBox.
    e.proxied[0].proxy_ok = false;
    let p = detect(&e, &empty(), &ctx());
    let s = summarize(&Verdict::Clean, &empty(), &p, &ctx(), ID.into(), 30);
    assert!(s.fixes.is_empty());
    assert_eq!(s.explanations.len(), 1, "{:?}", s.explanations);
}

#[test]
fn r1_a_proxied_domain_already_through_the_vpn_is_not_a_failure() {
    let p = |route: Option<&str>, direct, rule: Option<&str>, direct_ok, proxy_ok| {
        Pattern::R1ViaSystemProxy {
            domain: "steam.live.bhvrdbd.com".into(),
            direct_ok,
            proxy_ok,
            route: route.map(str::to_owned),
            direct,
            rule: rule.map(str::to_owned),
        }
    };
    let verdict = |pattern| {
        summarize(&Verdict::Clean, &empty(), &[pattern], &ctx(), ID.into(), 30).verdict_text
    };
    const CLEAN: &str = "соединения программы проходят; если проблема есть, она не в сети";
    // Direct fails, the VPN works, and NekoBox already sends it through the VPN.
    assert_eq!(
        verdict(p(Some("через VPN"), false, None, false, true)),
        CLEAN
    );
    assert_eq!(
        verdict(p(None, false, Some("domain:bhvrdbd.com"), false, true)),
        CLEAN,
        "the proxy list"
    );
    // The way it goes now fails: named.
    assert!(verdict(p(Some("напрямую"), true, None, false, true)).contains("не работает"));
    assert!(verdict(p(Some("через VPN"), false, None, true, false)).contains("не работает"));
    // Not known where it goes: only when both fail.
    assert_eq!(verdict(p(None, false, None, false, true)), CLEAN);
    assert!(verdict(p(None, false, None, false, false)).contains("не работает"));
}

#[test]
fn every_failed_system_check_is_an_explanation() {
    use zapret::checks::{CheckId, CheckLevel, SystemCheck};
    let check = |id, level, detail: &str| SystemCheck {
        id,
        level,
        detail: detail.into(),
        action: None,
    };
    let lines = diag::summary::check_lines(&[
        check(CheckId::Bfe, CheckLevel::Ok, ""),
        check(CheckId::Proxy, CheckLevel::Warn, "127.0.0.1:18082"),
        check(CheckId::Conflicts, CheckLevel::Problem, "GoodbyeDPI"),
    ]);
    assert_eq!(
        lines,
        [
            "системная проверка, внимание: включён системный прокси (127.0.0.1:18082)",
            "системная проверка, проблема: установлены службы других обходов (GoodbyeDPI)",
        ]
    );
}

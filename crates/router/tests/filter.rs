use router::divert::{
    Engine, FILTER_BUDGET, MAX_FILTER_RANGES, capture_filter, filter, pick_dll, refused,
    refused_for_good,
};
use router::rules::{self, Preset, Protocol, Rule, Table};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

fn rule(ports: &[&str], ips: &[&str]) -> Rule {
    Rule {
        name: "r".into(),
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
        domains: vec![],
        ..rules::preset(Preset::DiscordVoice)
    }
}

#[test]
fn the_voice_preset_makes_the_scripts_filter() {
    assert_eq!(
        filter(&Table::new(&[rules::preset(Preset::DiscordVoice)])).unwrap(),
        "outbound and !loopback and ip and ((udp and ((udp.DstPort >= 19294 and udp.DstPort <= 19344) \
         or udp.DstPort >= 50000) and ((ip.DstAddr >= 35.217.0.0 and ip.DstAddr <= 35.217.255.255) \
         or (ip.DstAddr >= 104.29.0.0 and ip.DstAddr <= 104.29.255.255))))"
    );
}

#[test]
fn single_values_and_missing_ports_are_short() {
    assert_eq!(
        filter(&Table::new(&[
            rule(&["443"], &["1.1.1.1"]),
            rule(&[], &["10.0.0.0/8"])
        ]))
        .unwrap(),
        "outbound and !loopback and ip and ((udp and (udp.DstPort == 443) and (ip.DstAddr == 1.1.1.1)) \
         or (udp and ((ip.DstAddr >= 10.0.0.0 and ip.DstAddr <= 10.255.255.255))))"
    );
    assert_eq!(filter(&Table::new(&[])), None, "no rules, no handle");
    assert_eq!(
        filter(&Table::new(&[Rule {
            enabled: false,
            ..rule(&[], &["1.1.1.1"])
        }])),
        None
    );
}

#[test]
fn overlapping_and_adjacent_subnets_merge() {
    let table = Table::new(&[rule(&[], &["10.0.0.0/24", "10.0.1.0/24", "10.0.0.128/25"])]);
    assert_eq!(table.rules[0].ranges, [(0x0A00_0000, 0x0A00_01FF)]);
}

fn named(rule: Rule, names: &[(&str, &str)]) -> Table {
    let names: Vec<(Ipv4Addr, String)> = names
        .iter()
        .map(|(ip, n)| (ip.parse().unwrap(), n.to_string()))
        .collect();
    Table::new(&[rule]).with_names(&names)
}

fn domain_rule(protocol: Protocol, ports: &[&str]) -> Rule {
    Rule {
        protocol,
        domains: vec!["example.com".into()],
        ..rule(ports, &[])
    }
}

#[test]
fn a_domain_rule_without_answers_captures_only_first_syns() {
    assert_eq!(
        capture_filter(
            &Table::new(&[rules::preset(Preset::Discord)]),
            Some(61000),
            &[]
        )
        .unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack) or (tcp and tcp.SrcPort == 61000))",
        "only the first SYNs: no UDP, no TCP past the handshake"
    );
    assert_eq!(
        capture_filter(
            &Table::new(&[domain_rule(Protocol::Udp, &["443"])]),
            Some(61000),
            &[]
        ),
        None,
        "a domain UDP rule without answers captures no UDP at all"
    );
    let voice = Table::new(&[rules::preset(Preset::DiscordVoice)]);
    assert_eq!(
        capture_filter(&voice, Some(61000), &[]),
        filter(&voice),
        "a UDP rule needs no acceptor part"
    );
}

#[test]
fn a_domain_tcp_rule_catches_first_syns_on_its_ports() {
    assert_eq!(
        capture_filter(
            &Table::new(&[domain_rule(Protocol::Tcp, &["443", "8000-8100"])]),
            None,
            &[]
        )
        .unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack and (tcp.DstPort == 443 or (tcp.DstPort >= 8000 and tcp.DstPort <= 8100))))"
    );
}

#[test]
fn resolved_ips_and_the_acceptor_replies_are_captured() {
    let table = named(
        rules::preset(Preset::Discord),
        &[
            ("162.159.135.232", "discord.com"),
            ("162.159.136.232", "gateway.discord.gg"),
            ("8.8.8.8", "example.com"),
        ],
    );
    assert_eq!(
        capture_filter(&table, Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and (((udp or tcp) and (ip.DstAddr == 162.159.135.232 or ip.DstAddr == 162.159.136.232)) or (tcp and tcp.Syn and !tcp.Ack) or (tcp and tcp.SrcPort == 61000))"
    );
}

#[test]
fn tcp_and_both_rules_use_their_own_port_fields() {
    let tcp = Rule {
        name: "t".into(),
        protocol: Protocol::Tcp,
        ..rule(&["443"], &["1.1.1.1"])
    };
    let both = Rule {
        name: "b".into(),
        protocol: Protocol::Both,
        ..rule(&["443", "50000-65535"], &["2.2.2.2"])
    };
    assert_eq!(
        capture_filter(&Table::new(&[tcp, both]), Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and (tcp.DstPort == 443) and (ip.DstAddr == 1.1.1.1)) or (((udp and (udp.DstPort == 443 or udp.DstPort >= 50000)) or (tcp and (tcp.DstPort == 443 or tcp.DstPort >= 50000))) and (ip.DstAddr == 2.2.2.2)) or (tcp and tcp.SrcPort == 61000))"
    );
}

#[test]
fn a_rule_captures_only_its_own_protocol() {
    let tcp = Rule {
        protocol: Protocol::Tcp,
        ..rule(&["443"], &["1.1.1.1"])
    };
    let text = capture_filter(&Table::new(&[tcp]), Some(61000), &[]).unwrap();
    assert!(
        !text.contains("udp"),
        "a tcp-only rule emits no udp clause: {text}"
    );
    let udp = rule(&["443"], &["1.1.1.1"]);
    let text = capture_filter(&Table::new(&[udp]), Some(61000), &[]).unwrap();
    assert!(
        !text.contains("tcp"),
        "a udp-only rule emits no tcp clause: {text}"
    );
}

#[test]
fn a_live_connection_keeps_its_destination_in_the_filter() {
    let held = ["162.159.135.232".parse().unwrap()];
    assert_eq!(
        capture_filter(&Table::new(&[]), Some(61000), &held).unwrap(),
        "outbound and !loopback and ip and ((tcp and (ip.DstAddr == 162.159.135.232)) or (tcp and tcp.SrcPort == 61000))"
    );
    let held: Vec<Ipv4Addr> = ["10.0.0.2", "10.0.0.1", "10.0.0.2"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        capture_filter(&Table::new(&[]), None, &held).unwrap(),
        "outbound and !loopback and ip and ((tcp and ((ip.DstAddr >= 10.0.0.1 and ip.DstAddr <= 10.0.0.2))))"
    );
}

#[test]
fn too_many_ranges_merge_the_closest_ones() {
    let ips: Vec<String> = (0..65).map(|i| format!("10.{i}.0.0/24")).collect();
    let ips: Vec<&str> = ips.iter().map(String::as_str).collect();
    let table = Table::new(&[rule(&["50000-65535"], &ips)]);
    let text = filter(&table).unwrap();
    assert_eq!(text.matches("ip.DstAddr >=").count(), 64);
    assert!(
        text.starts_with(
            "outbound and !loopback and ip and ((udp and (udp.DstPort >= 50000) and ((ip.DstAddr >= 10.0.0.0 and ip.DstAddr <= 10.1.0.255) or "
        ),
        "{text}"
    );
    assert!(text.contains("(ip.DstAddr >= 10.64.0.0 and ip.DstAddr <= 10.64.0.255)"));
    assert_eq!(
        table.match_udp("10.0.200.1:50001".parse().unwrap()),
        None,
        "the hub matches exactly"
    );
}

#[test]
fn merging_picks_the_smallest_gap_across_parts() {
    // 63 far-apart /24s in one rule, two close ones in another: the close pair merges.
    let far: Vec<String> = (0..63).map(|i| format!("10.{}.0.0/24", i * 2)).collect();
    let far: Vec<&str> = far.iter().map(String::as_str).collect();
    let text = filter(&Table::new(&[
        rule(&[], &far),
        rule(&[], &["192.168.0.0/24", "192.168.1.128/25"]),
    ]))
    .unwrap();
    assert_eq!(text.matches("ip.DstAddr >=").count(), 64);
    assert!(
        text.contains("(ip.DstAddr >= 192.168.0.0 and ip.DstAddr <= 192.168.1.255)"),
        "{text}"
    );
}

/// Tests WinDivert compiles: every `and`/`or` joins two of them.
fn tests(text: &str) -> usize {
    text.matches(" and ").count() + text.matches(" or ").count() + 1
}

#[test]
fn same_syn_parts_are_written_once() {
    let a = domain_rule(Protocol::Tcp, &[]);
    let b = domain_rule(Protocol::Both, &[]);
    let text = capture_filter(&Table::new(&[a, b]), None, &[]).unwrap();
    assert_eq!(text.matches("tcp.Syn").count(), 1, "{text}");
}

#[test]
fn both_discord_presets_with_many_answers_fit_the_budget() {
    let names: Vec<(Ipv4Addr, String)> = (0..400u32)
        .map(|i| {
            let name = if i % 2 == 0 {
                "discord.com"
            } else {
                "x.discord.media"
            };
            (Ipv4Addr::from(0x0A00_0000 + i * 1000), name.to_string())
        })
        .collect();
    let table = Table::new(&[
        rules::preset(Preset::Discord),
        rules::preset(Preset::DiscordVoice),
    ])
    .with_names(&names);
    let held: Vec<Ipv4Addr> = (0..100u32)
        .map(|i| Ipv4Addr::from(0xC000_0000 + i * 7))
        .collect();
    let text = capture_filter(&table, Some(61000), &held).unwrap();
    assert!(
        tests(&text) <= FILTER_BUDGET,
        "{} tests: {text}",
        tests(&text)
    );
    assert!(text.matches("ip.DstAddr").count() <= 2 * MAX_FILTER_RANGES);
    assert!(text.contains("tcp.Syn") && text.ends_with("(tcp and tcp.SrcPort == 61000))"));
}

#[test]
fn many_ports_still_fit_the_budget() {
    let ports: Vec<String> = (0..150).map(|i| (1000 + 2 * i).to_string()).collect();
    let ports: Vec<&str> = ports.iter().map(String::as_str).collect();
    let both = Rule {
        protocol: Protocol::Both,
        domains: vec!["example.com".into()],
        ..rule(&ports, &["1.1.1.1", "2.2.2.2"])
    };
    let table = Table::new(&[both]);
    let text = capture_filter(&table, Some(61000), &[]).unwrap();
    assert!(
        tests(&text) <= FILTER_BUDGET,
        "{} tests: {text}",
        tests(&text)
    );
    assert_eq!(
        table.matches(Protocol::Tcp, "1.1.1.1:1001".parse().unwrap()),
        None,
        "the hub matches exactly"
    );
}

#[test]
fn the_dll_comes_from_the_running_engine() {
    let flowseal = Path::new(r"E:\Zapret");
    let winws2 = Path::new(r"D:\zapret2\binaries\windows-x86_64");
    let fs_dll = PathBuf::from(r"E:\Zapret\bin\WinDivert.dll");
    let z2_dll = PathBuf::from(r"D:\zapret2\binaries\windows-x86_64\WinDivert.dll");
    let all = |_: &Path| true;
    assert_eq!(
        pick_dll(Some(Engine::Zapret2), Some(flowseal), Some(winws2), all).unwrap(),
        z2_dll
    );
    assert_eq!(
        pick_dll(Some(Engine::Flowseal), Some(flowseal), Some(winws2), all).unwrap(),
        fs_dll
    );
    assert_eq!(
        pick_dll(None, Some(flowseal), Some(winws2), all).unwrap(),
        fs_dll,
        "Flowseal first"
    );
    let only_z2 = |path: &Path| path == z2_dll;
    assert_eq!(
        pick_dll(None, Some(flowseal), Some(winws2), only_z2).unwrap(),
        z2_dll
    );
    let err = pick_dll(
        Some(Engine::Flowseal),
        Some(flowseal),
        Some(winws2),
        only_z2,
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains(r"E:\Zapret\bin\WinDivert.dll"),
        "no fallback for a running engine: {err:#}"
    );
    assert!(pick_dll(None, None, None, all).is_err());
}

#[test]
fn a_missing_dll_is_named_with_the_windows_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("WinDivert.dll");
    let err = format!("{:#}", router::divert::Dll::load(&path).unwrap_err());
    assert!(
        err.contains("WinDivert.dll") && err.contains("cannot load"),
        "{err}"
    );
}

#[test]
fn a_refused_capture_filter_names_too_many_rules() {
    let invalid = std::io::Error::from_raw_os_error(87);
    assert!(refused(&invalid).starts_with("too many rules for the capture filter"));
    let other = std::io::Error::from_raw_os_error(5);
    assert!(refused(&other).starts_with("WinDivert refused the filter: "));
    assert!(refused_for_good(&invalid) && !refused_for_good(&other));
}

const DBD: &str = r"D:\Games\DeadByDaylight-Win64-Shipping.exe";

fn app_rule(protocol: Protocol, ports: &[&str]) -> Rule {
    Rule {
        protocol,
        domains: vec![],
        apps: vec!["DeadByDaylight-Win64-Shipping.exe".into()],
        ..rule(ports, &[])
    }
}

#[test]
fn an_app_rule_captures_first_syns_and_udp_only_while_its_program_runs() {
    let both = Table::new(&[app_rule(Protocol::Both, &[])]);
    assert_eq!(
        capture_filter(&both, Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack) or (tcp and tcp.SrcPort == 61000))",
        "the game is not running: SYNs only"
    );
    assert_eq!(
        capture_filter(&both.with_running(&[DBD]), Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack) or (udp) or (tcp and tcp.SrcPort == 61000))"
    );
    let udp = Table::new(&[app_rule(Protocol::Udp, &["7000-8000"])]);
    assert_eq!(filter(&udp), None, "nothing while the game is not running");
    assert_eq!(
        filter(&udp.clone().with_running(&[r"C:\x\chrome.exe"])),
        None
    );
    assert_eq!(
        filter(&udp.with_running(&[DBD])).unwrap(),
        "outbound and !loopback and ip and ((udp and ((udp.DstPort >= 7000 and udp.DstPort <= 8000))))"
    );
    let dbd = Table::new(&[rules::preset(Preset::Dbd)]);
    assert_eq!(
        filter(&dbd).unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack))",
        "domain and program share the SYN part"
    );
}

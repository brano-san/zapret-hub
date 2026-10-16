use router::divert::{Engine, filter, pick_dll};
use router::rules::{self, Preset, Rule, Table};
use std::path::{Path, PathBuf};

fn rule(ports: &[&str], ips: &[&str]) -> Rule {
    Rule {
        name: "r".into(),
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
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

#[test]
fn too_many_ranges_leave_the_ips_to_the_hub() {
    let ips: Vec<String> = (0..65).map(|i| format!("10.{i}.0.0/24")).collect();
    let ips: Vec<&str> = ips.iter().map(String::as_str).collect();
    let text = filter(&Table::new(&[rule(&["50000-65535"], &ips)])).unwrap();
    assert_eq!(
        text,
        "outbound and !loopback and ip and ((udp and (udp.DstPort >= 50000)))"
    );
    // no ports: all outbound UDP reaches the hub, which matches the IPs itself
    let text = filter(&Table::new(&[rule(&[], &ips)])).unwrap();
    assert_eq!(text, "outbound and !loopback and ip and ((udp))");
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

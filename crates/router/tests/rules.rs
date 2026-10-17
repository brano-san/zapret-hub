use router::rules::{self, Preset, Protocol, Rule, Table};

fn voice() -> Rule {
    rules::preset(Preset::DiscordVoice)
}

#[test]
fn the_discord_voice_preset_is_the_spec() {
    assert_eq!(
        voice(),
        Rule {
            name: "Discord голос".into(),
            enabled: true,
            protocol: Protocol::Udp,
            ports: vec!["19294-19344".into(), "50000-65535".into()],
            ips: vec!["104.29.0.0/16".into(), "35.217.0.0/16".into()],
            domains: vec!["discord.media".into()],
            apps: vec![],
        }
    );
    assert!(rules::check(&voice()).is_empty());
}

#[test]
fn a_file_round_trips_and_a_missing_one_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    assert_eq!(rules::load(&path).unwrap(), Vec::<Rule>::new());
    rules::save(&path, &[voice()]).unwrap();
    assert_eq!(rules::load(&path).unwrap(), [voice()]);
    assert!(std::fs::read_to_string(&path).unwrap().contains("[[rule]]"));
}

#[test]
fn a_notepad_file_with_a_bom_parses() {
    let text =
        "\u{feff}[[rule]]\r\nname = \"v\"\r\nprotocol = \"udp\"\r\nips = [\"1.2.3.0/24\"]\r\n";
    let parsed = rules::parse(text).unwrap();
    assert_eq!(parsed[0].name, "v");
    assert!(parsed[0].enabled, "enabled defaults to true");
    assert!(parsed[0].ports.is_empty());
}

#[test]
fn errors_name_the_line_and_the_field() {
    let text = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n\n\
                [[rule]]\nname = \"b\"\nprotocol = \"udp\"\nports = [\"70000\"]\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(text).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(9), Some("ports")));
    assert!(err.to_string().starts_with("line 9: ports: "), "{err}");
}

#[test]
fn names_follow_the_strategy_rules_plus_cyrillic() {
    let named = |name: &str| Rule {
        name: name.into(),
        ..voice()
    };
    assert!(rules::check(&named("Discord голос")).is_empty());
    for bad in ["", " ", "a/b", "-x", &"a".repeat(41)] {
        assert_eq!(rules::check(&named(bad))[0].field, "name", "{bad:?}");
    }
    let text = "[[rule]]\nname = \"Discord голос\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n\
                [[rule]]\nname = \"DISCORD ГОЛОС\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(text).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(6), Some("name")));
}

#[test]
fn ports_and_subnets_are_checked() {
    let with = |ports: &[&str], ips: &[&str]| Rule {
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
        domains: vec![],
        apps: vec![],
        ..voice()
    };
    let fields = |rule: Rule| {
        rules::check(&rule)
            .into_iter()
            .map(|e| e.field)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        fields(with(&["443", "1-65535"], &["10.0.0.1", "10.1.2.3/8"])),
        Vec::<String>::new()
    );
    for ports in [["0"], ["100-50"], ["65536"], ["x"]] {
        assert_eq!(fields(with(&ports, &["1.1.1.1"])), ["ports"], "{ports:?}");
    }
    for ips in [["300.1.1.1"], ["1.2.3.0/33"], ["discord.media"], ["::1"]] {
        assert_eq!(fields(with(&[], &ips)), ["ips"], "{ips:?}");
    }
    assert_eq!(fields(with(&[], &[])), ["ips"], "a rule needs a target");
}

#[test]
fn a_cli_rule_file_has_no_name() {
    let text = "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n";
    let rule = rules::parse_rule_file("Мой голос", text).unwrap();
    assert_eq!((rule.name.as_str(), rule.enabled), ("Мой голос", true));
    let named = "[rule]\nname = \"x\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    assert_eq!(
        rules::parse_rule_file("x", named)
            .unwrap_err()
            .field
            .as_deref(),
        Some("name")
    );
}

#[test]
fn the_whole_discord_preset_is_the_spec() {
    let whole = rules::preset(Preset::Discord);
    assert_eq!(
        (whole.name.as_str(), whole.protocol),
        ("Discord целиком", Protocol::Both)
    );
    assert_eq!(
        whole.domains,
        [
            "discord.com",
            "discord.gg",
            "discordapp.com",
            "discordapp.net",
            "discord.media"
        ]
    );
    assert!(whole.ips.is_empty() && whole.ports.is_empty());
    assert!(rules::check(&whole).is_empty());
}

#[test]
fn domains_are_ldh_names_without_scheme_or_path() {
    let with = |d: &[&str]| Rule {
        domains: d.iter().map(|s| s.to_string()).collect(),
        ips: vec![],
        apps: vec![],
        ..voice()
    };
    let fields = |r: Rule| {
        rules::check(&r)
            .into_iter()
            .map(|e| e.field)
            .collect::<Vec<_>>()
    };
    assert!(
        fields(with(&[
            "discord.com",
            "Media.Discord.GG",
            "xn--d1acufc.xn--p1ai",
            "a-b.example.",
            "1e100.net"
        ]))
        .is_empty()
    );
    let long = format!("{}.com", "a".repeat(64));
    for bad in [
        "https://discord.com",
        "discord.com/app",
        "discord.com:443",
        "*.discord.com",
        "-a.com",
        "a-.com",
        "a..com",
        "com",
        "дискорд.рф",
        "1.2.3.4",
        &long,
    ] {
        assert_eq!(fields(with(&[bad])), ["domains"], "{bad:?}");
    }
    assert_eq!(
        fields(with(&[])),
        ["ips"],
        "a rule needs ips, domains or apps"
    );
}

#[test]
fn tcp_and_both_parse() {
    for p in ["tcp", "both"] {
        let text =
            format!("[[rule]]\nname = \"a\"\nprotocol = \"{p}\"\ndomains = [\"discord.com\"]\n");
        assert_eq!(rules::parse(&text).unwrap()[0].domains, ["discord.com"]);
    }
    let err = rules::parse("[[rule]]\nname = \"a\"\nprotocol = \"tcp\"\ndomains = [\"x/y\"]\n")
        .unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(4), Some("domains")));
}

#[test]
fn import_sends_ip_lines_to_ips_and_names_to_domains() {
    let text =
        "\u{feff}# list\r\n104.29.0.0/16\r\ndiscord.media\r\nDiscord.Media\n\n35.217.0.0/16\n";
    let got = rules::import(&["104.29.0.0/16".into()], &[], text).unwrap();
    assert_eq!(got.ips, ["104.29.0.0/16", "35.217.0.0/16"]);
    assert_eq!(got.domains, ["discord.media"]);
    let err = rules::import(&[], &[], "1.1.1.1\nhttps://discord.com/\n").unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(2), Some("domains")));
    assert_eq!(
        rules::import(&[], &[], "300.1.1.1\n")
            .unwrap_err()
            .field
            .as_deref(),
        Some("ips")
    );
}

#[test]
fn domains_cover_themselves_and_subdomains() {
    assert!(rules::covers("discord.media", "discord.media"));
    assert!(rules::covers("discord.media", "c-fra08.discord.media"));
    assert!(!rules::covers("discord.media", "notdiscord.media"));
    assert!(!rules::covers("discord.media", "media"));
}

#[test]
fn resolved_names_add_ips_to_their_rules_by_protocol() {
    let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
    let table = Table::new(&[voice(), rules::preset(Preset::Discord)]).with_names(&[
        (ip("162.159.135.232"), "gateway.discord.gg".into()),
        (ip("35.214.1.1"), "c-fra08.discord.media".into()),
        (ip("8.8.8.8"), "dns.google".into()),
    ]);
    let at = |p, a: &str| table.matches(p, a.parse().unwrap());
    assert_eq!(
        at(Protocol::Tcp, "162.159.135.232:443"),
        Some("Discord целиком")
    );
    assert_eq!(
        at(Protocol::Udp, "35.214.1.1:50001"),
        Some("Discord голос"),
        "file order"
    );
    assert_eq!(at(Protocol::Tcp, "35.214.1.1:443"), Some("Discord целиком"));
    assert_eq!(
        at(Protocol::Tcp, "104.29.1.1:50001"),
        None,
        "the voice rule is UDP only"
    );
    assert_eq!(table.match_udp("8.8.8.8:53".parse().unwrap()), None);
    assert!(table.wants("cdn.discordapp.com") && !table.wants("example.com"));
    let off = Rule {
        enabled: false,
        ..rules::preset(Preset::Discord)
    };
    assert!(!Table::new(&[off]).wants("discord.com"));
}

#[test]
fn edits_go_by_name_without_case() {
    let mut list = vec![voice()];
    let dup = Rule {
        name: "discord ГОЛОС".into(),
        ..voice()
    };
    assert!(rules::add(&mut list, dup).is_err());
    rules::set_enabled(&mut list, "DISCORD голос", false).unwrap();
    assert!(!list[0].enabled);
    let other = Rule {
        name: "other".into(),
        ..voice()
    };
    assert!(rules::replace(&mut list, other).is_err());
    assert!(rules::find(&list, "nope").is_err());
    assert_eq!(
        rules::remove(&mut list, "discord голос").unwrap().name,
        "Discord голос"
    );
    assert!(list.is_empty());
}

#[test]
fn the_table_matches_enabled_rules_by_port_and_subnet() {
    let off = Rule {
        name: "off".into(),
        enabled: false,
        ips: vec!["8.8.8.0/24".into()],
        ..voice()
    };
    let table = Table::new(&[voice(), off]);
    let at = |addr: &str| table.match_udp(addr.parse().unwrap());
    assert_eq!(at("104.29.5.6:50001"), Some("Discord голос"));
    assert_eq!(at("35.217.0.1:19294"), Some("Discord голос"));
    assert_eq!(at("35.217.255.255:65535"), Some("Discord голос"));
    assert_eq!(at("35.217.0.1:19293"), None);
    assert_eq!(at("104.29.5.6:443"), None);
    assert_eq!(at("8.8.8.8:50001"), None, "a disabled rule matches nothing");
    assert!(Table::new(&[]).is_empty());
}

#[test]
fn an_unknown_field_is_named() {
    let err = rules::parse("[[rule]]\nname = \"a\"\ncolour = 1\n").unwrap_err();
    assert!(err.to_string().contains("colour"), "{err}");
}

#[test]
fn domain_length_limits_and_label_boundaries() {
    let with = |d: String| Rule {
        domains: vec![d],
        ips: vec![],
        ..voice()
    };
    let label = "a".repeat(63);
    assert!(rules::check(&with(format!("{label}.com"))).is_empty());
    // 4 x 63 + 3 dots + ".com" = 259 > 253
    let long = format!("{label}.{label}.{label}.{label}.com");
    assert_eq!(rules::check(&with(long))[0].field, "domains");
    assert!(!rules::covers("discord.media", "xdiscord.media"));
    assert!(rules::covers("discord.media", "x.discord.media"));
}

const DBD: &str = r"D:\Steam\steamapps\common\Dead by Daylight\DeadByDaylight\Binaries\Win64\DeadByDaylight-Win64-Shipping.exe";

fn app_rule(name: &str, protocol: Protocol, ports: &[&str], apps: &[&str]) -> Rule {
    Rule {
        name: name.into(),
        enabled: true,
        protocol,
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: vec![],
        domains: vec![],
        apps: apps.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn the_dbd_preset_is_the_spec() {
    let dbd = rules::preset(Preset::Dbd);
    assert_eq!(
        (dbd.name.as_str(), dbd.protocol),
        ("Dead by Daylight", Protocol::Both)
    );
    assert_eq!(dbd.apps, ["DeadByDaylight-Win64-Shipping.exe"]);
    assert_eq!(dbd.domains, ["bhvrdbd.com"]);
    assert!(dbd.ips.is_empty() && dbd.ports.is_empty());
    assert!(rules::check(&dbd).is_empty());
}

#[test]
fn apps_are_exe_names_or_full_paths() {
    let fields = |apps: &[&str]| {
        rules::check(&app_rule("a", Protocol::Udp, &[], apps))
            .into_iter()
            .map(|e| e.field)
            .collect::<Vec<_>>()
    };
    assert!(
        fields(&[
            "Discord.exe",
            "DeadByDaylight-Win64-Shipping.EXE",
            r"C:\Games\DbD\x.exe",
            "D:/Games/y.exe",
            r"\\nas\games\z.exe"
        ])
        .is_empty()
    );
    for bad in [
        "Discord",
        "",
        " ",
        r"Games\x.exe",
        "*.exe",
        "a?.exe",
        "x.exe:stream",
        r"C:\Games\notes.txt",
    ] {
        assert_eq!(fields(&[bad]), ["apps"], "{bad:?}");
    }
    assert_eq!(fields(&[]), ["ips"], "a rule needs ips, domains or apps");
}

#[test]
fn apps_parse_and_a_bad_one_names_its_line() {
    let ok = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\napps = [\"Discord.exe\"]\n";
    assert_eq!(rules::parse(ok).unwrap()[0].apps, ["Discord.exe"]);
    let bad =
        "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\napps = [\"Discord\"]\n";
    let err = rules::parse(bad).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(5), Some("apps")));
    let old = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    assert!(
        rules::parse(old).unwrap()[0].apps.is_empty(),
        "a stage-2 file still loads"
    );
}

#[test]
fn a_flow_matches_by_program_name_or_full_path_without_case() {
    let table = Table::new(&[
        app_rule(
            "by name",
            Protocol::Udp,
            &[],
            &["deadbydaylight-win64-shipping.exe"],
        ),
        app_rule(
            "by path",
            Protocol::Tcp,
            &["443"],
            &[r"C:\Games\Discord.exe"],
        ),
    ]);
    let dst = "52.1.1.1:7777".parse().unwrap();
    let https = "1.1.1.1:443".parse().unwrap();
    assert_eq!(
        table.matches_flow(Protocol::Udp, dst, Some(DBD)),
        Some("by name")
    );
    assert_eq!(
        table.matches_flow(Protocol::Tcp, dst, Some(DBD)),
        None,
        "the rule is UDP only"
    );
    assert_eq!(
        table.matches_flow(Protocol::Udp, dst, None),
        None,
        "no owner, no program"
    );
    assert_eq!(
        table.matches_flow(Protocol::Tcp, https, Some(r"c:\games\DISCORD.exe")),
        Some("by path")
    );
    assert_eq!(
        table.matches_flow(Protocol::Tcp, https, Some(r"D:\Discord.exe")),
        None,
        "a full path is the whole path"
    );
    assert_eq!(
        table.matches_flow(
            Protocol::Tcp,
            "1.1.1.1:80".parse().unwrap(),
            Some(r"C:\Games\Discord.exe")
        ),
        None,
        "ports still count"
    );
    assert_eq!(
        table.matches(Protocol::Udp, dst),
        None,
        "address matching ignores programs"
    );
    assert!(table.wants_owner(Protocol::Tcp, 443));
    assert!(
        !table.wants_owner(Protocol::Udp, 7777)
            && table
                .clone()
                .with_running(&[DBD])
                .wants_owner(Protocol::Udp, 7777),
        "UDP owners only while the program runs"
    );
    assert!(
        !table.wants_owner(Protocol::Tcp, 80)
            && !Table::new(&[voice()]).wants_owner(Protocol::Udp, 50000)
    );
}

#[test]
fn a_udp_app_rule_runs_while_its_program_runs() {
    let table = Table::new(&[rules::preset(Preset::Dbd)]);
    assert!(table.idle_apps() && table.starts(DBD) && !table.starts(r"C:\x\chrome.exe"));
    let running = table.with_running(&[r"C:\x\chrome.exe", DBD]);
    assert!(running.rules[0].running && !running.idle_apps() && !running.starts(DBD));
    let tcp = Table::new(&[app_rule("t", Protocol::Tcp, &[], &["x.exe"])]);
    assert!(
        !tcp.idle_apps() && !tcp.with_running(&[r"C:\x.exe"]).rules[0].running,
        "TCP needs only SYNs"
    );
}

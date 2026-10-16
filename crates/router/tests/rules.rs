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
fn fields_of_later_stages_are_named() {
    for (field, value) in [
        ("domains", "[\"discord.media\"]"),
        ("apps", "[\"Discord.exe\"]"),
    ] {
        let text = format!(
            "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n{field} = {value}\n"
        );
        let err = rules::parse(&text).unwrap_err();
        assert_eq!(err.line, Some(5));
        assert!(
            err.to_string()
                .contains(&format!("`{field}` is not supported yet")),
            "{err}"
        );
    }
    let tcp = "[[rule]]\nname = \"a\"\nprotocol = \"tcp\"\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(tcp).unwrap_err();
    assert_eq!(
        (err.line, err.field.as_deref()),
        (Some(3), Some("protocol"))
    );
    assert!(
        rules::parse("[[rule]]\nname = \"a\"\ncolour = 1\n")
            .unwrap_err()
            .to_string()
            .contains("colour")
    );
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
fn import_takes_ip_lines_once_and_refuses_the_rest() {
    let have = vec!["104.29.0.0/16".to_string()];
    let text = "\u{feff}# voice\r\n104.29.0.0/16\r\n\r\n35.217.0.0/16\n 35.217.0.0/16 \n";
    assert_eq!(
        rules::import(&have, text).unwrap(),
        ["104.29.0.0/16", "35.217.0.0/16"]
    );
    let err = rules::import(&have, "1.1.1.1\ndiscord.media\n").unwrap_err();
    assert_eq!(err.line, Some(2));
    assert!(err.to_string().contains("discord.media"), "{err}");
}

#[test]
fn edits_go_by_name_without_case() {
    let mut list = vec![voice()];
    assert!(
        rules::add(
            &mut list,
            Rule {
                name: "discord ГОЛОС".into(),
                ..voice()
            }
        )
        .is_err()
    );
    rules::set_enabled(&mut list, "DISCORD голос", false).unwrap();
    assert!(!list[0].enabled);
    assert!(
        rules::replace(
            &mut list,
            Rule {
                name: "other".into(),
                ..voice()
            }
        )
        .is_err()
    );
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

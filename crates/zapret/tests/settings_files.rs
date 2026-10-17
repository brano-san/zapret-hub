mod common;

use std::fs;

use zapret::batparse::GameFilterVars;
use zapret::fakes::FakeKind;
use zapret::game_filter::{GameFilter, GameFilterMode, parse, validate_range};
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

#[test]
fn game_filter_reads_the_service_bat_format() {
    let gf = parse("mode=all\r\ntcp=1024-65535\r\nudp=50000-50100,1024-2000\r\n");
    assert_eq!(gf.mode, GameFilterMode::All);
    assert_eq!(gf.udp, "50000-50100,1024-2000");
    assert_eq!(gf.vars().tcp, "1024-65535");
}

#[test]
fn game_filter_reads_legacy_single_word_files() {
    assert_eq!(parse("udp\r\n").mode, GameFilterMode::Udp);
    assert_eq!(parse("all").mode, GameFilterMode::All);
    assert_eq!(parse("tcp=80\r\n").mode, GameFilterMode::Disabled);
    assert_eq!(
        parse("mode=all \r\n").mode,
        GameFilterMode::Disabled,
        "service.bat compares the value as is"
    );
}

#[test]
fn bad_ranges_fall_back_to_the_default() {
    assert_eq!(parse("mode=tcp\ntcp=0-10\n").tcp, "1024-65535");
    assert_eq!(
        validate_range(" 80 , 443-444").as_deref(),
        Some("80,443-444")
    );
    for bad in ["", "70000", "10-5", "123456", "1-2-3", "a", "80,,81", "080"] {
        assert_eq!(validate_range(bad), None, "{bad}");
    }
}

#[test]
fn game_filter_vars_follow_the_mode() {
    let udp = GameFilter {
        mode: GameFilterMode::Udp,
        tcp: "1000-2000".into(),
        udp: "3000".into(),
    };
    assert_eq!(
        udp.vars(),
        GameFilterVars {
            all: "3000".into(),
            tcp: "12".into(),
            udp: "3000".into()
        }
    );
    assert_eq!(GameFilter::default().vars(), GameFilterVars::disabled());
}

#[test]
fn game_filter_write_then_read() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let gf = GameFilter {
        mode: GameFilterMode::Tcp,
        tcp: "1024-2000".into(),
        udp: "1024-65535".into(),
    };
    zapret::game_filter::write(&folder, &gf).unwrap();
    assert_eq!(zapret::game_filter::read(&folder), gf);
    assert_eq!(
        fs::read_to_string(dir.path().join("utils").join("game_filter.enabled")).unwrap(),
        "mode=tcp\r\ntcp=1024-2000\r\nudp=1024-65535\r\n"
    );
    let bad = GameFilter {
        mode: GameFilterMode::All,
        tcp: "x".into(),
        udp: "1".into(),
    };
    assert!(zapret::game_filter::write(&folder, &bad).is_err());
}

#[test]
fn parse_file_uses_the_current_game_filter() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let bat = dir.path().join("general.bat");
    fs::write(
        &bat,
        "start \"z\" /min \"%BIN%winws.exe\" --wf-tcp=80,%GameFilterTCP%\r\n",
    )
    .unwrap();
    assert_eq!(
        zapret::batparse::parse_file(&folder, &bat)
            .unwrap()
            .final_args,
        "  --wf-tcp 80,12"
    );
    fs::write(
        dir.path().join("utils").join("game_filter.enabled"),
        "mode=tcp\r\ntcp=2000\r\n",
    )
    .unwrap();
    assert_eq!(
        zapret::batparse::parse_file(&folder, &bat)
            .unwrap()
            .final_args,
        "  --wf-tcp 80,2000"
    );
}

#[test]
fn ipset_modes_cycle_like_service_bat() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Loaded);

    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "203.0.113.113/32\r\n");
    assert!(zapret::ipset::has_backup(&folder));

    zapret::ipset::set_mode(&folder, IpsetMode::Any).unwrap();
    assert_eq!(fs::read(&list).unwrap().len(), 0);

    zapret::ipset::set_mode(&folder, IpsetMode::Loaded).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    assert!(!zapret::ipset::has_backup(&folder));
}

#[test]
fn ipset_loaded_needs_a_backup() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    assert_eq!(
        zapret::ipset::mode(&folder),
        IpsetMode::Any,
        "a missing list counts as any"
    );
    let err = zapret::ipset::set_mode(&folder, IpsetMode::Loaded)
        .unwrap_err()
        .to_string();
    assert!(err.contains("backup"), "{err}");
    assert_eq!(zapret::ipset::mode_of("\u{feff}\r\n"), IpsetMode::Any);
}

#[test]
fn ipset_test_switch_puts_the_list_back() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    zapret::ipset::enter_test_any(&folder).unwrap();
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Any);
    zapret::ipset::leave_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    zapret::ipset::leave_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
}

#[test]
fn ipset_test_switch_entered_twice_keeps_the_real_list() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let lists = dir.path().join("lists");
    let list = lists.join("ipset-all.txt");
    let aside = lists.join("ipset-all.hub-test-backup.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    zapret::ipset::enter_test_any(&folder).unwrap();
    // The first DPI run was interrupted: the next one starts with the emptied list on disk.
    zapret::ipset::enter_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&aside).unwrap(), "1.1.1.0/24\r\n");
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Any);
    zapret::ipset::leave_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    assert!(!aside.exists());
}

#[test]
fn an_ipset_list_that_cannot_be_read_is_not_switched() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    let lock = common::lock_reads(&list);
    let err = zapret::ipset::try_mode(&folder).unwrap_err();
    assert!(format!("{err:#}").contains("ipset-all.txt"), "{err:#}");
    let switched = zapret::ipset::set_mode(&folder, IpsetMode::None);
    drop(lock);
    assert!(
        switched.is_err(),
        "a list it could not read was overwritten"
    );
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    assert!(!zapret::ipset::has_backup(&folder));
}

#[test]
fn a_game_filter_that_cannot_be_read_is_an_error() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let file = dir.path().join("utils").join("game_filter.enabled");
    fs::write(&file, "mode=tcp\r\ntcp=2000\r\n").unwrap();
    let bat = dir.path().join("general.bat");
    fs::write(
        &bat,
        "start \"z\" /min \"%BIN%winws.exe\" --wf-tcp=80,%GameFilterTCP%\r\n",
    )
    .unwrap();
    let _lock = common::lock_reads(&file);
    let err = zapret::game_filter::try_read(&folder).unwrap_err();
    assert!(
        format!("{err:#}").contains("game_filter.enabled"),
        "{err:#}"
    );
    let parsed = zapret::batparse::parse_file(&folder, &bat);
    assert!(
        parsed.is_err(),
        "parsed with the game filter off: {parsed:?}"
    );
}

#[test]
fn ipset_install_keeps_the_previous_list() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let lists = dir.path().join("lists");
    fs::write(lists.join("ipset-all.txt"), "old\r\n").unwrap();
    fs::write(lists.join("ipset-all.txt.download"), "new\r\n").unwrap();
    zapret::ipset::install(&folder, &lists.join("ipset-all.txt.download")).unwrap();
    assert_eq!(
        fs::read_to_string(lists.join("ipset-all.txt")).unwrap(),
        "new\r\n"
    );
    assert_eq!(
        fs::read_to_string(lists.join("ipset-all.txt.hub-prev")).unwrap(),
        "old\r\n"
    );
}

#[test]
fn user_lists_start_with_service_bat_defaults() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    let general = zapret::user_lists::read(&folder, UserList::General).unwrap();
    assert_eq!(
        general[..2],
        ["# Never leave this file empty", "domain.example.abc"]
    );
    let hub: Vec<&str> = zapret::user_lists::HUB_GENERAL.lines().collect();
    assert_eq!(general[2..], hub[..]);
    for domain in &hub {
        zapret::user_lists::normalize(UserList::General, domain).unwrap();
    }
    // A removed hub domain comes back, nothing is doubled.
    zapret::user_lists::remove(&folder, UserList::General, "x.com").unwrap();
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    let again = zapret::user_lists::read(&folder, UserList::General).unwrap();
    assert_eq!(again.len(), general.len());
    assert!(again.iter().any(|l| l == "x.com"));
    assert_eq!(
        zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(),
        ["203.0.113.113/32"]
    );
}

#[test]
fn user_list_entries_are_validated_and_deduplicated() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    zapret::user_lists::add(&folder, UserList::General, " Discord.COM ").unwrap();
    zapret::user_lists::add(&folder, UserList::General, "discord.com").unwrap();
    let general = zapret::user_lists::read(&folder, UserList::General).unwrap();
    assert_eq!(general.iter().filter(|l| *l == "discord.com").count(), 1);
    assert!(zapret::user_lists::add(&folder, UserList::General, "not a domain").is_err());
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.0/8").unwrap();
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "::1").unwrap();
    assert!(zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.0/33").is_err());
}

#[test]
fn removing_the_last_entry_puts_the_placeholder_back() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.1").unwrap();
    zapret::user_lists::remove(&folder, UserList::IpsetExclude, "203.0.113.113/32").unwrap();
    assert_eq!(
        zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(),
        ["10.0.0.1"]
    );
    zapret::user_lists::remove(&folder, UserList::IpsetExclude, "10.0.0.1").unwrap();
    assert_eq!(
        zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(),
        ["203.0.113.113/32"]
    );
}

#[test]
fn finds_and_replaces_the_active_fake() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let bin = dir.path().join("bin");
    fs::write(bin.join("stun.bin"), "a").unwrap();
    fs::write(bin.join("quic.bin"), "b").unwrap();
    fs::write(bin.join("ACTIVE_DISCORD_UDP.bin"), "a").unwrap();
    let state = zapret::fakes::state(&folder).unwrap();
    assert_eq!(state.files, ["quic", "stun"]);
    assert_eq!(state.discord.as_deref(), Some("stun"));
    assert_eq!(state.game, None);

    zapret::fakes::replace(&folder, FakeKind::Discord, "quic").unwrap();
    assert_eq!(
        zapret::fakes::state(&folder).unwrap().discord.as_deref(),
        Some("quic")
    );
    assert!(zapret::fakes::replace(&folder, FakeKind::Game, "..\\service").is_err());
}

#[test]
fn a_read_only_fake_leaves_the_active_one_writable() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let bin = dir.path().join("bin");
    let (quic, active) = (bin.join("quic.bin"), bin.join("ACTIVE_DISCORD_UDP.bin"));
    fs::write(bin.join("stun.bin"), "a").unwrap();
    fs::write(&quic, "b").unwrap();
    fs::write(&active, "a").unwrap();
    let mut read_only = fs::metadata(&quic).unwrap().permissions();
    read_only.set_readonly(true);
    fs::set_permissions(&quic, read_only).unwrap();

    zapret::fakes::replace(&folder, FakeKind::Discord, "quic").unwrap();
    assert!(!fs::metadata(&active).unwrap().permissions().readonly());
    zapret::fakes::replace(&folder, FakeKind::Discord, "stun").unwrap();
    assert_eq!(fs::read(&active).unwrap(), b"a");
}

#[test]
fn check_updates_flag_toggles() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    assert!(!zapret::flags::check_updates(&folder));
    zapret::flags::set_check_updates(&folder, true).unwrap();
    assert!(zapret::flags::check_updates(&folder));
    zapret::flags::set_check_updates(&folder, false).unwrap();
    zapret::flags::set_check_updates(&folder, false).unwrap();
    assert!(!zapret::flags::check_updates(&folder));
}

use router::rules::{self, Preset};
use router::{Router, RouterState, Setup, parse_proxy};

fn router(dir: &std::path::Path) -> Router {
    Router::new(dir.join("routes.toml"))
}

#[test]
fn a_new_router_is_off_and_pause_stop_resume_are_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.pause();
    r.resume();
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
    assert!(r.flows(true).is_empty());
}

#[test]
fn a_missing_dll_makes_it_unavailable_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    let setup = Setup {
        dll: dir.path().join("WinDivert.dll"),
        proxy: "127.0.0.1:2080".parse().unwrap(),
    };
    assert!(r.start(setup).is_err());
    match r.status().state {
        RouterState::Unavailable { reason } => {
            assert!(reason.contains("WinDivert.dll"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    r.fail("set the SOCKS5 proxy in Settings first".into());
    assert!(
        matches!(r.status().state, RouterState::Unavailable { reason } if reason.contains("SOCKS5"))
    );
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
}

#[test]
fn an_edit_writes_the_file_and_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::DiscordVoice)))
        .unwrap();
    assert_eq!(r.rules().rules[0].rule.name, "Discord голос");
    assert_eq!(
        rules::load(&dir.path().join("routes.toml")).unwrap().len(),
        1
    );
    let bad = r.edit(|list| {
        list[0].ports = vec!["0".into()];
        Ok(())
    });
    assert!(bad.is_err());
    assert_eq!(
        rules::load(&dir.path().join("routes.toml")).unwrap()[0]
            .ports
            .len(),
        2,
        "a failed edit writes nothing"
    );
}

#[test]
fn rules_are_read_again_when_the_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    let r = router(dir.path());
    assert!(r.rules().rules.is_empty());
    rules::save(&path, &[rules::preset(Preset::DiscordVoice)]).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert_eq!(r.rules().rules.len(), 1);
}

#[test]
fn a_broken_file_keeps_the_rules_and_shows_where() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    rules::save(&path, &[rules::preset(Preset::DiscordVoice)]).unwrap();
    let r = router(dir.path());
    std::fs::write(
        &path,
        "[[rule]]\nname = \"x\"\nprotocol = \"udp\"\nips = [\"300.0.0.1\"]\n",
    )
    .unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let view = r.rules();
    assert_eq!(view.rules[0].rule.name, "Discord голос");
    assert!(
        view.error.as_deref().unwrap().starts_with("line 4: ips:"),
        "{view:?}"
    );
    assert_eq!(r.status().rules_error, view.error);
}

#[test]
fn an_edit_never_overwrites_a_broken_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    std::fs::write(&path, "[[rule]\n").unwrap();
    let r = router(dir.path());
    let err = r
        .edit(|list| rules::add(list, rules::preset(Preset::DiscordVoice)))
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("routes.toml has an error"),
        "{err:#}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[[rule]\n");
}

#[test]
fn the_proxy_is_an_ipv4_address_from_the_settings() {
    assert_eq!(
        parse_proxy(Some("127.0.0.1:2080")).unwrap(),
        "127.0.0.1:2080".parse().unwrap()
    );
    assert!(format!("{:#}", parse_proxy(None).unwrap_err()).contains("SOCKS5"));
    assert!(format!("{:#}", parse_proxy(Some(" ")).unwrap_err()).contains("SOCKS5"));
    assert!(format!("{:#}", parse_proxy(Some("[::1]:2080")).unwrap_err()).contains("IPv4"));
    let lan = format!("{:#}", parse_proxy(Some("192.168.1.2:2080")).unwrap_err());
    assert!(
        lan.contains("local") && lan.contains("192.168.1.2:2080"),
        "{lan}"
    );
    assert!(parse_proxy(Some("127.0.0.2:2080")).is_ok());
}

#[test]
fn a_start_during_a_pause_waits_for_resume() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    let setup = Setup {
        dll: dir.path().join("WinDivert.dll"),
        proxy: "127.0.0.1:2080".parse().unwrap(),
    };
    r.pause();
    r.stop();
    r.start(setup).unwrap();
    assert_eq!(
        r.status().state,
        RouterState::Paused,
        "nothing opens until resume"
    );
    r.resume();
    assert!(
        matches!(r.status().state, RouterState::Unavailable { reason } if reason.contains("WinDivert.dll"))
    );
}

#[test]
fn on_change_fires_when_the_state_changes() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let c = calls.clone();
    r.on_change(move || {
        c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    r.stop();
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "Off to Off is no change"
    );
    r.fail("no proxy".into());
    r.fail("no proxy".into());
    r.stop();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_failed_edit_still_applies_an_outside_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    let r = router(dir.path());
    assert!(r.table().is_empty());
    rules::save(&path, &[rules::preset(Preset::DiscordVoice)]).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let bad = r.edit(|list| {
        list[0].ports = vec!["0".into()];
        Ok(())
    });
    assert!(bad.is_err());
    let voice = "104.29.1.1:50000".parse().unwrap();
    assert_eq!(
        r.table().match_udp(voice),
        Some("Discord голос"),
        "the outside change is in force"
    );
    assert_eq!(r.rules().rules.len(), 1);
}

#[test]
fn a_domain_rule_is_saved_and_its_names_are_wanted() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::Discord)))
        .unwrap();
    assert!(r.table().wants("gateway.discord.gg"));
    assert_eq!(
        rules::load(&dir.path().join("routes.toml")).unwrap()[0]
            .domains
            .len(),
        5
    );
}

#[test]
fn an_app_rule_is_saved_with_its_programs() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::Dbd)))
        .unwrap();
    assert!(r.table().wants_owner(rules::Protocol::Tcp, 7777));
    assert_eq!(
        rules::load(&dir.path().join("routes.toml")).unwrap()[0].apps,
        ["DeadByDaylight-Win64-Shipping.exe"]
    );
}

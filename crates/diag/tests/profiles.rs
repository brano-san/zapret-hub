use diag::profiles::{ProbeKind, any_program, builtin, load};

#[test]
fn builtin_has_three_profiles() {
    let p = builtin();
    let ids: Vec<_> = p.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["discord", "dbd", "youtube"]);
    assert_eq!(p[0].probes.len(), 3);
    assert_eq!(p[0].probes[0].kind, ProbeKind::Https);
    assert_eq!(p[0].probes[0].target, "discord.com");
}

#[test]
fn override_replaces_by_id_and_adds_new() {
    let dir = tempfile::tempdir().unwrap();
    let mut dbd = builtin().remove(1);
    dbd.hint = "X".into();
    std::fs::write(dir.path().join("dbd.toml"), toml::to_string(&dbd).unwrap()).unwrap();
    let mine = diag::profiles::Profile {
        id: "mine".into(),
        ..dbd
    };
    std::fs::write(
        dir.path().join("mine.toml"),
        toml::to_string(&mine).unwrap(),
    )
    .unwrap();
    let all = load(dir.path()).unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[1].hint, "X");
    assert_eq!(all.last().unwrap().id, "mine");
}

#[test]
fn broken_override_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.toml"), "id = [").unwrap();
    let e = load(dir.path()).unwrap_err();
    assert!(format!("{e:#}").contains("bad.toml"));
}

#[test]
fn any_program_probes_observed() {
    let p = any_program("game.exe");
    let k: Vec<_> = p
        .probes
        .iter()
        .map(|s| (s.kind, s.target.as_str()))
        .collect();
    assert_eq!(
        k,
        [
            (ProbeKind::Https, "observed"),
            (ProbeKind::Live, "observed")
        ]
    );
}

#[test]
fn each_program_gets_its_own_rule() {
    use router::rules::{Protocol, Rule, check};
    assert_eq!(builtin()[0].rule_name(), "diag-discord");
    assert_eq!(any_program("game1.exe").rule_name(), "diag-game1");
    let odd = any_program("Game (x64) — Ünïcode build of a very long program name.exe").rule_name();
    assert_eq!(odd.chars().count(), 40, "{odd}");
    assert!(odd.starts_with("diag-Game -x64- - "), "{odd}");
    let rule = Rule {
        name: odd,
        enabled: true,
        protocol: Protocol::Tcp,
        ports: vec!["443".into()],
        ips: vec!["1.2.3.0/24".into()],
        domains: vec![],
        apps: vec![],
    };
    assert!(check(&rule).is_empty(), "{:?}", check(&rule));
}

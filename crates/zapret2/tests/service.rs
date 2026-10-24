mod common;

use zapret::service::{SERVICE2, active, strategy};
use zapret2::Engine;
use zapret2::service::{bin_path, dry_run_args, intercept_args, save, validate};
use zapret2::strategies;

#[cfg(windows)]
#[test]
fn winws2_gets_the_strategy_first_and_its_own_flags_last() {
    let tail = r#"--chdir="D:\zapret2" --wf-tcp-out=443 --lua-desync=fake:blob=fake_default_tls"#;
    assert_eq!(
        dry_run_args(tail),
        format!("{tail} --wf-dup-check=0 --dry-run")
    );
    assert_eq!(intercept_args(tail), format!("{tail} --intercept=0"));
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "bin", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    assert_eq!(
        bin_path(&engine, tail),
        format!("\"{}\\bin\\winws2.exe\" {tail}", dir.path().display())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn nfqws2_gets_the_strategy_first_and_its_own_flags_last() {
    let tail = r#"--chdir="/opt/zapret2" --lua-desync=fake:blob=fake_default_tls --qnum=200"#;
    assert_eq!(dry_run_args(tail), format!("{tail} --dry-run"));
    assert_eq!(intercept_args(tail), format!("{tail} --intercept=0"));
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "bin", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    // Every word as `run` splits it, quoted for systemd.
    assert_eq!(
        bin_path(&engine, tail),
        format!(
            r#""{}/bin/nfqws2" "--chdir=/opt/zapret2" "--lua-desync=fake:blob=fake_default_tls" "--qnum=200""#,
            dir.path().display()
        )
    );
}

/// What an install writes, from the strategy's line: the unit's `ExecStart` and the table.
#[cfg(target_os = "linux")]
#[test]
fn an_install_writes_the_checked_argv_and_the_strategys_ports() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let engine = Engine::open(dir.path())
        .unwrap()
        .with_excluded_ports(&["51413".to_owned()])
        .unwrap();
    let tail = r#"--chdir="/my z2" --wf-tcp-out=80,443 --wf-udp-out=443,50000-65535,1024-65535 --qnum=7 --lua-init="a='$x'""#;
    let (exec, table) = zapret2::service::service_plan(&engine, tail).unwrap();
    assert_eq!(
        exec,
        format!(
            r#""{}/nfqws2" "--chdir=/my z2" "--lua-init=a='$$x'" "--qnum=200""#,
            dir.path().display()
        )
    );
    let unit = zapret::service::unit(&exec, None).unwrap();
    assert!(unit.contains(&format!("ExecStart={exec}\n")), "{unit}");
    for part in [
        "tcp dport { 80,443 } ct original packets 1-12 queue num 200 bypass",
        "udp dport { 443,1024-65535 } ct original packets 1-8 queue num 200 bypass",
        "th dport { 51413 } return",
    ] {
        assert!(table.contains(part), "{part}: {table}");
    }
}

#[test]
fn the_static_check_comes_before_winws2_runs() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    // winws2.exe is an empty file here: had it run, the error would be about starting it.
    let err = format!(
        "{:#}",
        validate(&engine, "--lua-desync=nosuch").unwrap_err()
    );
    assert!(err.contains("nosuch"), "{err}");
}

#[test]
fn a_save_writes_nothing_until_the_check_passes() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(
        dir.path(),
        "",
        "NFQWS2_COMPAT_VER_REQUIRED=6\nfunction fake(ctx, desync)\nend\n",
    );
    let engine = Engine::open(dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let file = strategies::dir(data.path()).join("mine.txt");
    let text = "--wf-tcp-out=443\n--lua-desync=fake:blob=fake_default_tls\n";
    // The static check passes; winws2 (an empty file) cannot run, so nothing is saved.
    let err = format!(
        "{:#}",
        save(&engine, data.path(), "mine", text, true).unwrap_err()
    );
    assert!(err.contains(common::CANNOT_RUN), "{err}");
    assert!(!file.exists());
    let err = format!(
        "{:#}",
        save(&engine, data.path(), "mine", text, false).unwrap_err()
    );
    assert!(err.contains("no zapret2 strategy of yours"), "{err}");
    assert!(!file.exists());
}

#[test]
fn a_bad_name_is_refused_before_any_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    for (name, refusal) in [
        ("..\\evil", "a name is 1-40"),
        ("a/b", "a name is 1-40"),
        ("general", "is built in"),
        ("con", "Windows device name"),
    ] {
        let err = format!(
            "{:#}",
            save(&engine, data.path(), name, "--wf-tcp-out=443", true).unwrap_err()
        );
        assert!(
            err.contains(refusal) && !err.contains(common::CANNOT_RUN),
            "{name}: {err}"
        );
    }
    assert!(!strategies::dir(data.path()).exists());
}

fn live() -> (Engine, tempfile::TempDir) {
    let dir = std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR is the zapret2 folder");
    (Engine::open(dir).unwrap(), tempfile::tempdir().unwrap())
}

#[test]
#[ignore = "needs admin rights and a zapret2 release in ZAPRET2_DIR; loads WinDivert"]
fn live_every_built_in_strategy_passes_the_whole_check() {
    let (engine, data) = live();
    for s in strategies::list(data.path()).unwrap() {
        let tail = strategies::tail(&engine, &s.text).unwrap();
        validate(&engine, &tail).unwrap_or_else(|err| panic!("{}: {err:#}", s.name));
    }
}

#[test]
#[ignore = "needs admin rights and a zapret2 release in ZAPRET2_DIR; replaces the zapret2 service"]
fn live_apply_zapret2() {
    let (engine, data) = live();
    let check = zapret2::service::apply(&engine, data.path(), "general").unwrap();
    eprintln!("zapret2: {check:?}");
    assert_eq!(active(), (SERVICE2, zapret::service::WINWS2));
    assert_eq!(strategy().as_deref(), Some("zapret2:general"));
}

#[test]
fn the_check_before_a_snapshot_changes_nothing_when_it_fails() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", "NFQWS2_COMPAT_VER_REQUIRED=6\n");
    let engine = Engine::open(dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let text = "--lua-desync=nosuch\n";
    let err = format!(
        "{:#}",
        zapret2::service::check_save(&engine, data.path(), "mine", text, true).unwrap_err()
    );
    assert!(err.contains("nosuch"), "{err}");
    assert!(!strategies::dir(data.path()).join("mine.txt").exists());
}

#[test]
fn the_prefix_is_matched_in_any_case() {
    assert_eq!(zapret2::strip_prefix("zapret2:General"), Some("General"));
    assert_eq!(zapret2::strip_prefix("Zapret2:a"), Some("a"));
    assert_eq!(zapret2::strip_prefix("general"), None);
    assert_eq!(zapret2::strip_prefix("zapret"), None);
    assert_eq!(zapret2::strip_prefix("ёzapret2:a"), None);
}

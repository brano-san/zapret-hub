mod common;

use std::fs;

use zapret::service::{SERVICE, SERVICE2, active, state, strategy};
use zapret2::Engine;
use zapret2::service::{bin_path, dry_run_args, intercept_args, save, validate};
use zapret2::strategies;

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
        save(&engine, None, data.path(), "mine", text, true).unwrap_err()
    );
    assert!(err.contains("os error 193"), "{err}");
    assert!(!file.exists());
    let err = format!(
        "{:#}",
        save(
            &engine,
            None,
            data.path(),
            "mine",
            "--hostlist=\"%LISTS%a.txt\"",
            true
        )
        .unwrap_err()
    );
    assert!(err.contains("%LISTS%"), "{err}");
    let err = format!(
        "{:#}",
        save(&engine, None, data.path(), "mine", text, false).unwrap_err()
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
            save(&engine, None, data.path(), name, "--wf-tcp-out=443", true).unwrap_err()
        );
        assert!(
            err.contains(refusal) && !err.contains("os error 193"),
            "{name}: {err}"
        );
    }
    assert!(!strategies::dir(data.path()).exists());
}

fn live() -> (Engine, zapret::folder::Folder, tempfile::TempDir) {
    let dir = std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR is the zapret2 folder");
    let engine = Engine::open(dir).unwrap();
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    (engine, folder, tempfile::tempdir().unwrap())
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; loads WinDivert"]
fn live_every_built_in_strategy_passes_the_whole_check() {
    let (engine, folder, data) = live();
    for s in strategies::list(data.path()).unwrap() {
        let tail = strategies::tail(&engine, Some(&folder), &s.text).unwrap();
        validate(&engine, &tail).unwrap_or_else(|err| panic!("{}: {err:#}", s.name));
    }
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; replaces the zapret and zapret2 services"]
fn live_apply_zapret2_then_flowseal_back() {
    let (engine, folder, data) = live();
    let check = zapret2::service::apply(&engine, Some(&folder), data.path(), "general").unwrap();
    eprintln!("zapret2: {check:?}");
    assert_eq!(active(), (SERVICE2, zapret::service::WINWS2));
    assert_eq!(strategy().as_deref(), Some("zapret2:general"));
    assert_eq!(state(SERVICE), zapret::service::SvcState::NotInstalled);
    let check = zapret::service::apply(&folder, "general").unwrap();
    eprintln!("flowseal: {check:?}");
    assert_eq!(active(), (SERVICE, "winws.exe"));
    assert_eq!(state(SERVICE2), zapret::service::SvcState::NotInstalled);
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; reinstalls zapret2"]
fn live_a_game_filter_change_reinstalls_zapret2() {
    let (engine, folder, data) = live();
    strategies::write(
        data.path(),
        "game",
        &format!(
            "{}\n--new --filter-tcp=%GameFilterTCP% --lua-desync=fake:blob=fake_default_tls",
            strategies::find(data.path(), "general").unwrap().text
        ),
    )
    .unwrap();
    zapret2::service::apply(&engine, Some(&folder), data.path(), "game").unwrap();
    zapret2::service::reinstall_active(Some(&engine), &folder, data.path()).unwrap();
    let image = zapret::service::config(SERVICE2).unwrap().image_path;
    let filter = zapret::game_filter::read(&folder).vars();
    assert!(
        image.contains(&format!("--filter-tcp={}", filter.tcp)),
        "{image}"
    );
    fs::remove_dir_all(strategies::dir(data.path())).unwrap();
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
        zapret2::service::check_save(&engine, None, data.path(), "mine", text, true).unwrap_err()
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

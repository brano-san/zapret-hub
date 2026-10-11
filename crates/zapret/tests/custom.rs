mod common;

use std::path::Path;

use zapret::batparse::{GameFilterVars, parse};
use zapret::custom::{CustomStrategy, Store, split_bat, validate_name};

fn sample() -> CustomStrategy {
    CustomStrategy {
        name: "Test 1".into(),
        cloned_from: "general (ALT11)".into(),
        wf: "--wf-tcp=80,443".into(),
        blocks: vec![
            "--filter-tcp=80 --dpi-desync=fake".into(),
            "--filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\"".into(),
        ],
    }
}

#[test]
fn every_flowseal_strategy_survives_split_and_regenerate() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let root = std::fs::read_to_string(dir.join("ROOT.txt"))
        .expect("run tools\\gen-bat-fixtures.cmd first");
    let root = root.trim_end();
    let vars = GameFilterVars {
        all: "1024-65535".into(),
        tcp: "1024-65535".into(),
        udp: "1024-65535".into(),
    };
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bat"))
        {
            continue;
        }
        let bat = zapret::read_text(&path).unwrap();
        let Ok(original) = parse(&bat, root, &vars) else {
            continue;
        };
        let (wf, blocks) = split_bat(&bat).unwrap();
        let regenerated = CustomStrategy {
            name: "x".into(),
            cloned_from: "y".into(),
            wf,
            blocks,
        };
        assert_eq!(
            parse(&regenerated.to_bat(), root, &vars).unwrap(),
            original,
            "{}",
            path.display()
        );
        compared += 1;
    }
    assert!(compared >= 20, "only {compared} strategies compared");
}

#[test]
fn the_generated_file_looks_like_flowseal() {
    assert_eq!(sample().file_stem(), "general (HUB Test 1)");
    let bat = sample().to_bat();
    assert!(bat.starts_with("@echo off\r\nchcp 65001 > nul\r\n"));
    assert!(bat.ends_with(concat!(
        "cd /d %BIN%\r\n\r\n",
        "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443 ^\r\n",
        "--filter-tcp=80 --dpi-desync=fake --new ^\r\n",
        "--filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\"\r\n",
    )));
}

#[test]
fn editor_text_becomes_one_line_and_is_checked() {
    let s = CustomStrategy {
        blocks: vec![
            "--filter-tcp=80\r\n  --dpi-desync=fake".into(),
            "  \n".into(),
        ],
        ..sample()
    };
    assert_eq!(
        s.normalized().unwrap().blocks,
        ["--filter-tcp=80 --dpi-desync=fake"]
    );
    for (wf, block) in [
        ("--wf-tcp=80", "--filter-tcp=80 & calc"),
        ("--wf-tcp=80", "--filter-tcp=80 --new --filter-tcp=443"),
        ("--filter-tcp=80", "--dpi-desync=fake"),
        ("--wf-tcp=80", ""),
    ] {
        let s = CustomStrategy {
            wf: wf.into(),
            blocks: vec![block.into()],
            ..sample()
        };
        assert!(s.normalized().is_err(), "{wf} / {block}");
    }
}

#[test]
fn names_are_checked() {
    assert_eq!(
        validate_name("  My strat-1.2_x ").unwrap(),
        "My strat-1.2_x"
    );
    let long = "x".repeat(41);
    for bad in ["", " ", "-x", "a/b", "привет", "a\"b", long.as_str()] {
        assert!(validate_name(bad).is_err(), "{bad}");
    }
    // Windows device names: `data\strategies\NUL.toml` would vanish into the device.
    for device in ["CON", "nul", "Nul.x", "CON .x", "COM1", "lpt9"] {
        assert!(validate_name(device).is_err(), "{device}");
    }
    for fine in ["CONx", "Console", "COM10"] {
        assert_eq!(validate_name(fine).unwrap(), fine);
    }
}

#[test]
fn a_record_whose_name_is_not_a_valid_name_names_its_file() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(
        data.path().join("evil.toml"),
        r#"name = 'x)\..\evil'
cloned_from = 'general'
wf = '--wf-tcp=80'
blocks = ['--filter-tcp=80']
"#,
    )
    .unwrap();
    let err = Store::new(data.path()).list().unwrap_err().to_string();
    assert!(err.contains("evil.toml"), "{err}");
}

#[test]
fn store_keeps_records_and_files_in_step() {
    let zapret_dir = common::fake_zapret();
    let folder = common::folder(&zapret_dir);
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    assert!(
        store.list().unwrap().is_empty(),
        "a missing folder is an empty store"
    );

    let s = sample();
    store.save(&s).unwrap();
    zapret::custom::write_bat(&folder, &s).unwrap();
    assert_eq!(store.list().unwrap(), std::slice::from_ref(&s));
    assert_eq!(store.get("TEST 1").unwrap(), s);

    let bat = zapret_dir.path().join("general (HUB Test 1).bat");
    std::fs::remove_file(&bat).unwrap();
    assert_eq!(
        zapret::custom::materialize_all(&folder, &store).unwrap(),
        ["Test 1"]
    );
    assert!(bat.is_file());

    assert!(
        zapret::custom::delete(&folder, &store, "Test 1", Some("GENERAL (HUB TEST 1)")).is_err()
    );
    zapret::custom::delete(&folder, &store, "Test 1", Some("general (ALT11)")).unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(!bat.exists());
}

#[test]
fn a_broken_record_names_its_file() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("bad.toml"), "name = ").unwrap();
    let err = Store::new(data.path()).list().unwrap_err().to_string();
    assert!(err.contains("bad.toml"), "{err}");
}

#[test]
fn every_flowseal_strategy_passes_the_editor_checks_unchanged() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bat"))
        {
            continue;
        }
        let Ok((wf, blocks)) = split_bat(&zapret::read_text(&path).unwrap()) else {
            continue;
        };
        let s = CustomStrategy {
            name: "x".into(),
            cloned_from: "y".into(),
            wf,
            blocks,
        };
        assert_eq!(
            s.normalized()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            s,
            "{}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} strategies checked");
    // `^!` is Flowseal's spelling of a literal `!`; any other caret stays refused.
    for block in [
        "--dpi-desync-fake-tls=^x",
        "--dpi-desync-fake-tls=^^!",
        "--dpi-desync-fake-tls=^",
    ] {
        let s = CustomStrategy {
            blocks: vec![block.into()],
            ..sample()
        };
        assert!(s.normalized().is_err(), "{block}");
    }
}

#[test]
fn nothing_is_stored_when_winws_does_not_start() {
    // The fake folder's winws.exe is an empty file, which Windows refuses to run.
    let zapret_dir = common::fake_zapret();
    let folder = common::folder(&zapret_dir);
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let err = zapret::custom::save(&folder, &store, &sample(), true).unwrap_err();
    assert!(err.downcast_ref::<std::io::Error>().is_some(), "{err:#}");
    assert!(store.list().unwrap().is_empty());
    assert!(!zapret_dir.path().join("general (HUB Test 1).bat").exists());
}

// In the next two tests the fake folder's winws.exe cannot start, so an error that is not a spawn
// error proves `save` stopped before the dry run.
#[test]
fn a_corrupt_record_is_reported_before_winws_runs() {
    let zapret_dir = common::fake_zapret();
    let folder = common::folder(&zapret_dir);
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("bad.toml"), "name = ").unwrap();
    let store = Store::new(data.path());
    for new in [true, false] {
        let err = zapret::custom::save(&folder, &store, &sample(), new).unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("bad.toml"), "new = {new}: {err}");
        assert!(
            !err.contains("no custom strategy named"),
            "new = {new}: {err}"
        );
    }
    let written: Vec<_> = std::fs::read_dir(data.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, ["bad.toml"], "no record was written");
    assert!(!zapret_dir.path().join("general (HUB Test 1).bat").exists());
}

#[test]
fn a_name_in_use_or_missing_is_reported_before_winws_runs() {
    let zapret_dir = common::fake_zapret();
    let folder = common::folder(&zapret_dir);
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let attempt = |s: &CustomStrategy, new| {
        let err = zapret::custom::save(&folder, &store, s, new).unwrap_err();
        format!("{err:#}")
    };
    assert!(attempt(&sample(), false).contains("no custom strategy named Test 1"));

    store.save(&sample()).unwrap();
    let upper = CustomStrategy {
        name: "TEST 1".into(),
        ..sample()
    };
    assert!(attempt(&upper, true).contains("the name TEST 1 is taken"));

    // A file without a record takes the name as well.
    store.remove("Test 1").unwrap();
    zapret::custom::write_bat(&folder, &sample()).unwrap();
    assert!(attempt(&sample(), true).contains("the name Test 1 is taken"));
}

#[test]
#[ignore = "needs E:\\Zapret and admin rights"]
fn live_clone_of_alt11_passes_winws_dry_run() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let source = zapret::strategies::find(&folder, "general (ALT11)").unwrap();
    let (wf, blocks) = split_bat(&zapret::read_text(&source.path).unwrap()).unwrap();
    let s = CustomStrategy {
        name: "dry run probe".into(),
        cloned_from: source.name.clone(),
        wf,
        blocks,
    };
    let (_, parsed) = zapret::custom::validate(&folder, &s).unwrap();
    assert_eq!(
        parsed,
        zapret::batparse::parse_file(&folder, &source.path).unwrap()
    );
}

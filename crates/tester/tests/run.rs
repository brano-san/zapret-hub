use std::os::windows::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tester::model::Suite;
use tester::run::{Progress, Request, lua_failure, par_map, parallelism};

#[test]
fn par_map_keeps_the_order() {
    let items: Vec<u32> = (0..50).collect();
    let doubled = par_map(&items, 8, |x| {
        std::thread::sleep(Duration::from_millis(5));
        x * 2
    });
    assert_eq!(doubled, items.iter().map(|x| x * 2).collect::<Vec<_>>());
    assert!(par_map(&Vec::<u32>::new(), 8, |x| *x).is_empty());
}

#[test]
fn parallelism_is_between_8_and_16() {
    assert!((8..=16).contains(&parallelism()));
}

#[test]
fn the_dpi_suite_falls_back_to_its_cache() {
    let dir = tempfile::tempdir().unwrap();
    let json = r#"[{ "id": "US.GH-HPRN", "provider": "Self check", "country": "🧠", "host": "hyperion-cs.github.io" }]"#;
    assert!(tester::dpi::from_fetch(dir.path(), Err(anyhow::anyhow!("offline"))).is_err());
    let fresh = tester::dpi::from_fetch(dir.path(), Ok(json.into())).unwrap();
    assert_eq!(fresh[0].host, "hyperion-cs.github.io");
    assert_eq!(
        tester::dpi::from_fetch(dir.path(), Err(anyhow::anyhow!("offline"))).unwrap(),
        fresh
    );
    assert_eq!(
        tester::dpi::from_fetch(dir.path(), Ok("<html>".into())).unwrap(),
        fresh,
        "a broken download keeps the cache"
    );
}

#[test]
fn winws2_that_died_or_printed_a_lua_error_did_not_run_the_strategy() {
    let ready = "windivert initialized. capture is started.\n";
    assert_eq!(lua_failure(None, ready), None);
    let died = lua_failure(
        Some(ExitStatus::from_raw(87)),
        "LUA ERROR: desync function 'fakee' does not exist\n",
    )
    .unwrap();
    assert!(died.contains("87") && died.contains("fakee"), "{died}");
    let erred = lua_failure(
        None,
        &format!("{ready}LUA ERROR: attempt to index a nil value\n"),
    )
    .unwrap();
    assert!(erred.contains("attempt to index"), "{erred}");
}

/// Refused before anything changes, so it runs here: the services are not touched.
#[test]
fn zapret2_strategies_need_a_working_engine_before_the_run_starts() {
    let engine_dir = tempfile::tempdir().unwrap();
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        std::fs::write(engine_dir.path().join(file), b"").unwrap();
    }
    std::fs::create_dir_all(engine_dir.path().join("lua")).unwrap();
    for file in ["zapret-lib.lua", "zapret-antidpi.lua"] {
        std::fs::write(engine_dir.path().join("lua").join(file), "").unwrap();
    }
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into(), "zapret2:general".into()],
        repeats: 1,
    };
    // `run` starts with this and stops the services only after it: asserted directly, so that the
    // test cannot touch them.
    let refusal = |engine2: Option<&zapret2::Engine>| {
        format!(
            "{:#}",
            tester::run::zapret2_version(engine2, &request).unwrap_err()
        )
    };
    assert!(
        refusal(None).contains("zapret2 folder"),
        "{}",
        refusal(None)
    );
    // An empty winws2.exe cannot tell its version.
    assert!(
        refusal(Some(&engine)).contains("os error 193"),
        "{}",
        refusal(Some(&engine))
    );
    // A Flowseal-only request needs no engine, and a mixed-case prefix is still zapret2's.
    let flowseal = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into()],
        repeats: 1,
    };
    assert_eq!(tester::run::zapret2_version(None, &flowseal).unwrap(), None);
    let upper = Request {
        suite: Suite::Standard,
        strategies: vec!["Zapret2:general".into()],
        repeats: 1,
    };
    assert!(tester::run::zapret2_version(None, &upper).is_err());
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}

#[test]
fn flowseal_strategies_need_the_flowseal_folder_before_the_run_starts() {
    let request = |names: &[&str]| Request {
        suite: Suite::Standard,
        strategies: names.iter().map(|&n| n.to_owned()).collect(),
        repeats: 1,
    };
    tester::run::needs_flowseal(None, &request(&["zapret2:mine", "ZAPRET2:general"])).unwrap();
    let err = tester::run::needs_flowseal(None, &request(&["zapret2:mine", "general"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("general is a Flowseal strategy"), "{err}");
    assert!(err.contains("zapret folder is not set"), "{err}");
}

// Past its refusals, `run` is called only by the ignored tests below: it stops the services here.
#[test]
fn a_request_without_strategies_is_refused() {
    let request = Request {
        suite: Suite::Standard,
        strategies: Vec::new(),
        repeats: 1,
    };
    assert!(
        request
            .check()
            .unwrap_err()
            .to_string()
            .contains("no strategies")
    );
}

#[test]
#[ignore = "needs admin rights, E:\\Zapret and the network; stops zapret for about a minute"]
fn live_run_of_two_strategies() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general (ALT11)".into(), "general".into()],
        repeats: 1,
    };
    let run = tester::run::run(
        Some(&folder),
        None,
        data.path(),
        &request,
        None,
        &AtomicBool::new(false),
        &|p| eprintln!("{p:?}"),
    )
    .unwrap();
    eprintln!("{}", tester::report::to_text(&run));
    assert_eq!(run.strategies.len(), 2);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
    assert!(
        tester::history::dir(data.path())
            .join(format!("{}.json", run.id))
            .is_file()
    );
}

#[test]
#[ignore = "needs admin rights and E:\\Zapret; stops zapret for a few seconds"]
fn live_a_panic_during_the_run_is_passed_on_after_the_restore() {
    use zapret::service::{SERVICE, state};
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let before = state(SERVICE);
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into()],
        repeats: 1,
    };
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tester::run::run(
            Some(&folder),
            None,
            data.path(),
            &request,
            None,
            &AtomicBool::new(false),
            &|p| {
                // Zapret is stopped by now: the restore has to bring it back.
                if matches!(p, Progress::Strategy { .. }) {
                    panic!("progress panics");
                }
            },
        )
    }));
    assert!(ran.is_err(), "the panic is passed on");
    assert_eq!(state(SERVICE), before);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}

#[test]
#[ignore = "needs admin rights, E:\\Zapret, a zapret2 release in ZAPRET2_DIR and the network"]
fn live_a_run_mixes_the_engines_and_brings_the_active_one_back() {
    use zapret::service::{active, strategy};
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let engine = zapret2::Engine::open(std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR")).unwrap();
    let data = tempfile::tempdir().unwrap();
    let before = (active(), strategy());
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into(), "zapret2:general".into()],
        repeats: 1,
    };
    let run = tester::run::run(
        Some(&folder),
        Some(&engine),
        data.path(),
        &request,
        None,
        &AtomicBool::new(false),
        &|p| eprintln!("{p:?}"),
    )
    .unwrap();
    eprintln!("{}", tester::report::to_text(&run));
    assert_eq!(run.strategies.len(), 2);
    assert!(run.zapret2_version.is_some());
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
    assert_eq!((active(), strategy()), before);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}

#[test]
#[ignore = "needs admin rights, E:\\Zapret and the network"]
fn live_cancel_stops_after_the_current_strategy() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general (ALT11)".into(), "general".into()],
        repeats: 1,
    };
    let run = tester::run::run(
        Some(&folder),
        None,
        data.path(),
        &request,
        None,
        &cancel,
        &|p| {
            if matches!(p, Progress::Done { .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert!(run.canceled);
    assert_eq!(run.strategies.len(), 1);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
}

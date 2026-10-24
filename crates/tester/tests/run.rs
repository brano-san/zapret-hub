#[cfg(unix)]
use std::os::unix::process::ExitStatusExt as _;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use probe::curl::CheckKind;
use tester::model::Suite;
use tester::run::{Progress, Request, checks_for, lua_failure, par_map, parallelism};
use zapret::targets::Target;

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
fn parallelism_is_between_16_and_32() {
    assert!((16..=32).contains(&parallelism()));
}

fn target(host: &str) -> Target {
    Target {
        name: host.into(),
        url: Some(format!("https://{host}")),
        ping: host.into(),
    }
}

#[test]
fn the_dbd_server_gets_http_and_tls12_only_and_no_ping() {
    let all = (&CheckKind::ALL[..], true);
    assert_eq!(checks_for(&target("discord.com")), all);
    assert_eq!(checks_for(&target("store.steampowered.com")), all);
    assert_eq!(
        checks_for(&target("Steam.Live.BHVRdbd.com")),
        (&[CheckKind::Http, CheckKind::Tls12][..], false)
    );
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

/// A status with exit code `code`.
fn exit(code: i32) -> ExitStatus {
    #[cfg(windows)]
    return ExitStatus::from_raw(code as u32);
    #[cfg(unix)]
    return ExitStatus::from_raw(code << 8);
}

#[test]
fn winws2_that_died_or_printed_a_lua_error_did_not_run_the_strategy() {
    let ready = "windivert initialized. capture is started.\n";
    assert_eq!(lua_failure(None, ready), None);
    let died = lua_failure(
        Some(exit(87)),
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
        zapret2::EXE,
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
    // An empty engine file cannot tell its version: not a program (Windows), not executable
    // (Linux).
    let cannot_run = if cfg!(windows) { "os error 193" } else { "os error 13" };
    assert!(
        refusal(Some(&engine)).contains(cannot_run),
        "{}",
        refusal(Some(&engine))
    );
    // A request without zapret2 names needs no engine, and a mixed-case prefix is still zapret2's.
    let other = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into()],
        repeats: 1,
    };
    assert_eq!(tester::run::zapret2_version(None, &other).unwrap(), None);
    let upper = Request {
        suite: Suite::Standard,
        strategies: vec!["Zapret2:general".into()],
        repeats: 1,
    };
    assert!(tester::run::zapret2_version(None, &upper).is_err());
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}

#[test]
fn a_name_without_the_zapret2_prefix_stops_the_run_before_it_starts() {
    let request = |names: &[&str]| Request {
        suite: Suite::Standard,
        strategies: names.iter().map(|&n| n.to_owned()).collect(),
        repeats: 1,
    };
    tester::run::zapret2_only(&request(&["zapret2:mine", "ZAPRET2:general"])).unwrap();
    let err = tester::run::zapret2_only(&request(&["zapret2:mine", "general (ALT11)"]))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("general (ALT11) is not a zapret2 strategy"),
        "{err}"
    );
    assert!(err.contains("zapret2:<name>"), "{err}");
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

fn live_engine() -> zapret2::Engine {
    zapret2::Engine::open(std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR")).unwrap()
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and the network; stops zapret2 for about a minute"]
fn live_run_of_two_strategies_brings_the_active_one_back() {
    use zapret::service::{active, strategy};
    let engine = live_engine();
    let data = tempfile::tempdir().unwrap();
    let before = (active(), strategy());
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["zapret2:general (ALT11)".into(), "zapret2:general".into()],
        repeats: 1,
    };
    let run = tester::run::run(
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
    assert!(
        tester::history::dir(data.path())
            .join(format!("{}.json", run.id))
            .is_file()
    );
}

#[test]
#[ignore = "needs admin rights and a zapret2 release in ZAPRET2_DIR; stops zapret2 for a few seconds"]
fn live_a_panic_during_the_run_is_passed_on_after_the_restore() {
    use zapret::service::{SERVICE2, state};
    let engine = live_engine();
    let data = tempfile::tempdir().unwrap();
    let before = state(SERVICE2);
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["zapret2:general".into()],
        repeats: 1,
    };
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tester::run::run(
            Some(&engine),
            data.path(),
            &request,
            None,
            &AtomicBool::new(false),
            &|p| {
                // zapret2 is stopped by now: the restore has to bring it back.
                if matches!(p, Progress::Strategy { .. }) {
                    panic!("progress panics");
                }
            },
        )
    }));
    assert!(ran.is_err(), "the panic is passed on");
    assert_eq!(state(SERVICE2), before);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and the network"]
fn live_cancel_stops_after_the_current_strategy() {
    let engine = live_engine();
    let data = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["zapret2:general (ALT11)".into(), "zapret2:general".into()],
        repeats: 1,
    };
    let run = tester::run::run(Some(&engine), data.path(), &request, None, &cancel, &|p| {
        if matches!(p, Progress::Done { .. }) {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(run.canceled);
    assert_eq!(run.strategies.len(), 1);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tester::model::Suite;
use tester::run::{Progress, Request, par_map, parallelism};

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

// `run` is called only by the ignored tests below: it stops the zapret service of this machine.
#[test]
fn a_request_without_strategies_is_refused() {
    let request = Request {
        suite: Suite::Standard,
        strategies: Vec::new(),
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
    };
    let run = tester::run::run(
        &folder,
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
#[ignore = "needs admin rights, E:\\Zapret and the network"]
fn live_cancel_stops_after_the_current_strategy() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general (ALT11)".into(), "general".into()],
    };
    let run = tester::run::run(&folder, data.path(), &request, None, &cancel, &|p| {
        if matches!(p, Progress::Done { .. }) {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(run.canceled);
    assert_eq!(run.strategies.len(), 1);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
}

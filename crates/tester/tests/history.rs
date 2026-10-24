use tester::history;
use tester::model::{Score, StrategyResult, Suite, TestRun};

fn result(name: &str, sha: &str) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: sha.into(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
        passes: Vec::new(),
    }
}

fn run(id: &str, suite: Suite, canceled: bool, strategies: Vec<StrategyResult>) -> TestRun {
    TestRun {
        id: id.into(),
        suite,
        zapret2_version: None,
        canceled,
        restore_error: None,
        strategies,
    }
}

#[test]
fn runs_are_saved_listed_and_loaded() {
    let dir = tempfile::tempdir().unwrap();
    history::save(
        dir.path(),
        &run(
            "2026-10-09_10-00-00",
            Suite::Standard,
            false,
            vec![result("general (ALT)", "a")],
        ),
    )
    .unwrap();
    history::save(
        dir.path(),
        &run(
            "2026-10-10_10-00-00",
            Suite::Dpi,
            false,
            vec![result("general (ALT2)", "b")],
        ),
    )
    .unwrap();
    let ids: Vec<String> = history::summaries(dir.path())
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, ["2026-10-10_10-00-00", "2026-10-09_10-00-00"]);
    assert_eq!(
        history::load(dir.path(), "2026-10-09_10-00-00")
            .unwrap()
            .strategies[0]
            .name,
        "general (ALT)"
    );
    assert!(history::load(dir.path(), r"..\..\secret").is_err());
}

#[test]
fn a_run_saved_before_zapret2_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = serde_json::to_value(run(
        "2026-10-09_10-00-00",
        Suite::Standard,
        false,
        vec![result("general", "a")],
    ))
    .unwrap();
    old.as_object_mut().unwrap().remove("zapret2_version");
    std::fs::write(dir.path().join("2026-10-09_10-00-00.json"), old.to_string()).unwrap();
    let loaded = history::load(dir.path(), "2026-10-09_10-00-00").unwrap();
    assert_eq!(loaded.zapret2_version, None);
    assert_eq!(loaded.strategies[0].name, "general");
}

#[test]
fn latest_result_per_strategy_and_the_latest_full_ranking() {
    let dir = tempfile::tempdir().unwrap();
    history::save(
        dir.path(),
        &run(
            "2026-10-08_10-00-00",
            Suite::Standard,
            false,
            vec![
                result("general (ALT)", "old"),
                result("general (ALT2)", "x"),
            ],
        ),
    )
    .unwrap();
    history::save(
        dir.path(),
        &run(
            "2026-10-09_10-00-00",
            Suite::Standard,
            true,
            vec![result("general (ALT)", "new")],
        ),
    )
    .unwrap();
    history::save(
        dir.path(),
        &run(
            "2026-10-10_10-00-00",
            Suite::Dpi,
            false,
            vec![result("general (ALT3)", "d")],
        ),
    )
    .unwrap();
    let latest = history::latest_results(dir.path()).unwrap();
    assert_eq!(latest["general (alt)"].result.args_sha256, "new");
    assert_eq!(latest["general (alt2)"].run_id, "2026-10-08_10-00-00");
    assert_eq!(latest["general (alt3)"].suite, Suite::Dpi);
    assert_eq!(
        history::latest_ranking(dir.path()).unwrap().unwrap().id,
        "2026-10-08_10-00-00"
    );
}

#[test]
fn a_damaged_run_file_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("2026-10-10_00-00-00.json"), "{").unwrap();
    history::save(
        dir.path(),
        &run("2026-10-09_10-00-00", Suite::Standard, false, Vec::new()),
    )
    .unwrap();
    assert_eq!(history::summaries(dir.path()).unwrap().len(), 1);
}

/// Runs and requests from before repeats have no `passes` and no `repeats`.
#[test]
fn runs_and_requests_from_before_repeats_still_load() {
    let mut old = serde_json::to_value(run(
        "2026-10-09_10-00-00",
        Suite::Standard,
        false,
        vec![result("general", "a")],
    ))
    .unwrap();
    old["strategies"][0]
        .as_object_mut()
        .unwrap()
        .remove("passes")
        .unwrap();
    let loaded: TestRun = serde_json::from_value(old).unwrap();
    assert!(loaded.strategies[0].passes.is_empty());
    let request: tester::run::Request =
        serde_json::from_str(r#"{ "suite": "Standard", "strategies": ["general"] }"#).unwrap();
    assert_eq!(request.repeats, 1);
    request.check().unwrap();
    let too_many = tester::run::Request {
        repeats: 6,
        ..request
    };
    assert!(too_many.check().is_err());
}

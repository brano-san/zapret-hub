use tester::history;
use tester::model::{Score, StrategyResult, Suite, TestRun};

/// A zapret2 folder whose Lua defines only `fake`: the built-in strategies fail the static check.
fn fake_zapret2() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in [
        zapret2::EXE,
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        std::fs::write(root.join(file), b"").unwrap();
    }
    std::fs::create_dir_all(root.join("lua")).unwrap();
    std::fs::write(
        root.join("lua").join("zapret-lib.lua"),
        "NFQWS2_COMPAT_VER_REQUIRED=6\nfunction fake(ctx, desync)\nend\n",
    )
    .unwrap();
    std::fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
    dir
}

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

/// zapret2's built-in strategies as rows name them: bol-van's two, then the Flowseal copies.
fn built_in() -> Vec<String> {
    zapret2::strategies::list(tempfile::tempdir().unwrap().path())
        .unwrap()
        .into_iter()
        .map(|s| s.full_name())
        .collect()
}

#[test]
fn zapret2_rows_carry_their_checks_and_results() {
    let engine_dir = fake_zapret2();
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let text = "# mine\n--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"\n\
                --filter-tcp=443 --lua-desync=fake:blob=fake_default_tls\n--new\n--filter-udp=443\n";
    zapret2::strategies::write(engine_dir.path(), "mine", text).unwrap();
    let tail = zapret2::strategies::tail(&engine, text).unwrap();
    let run = TestRun {
        id: "2026-10-11_10-00-00".into(),
        suite: Suite::Standard,
        zapret2_version: Some("v1.0.5.2 (6b6c63e)".into()),
        canceled: false,
        restore_error: None,
        strategies: vec![result("zapret2:mine", &zapret::sha256_hex(tail.as_bytes()))],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    assert!(
        tester::catalog::rows(None, data.path(), None)
            .unwrap()
            .is_empty()
    );
    let rows = tester::catalog::rows(Some(&engine), data.path(), Some("ZAPRET2:MINE")).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let expected: Vec<String> = built_in()
        .into_iter()
        .chain(["zapret2:mine".to_owned()])
        .collect();
    assert_eq!(names, expected);
    let builtin = &rows[0];
    assert!(!builtin.custom && !builtin.active);
    assert!(
        builtin.error.as_deref().unwrap().contains("multidisorder"),
        "{builtin:?}"
    );
    let mine = rows.last().unwrap();
    assert!(!rows[1].flowseal_copy && rows[2].flowseal_copy);
    assert!(
        mine.custom && mine.active && mine.error.is_none(),
        "{mine:?}"
    );
    assert!(!mine.last.as_ref().unwrap().stale);

    let detail = tester::catalog::detail(Some(&engine), data.path(), None, "zapret2:mine").unwrap();
    assert_eq!(
        detail.wf,
        "--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\""
    );
    assert_eq!(
        detail.blocks,
        [
            "--filter-tcp=443 --lua-desync=fake:blob=fake_default_tls",
            "--filter-udp=443"
        ]
    );
    assert_eq!(detail.final_args, Some(tail));
    assert_eq!(detail.source, text);
    assert!(detail.result.is_some());

    zapret2::strategies::write(
        engine_dir.path(),
        "mine",
        &text.replace("udp=443", "udp=80"),
    )
    .unwrap();
    let rows = tester::catalog::rows(Some(&engine), data.path(), None).unwrap();
    assert!(
        rows.last().unwrap().last.as_ref().unwrap().stale,
        "an edit makes the result stale"
    );
}

#[test]
fn a_name_without_the_prefix_is_no_strategy() {
    let engine_dir = fake_zapret2();
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let err = tester::catalog::detail(Some(&engine), data.path(), None, "general").unwrap_err();
    assert!(
        format!("{err:#}").contains("no zapret2 strategy named general"),
        "{err:#}"
    );
    let err = tester::catalog::detail(None, data.path(), None, "zapret2:general").unwrap_err();
    assert!(
        format!("{err:#}").contains("zapret2 folder is not set"),
        "{err:#}"
    );
}

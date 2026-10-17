use tester::history;
use tester::model::{Score, StrategyResult, Suite, TestRun};
use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;

/// A Flowseal folder with one real strategy and one helper `.bat`.
fn fake_zapret() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("service.bat"), "set \"LOCAL_VERSION=1.10.3\"\r\n").unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin").join("winws.exe"), b"").unwrap();
    std::fs::write(root.join("bin").join("WinDivert64.sys"), b"").unwrap();
    std::fs::create_dir_all(root.join("lists")).unwrap();
    std::fs::create_dir_all(root.join("utils")).unwrap();
    std::fs::write(
        root.join("general.bat"),
        "start \"z\" /min \"%BIN%winws.exe\" --wf-tcp=80 ^\r\n--filter-tcp=80 --dpi-desync=fake\r\n",
    )
    .unwrap();
    std::fs::write(root.join("helper.bat"), "@echo off\r\necho hi\r\n").unwrap();
    dir
}

/// A zapret2 folder whose Lua defines only `fake`: the built-in strategies fail the static check.
fn fake_zapret2() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in [
        "winws2.exe",
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

#[test]
fn rows_carry_custom_active_errors_and_stale_results() {
    let zapret_dir = fake_zapret();
    let folder = Folder::open(zapret_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let mine = CustomStrategy {
        name: "mine".into(),
        cloned_from: "general".into(),
        wf: "--wf-tcp=80".into(),
        blocks: vec!["--filter-tcp=80".into()],
    };
    store.save(&mine).unwrap();
    zapret::custom::write_bat(&folder, &mine).unwrap();
    std::fs::write(
        zapret_dir.path().join("general (HUB orphan).bat"),
        mine.to_bat(),
    )
    .unwrap();

    let general_sha = zapret::batparse::parse_file(&folder, &zapret_dir.path().join("general.bat"))
        .unwrap()
        .sha256();
    let run = TestRun {
        id: "2026-10-10_10-00-00".into(),
        suite: Suite::Standard,
        flowseal_version: None,
        zapret2_version: None,
        canceled: false,
        restore_error: None,
        strategies: vec![
            result("general", &general_sha),
            result("general (HUB mine)", "outdated"),
        ],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    let rows =
        tester::catalog::rows(Some(&folder), None, &store, data.path(), Some("GENERAL")).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "general (HUB mine)",
            "general (HUB orphan)",
            "general",
            "helper"
        ]
    );
    let [mine_row, orphan, general, helper] = &rows[..] else {
        panic!("{rows:?}")
    };
    assert!(mine_row.custom && !mine_row.active && mine_row.last.as_ref().unwrap().stale);
    assert!(!orphan.custom && orphan.last.is_none());
    assert!(general.active && !general.last.as_ref().unwrap().stale);
    assert!(helper.error.as_deref().unwrap().contains("no winws.exe"));

    let detail = tester::catalog::detail(
        Some(&folder),
        None,
        &store,
        data.path(),
        None,
        "general (HUB mine)",
    )
    .unwrap();
    assert_eq!(detail.wf, "--wf-tcp=80");
    assert_eq!(detail.blocks, ["--filter-tcp=80"]);
    assert_eq!(detail.record, Some(mine));
    assert_eq!(detail.result.unwrap().args_sha256, "outdated");
    assert_eq!(detail.source, "");
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
fn zapret2_rows_follow_flowseals_with_their_checks_and_results() {
    let zapret_dir = fake_zapret();
    let folder = Folder::open(zapret_dir.path()).unwrap();
    let engine_dir = fake_zapret2();
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let text = "# mine\n--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"\n\
                --filter-tcp=443 --lua-desync=fake:blob=fake_default_tls\n--new\n--filter-udp=443\n";
    zapret2::strategies::write(engine_dir.path(), "mine", text).unwrap();
    let tail = zapret2::strategies::tail(&engine, Some(&folder), text).unwrap();
    let run = TestRun {
        id: "2026-10-11_10-00-00".into(),
        suite: Suite::Standard,
        flowseal_version: None,
        zapret2_version: Some("v1.0.5.2 (6b6c63e)".into()),
        canceled: false,
        restore_error: None,
        strategies: vec![result("zapret2:mine", &zapret::sha256_hex(tail.as_bytes()))],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    let without = tester::catalog::rows(Some(&folder), None, &store, data.path(), None).unwrap();
    assert!(without.iter().all(|row| !row.name.starts_with("zapret2:")));
    let rows = tester::catalog::rows(
        Some(&folder),
        Some(&engine),
        &store,
        data.path(),
        Some("ZAPRET2:MINE"),
    )
    .unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let expected: Vec<String> = ["general".to_owned(), "helper".to_owned()]
        .into_iter()
        .chain(built_in())
        .chain(["zapret2:mine".to_owned()])
        .collect();
    assert_eq!(names, expected);
    let builtin = &rows[2];
    assert!(!builtin.custom && !builtin.active);
    assert!(
        builtin.error.as_deref().unwrap().contains("multidisorder"),
        "{builtin:?}"
    );
    let mine = rows.last().unwrap();
    assert!(!rows[3].flowseal_copy && rows[4].flowseal_copy);
    assert!(
        mine.custom && mine.active && mine.error.is_none(),
        "{mine:?}"
    );
    assert!(!mine.last.as_ref().unwrap().stale);

    let detail = tester::catalog::detail(
        Some(&folder),
        Some(&engine),
        &store,
        data.path(),
        None,
        "zapret2:mine",
    )
    .unwrap();
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
    assert_eq!(detail.record, None);
    assert!(detail.result.is_some());

    zapret2::strategies::write(
        engine_dir.path(),
        "mine",
        &text.replace("udp=443", "udp=80"),
    )
    .unwrap();
    let rows =
        tester::catalog::rows(Some(&folder), Some(&engine), &store, data.path(), None).unwrap();
    assert!(
        rows.last().unwrap().last.as_ref().unwrap().stale,
        "an edit makes the result stale"
    );
}

#[test]
fn zapret2_alone_lists_its_strategies_and_names_what_needs_flowseal() {
    let engine_dir = fake_zapret2();
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let text = "--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"\n\
                --filter-tcp=443 --lua-desync=fake:blob=fake_default_tls\n";
    zapret2::strategies::write(engine_dir.path(), "mine", text).unwrap();
    let rows = tester::catalog::rows(None, Some(&engine), &store, data.path(), None).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let expected: Vec<String> = built_in()
        .into_iter()
        .chain(["zapret2:mine".to_owned()])
        .collect();
    assert_eq!(names, expected);
    // The built-in ones run on Flowseal's lists.
    assert!(
        rows[0].error.as_deref().unwrap().contains("%LISTS%"),
        "{:?}",
        rows[0]
    );
    assert!(rows[2].flowseal_copy && rows[2].error.is_some());
    assert_eq!(rows.last().unwrap().error, None);
    let detail = tester::catalog::detail(
        None,
        Some(&engine),
        &store,
        data.path(),
        None,
        "zapret2:mine",
    )
    .unwrap();
    assert!(detail.final_args.is_some());
    let err = tester::catalog::detail(None, Some(&engine), &store, data.path(), None, "general")
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("zapret folder is not set"),
        "{err:#}"
    );
}

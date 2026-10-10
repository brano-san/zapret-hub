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

fn result(name: &str, sha: &str) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: sha.into(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
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
        canceled: false,
        restore_error: None,
        strategies: vec![
            result("general", &general_sha),
            result("general (HUB mine)", "outdated"),
        ],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    let rows = tester::catalog::rows(&folder, &store, data.path(), Some("GENERAL")).unwrap();
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

    let detail =
        tester::catalog::detail(&folder, &store, data.path(), None, "general (HUB mine)").unwrap();
    assert_eq!(detail.wf, "--wf-tcp=80");
    assert_eq!(detail.blocks, ["--filter-tcp=80"]);
    assert_eq!(detail.record, Some(mine));
    assert_eq!(detail.result.unwrap().args_sha256, "outdated");
}

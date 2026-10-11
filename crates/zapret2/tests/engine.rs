mod common;

use std::path::Path;

use zapret2::{Engine, info, parse_banner, required_compat};

#[test]
fn winws2_is_found_in_every_layout() {
    for (exe_dir, exe) in [
        ("", "winws2.exe"),
        ("bin", r"bin\winws2.exe"),
        (
            r"binaries\windows-x86_64",
            r"binaries\windows-x86_64\winws2.exe",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        common::fake_engine(dir.path(), exe_dir, common::LIB);
        let engine = Engine::open(dir.path()).unwrap();
        assert_eq!(engine.root(), dir.path());
        assert_eq!(engine.exe(), dir.path().join(exe));
    }
}

#[test]
fn the_bundle_root_is_its_zapret_winws_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("zapret-winws");
    common::fake_engine(&root, "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    assert_eq!(engine.root(), root);
    assert_eq!(engine.lua(), root.join("lua"));
}

#[test]
fn a_trailing_backslash_is_not_part_of_the_root() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let picked = format!(r"{}\", dir.path().display());
    let engine = Engine::open(&picked).unwrap();
    assert_eq!(engine.root().as_os_str(), dir.path().as_os_str());
}

fn error(dir: &Path) -> String {
    format!("{:#}", Engine::open(dir).unwrap_err())
}

#[test]
fn every_missing_file_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let err = error(dir.path());
    for part in [
        "is not a zapret2 folder",
        "winws2.exe",
        r"lua\zapret-lib.lua",
        r"lua\zapret-antidpi.lua",
    ] {
        assert!(err.contains(part), "{part}: {err}");
    }
    common::fake_engine(dir.path(), "bin", common::LIB);
    std::fs::remove_file(dir.path().join("bin").join("cygwin1.dll")).unwrap();
    std::fs::remove_file(dir.path().join("bin").join("WinDivert64.sys")).unwrap();
    let err = error(dir.path());
    assert!(
        err.ends_with(r"missing bin\cygwin1.dll, bin\WinDivert64.sys"),
        "{err}"
    );
}

#[test]
fn a_cyrillic_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Запрет");
    common::fake_engine(&root, "", common::LIB);
    assert!(error(&root).contains("Cyrillic"), "{}", error(&root));
}

#[test]
fn a_drive_root_is_refused() {
    // `--chdir="C:\"` ends in `\"`, which winws2 reads as an escaped quote: no strategy would pass.
    let temp = std::env::temp_dir();
    let Some(std::path::Component::Prefix(drive)) = temp.components().next() else {
        panic!("{} has no drive", temp.display());
    };
    let root = format!(r"{}\", drive.as_os_str().to_string_lossy());
    let err = error(Path::new(&root));
    assert!(
        err.contains(&root) && err.contains("put zapret2 in a folder"),
        "{err}"
    );
}

#[test]
fn the_banner_gives_the_version_and_the_compat_level() {
    let github =
        "github version v1.0.5.2 (6b6c63e3385fa73f8af3be4a69171e947f5a319d) lua_compat_ver 6\n\n";
    assert_eq!(
        parse_banner(github),
        Some((
            "v1.0.5.2 (6b6c63e3385fa73f8af3be4a69171e947f5a319d)".into(),
            6
        ))
    );
    let own = "self-built version Oct 11 2026 12:00:00 lua_compat_ver 5\r\n";
    assert_eq!(
        parse_banner(own),
        Some(("self-built version Oct 11 2026 12:00:00".into(), 5))
    );
    assert_eq!(parse_banner("winws2: unknown option\n"), None);
    assert_eq!(parse_banner("github version v1 lua_compat_ver six"), None);
}

#[test]
fn the_lua_files_say_which_compat_level_they_need() {
    assert_eq!(required_compat(common::LIB), Some(6));
    assert_eq!(
        required_compat("NFQWS2_COMPAT_VER_REQUIRED = 7\r\n"),
        Some(7)
    );
    assert_eq!(
        required_compat("if NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then"),
        None
    );
}

#[test]
fn info_is_empty_without_a_folder_and_names_what_is_wrong() {
    assert_eq!(info(None), zapret2::Info::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().display().to_string();
    let shown = info(Some(&path));
    assert_eq!(shown.dir.as_deref(), Some(path.as_str()));
    assert_eq!(shown.version, None);
    assert!(
        shown.error.is_some_and(|err| err.contains("winws2.exe")),
        "the error names the missing engine"
    );
}

#[test]
fn a_start_that_fails_names_the_exe() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    // An empty winws2.exe is no program: Windows refuses to start it (os error 193).
    let err = engine.version().unwrap_err().to_string();
    let exe = engine.exe().display().to_string();
    assert!(err.contains(&exe), "the error names the exe: {err}");
    assert!(err.contains("(os error 193)"), "and keeps its cause: {err}");
    let shown = info(Some(&dir.path().display().to_string()));
    assert_eq!(shown.error, Some(err), "the same text reaches Info");
}

#[test]
#[ignore = "needs admin rights and an unpacked zapret2 release in ZAPRET2_DIR"]
fn live_version_of_a_release() {
    let dir = std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR is the zapret2 folder");
    let version = Engine::open(&dir).unwrap().version().unwrap();
    eprintln!("{version:?}");
    assert_eq!(version.compat, 6);
    let shown = info(Some(&dir));
    assert_eq!(shown.version, Some(version.text));
    assert_eq!(shown.error, None);
}

use std::fs;

use tester::engine::{launch, launch_text};

#[test]
fn launch_unknown_name_errors() {
    let err = launch(None, "nope").err().unwrap();
    assert!(
        format!("{err:#}").contains("no zapret2 strategy named nope"),
        "{err:#}"
    );
    let err = launch(None, "zapret2:nope").err().unwrap();
    assert!(format!("{err:#}").contains("folder is not set"), "{err:#}");
}

/// A zapret2 folder with the files `Engine::open` wants and one Lua function, `fake`.
fn engine(dir: &std::path::Path) -> zapret2::Engine {
    let root = dir;
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        fs::write(root.join(file), b"").unwrap();
    }
    fs::create_dir_all(root.join("lua")).unwrap();
    fs::write(
        root.join("lua").join("zapret-lib.lua"),
        "NFQWS2_COMPAT_VER_REQUIRED=6\n\nif NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then\n",
    )
    .unwrap();
    fs::write(
        root.join("lua").join("zapret-antidpi.lua"),
        "function fake(ctx, desync) end",
    )
    .unwrap();
    zapret2::Engine::open(root).unwrap()
}

#[test]
fn launch_text_rejects_bad_args() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    assert!(launch_text(&engine, "--lua-desync=nosuchfn").is_err());
    assert!(launch_text(&engine, "--lua-desync=").is_err());
}

#[test]
fn launch_text_starts_winws2_in_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let launch = launch_text(&engine, "--filter-tcp=443 --lua-desync=fake").unwrap();
    assert!(launch.zapret2);
    assert_eq!(launch.exe, engine.exe());
    assert_eq!(launch.cwd, engine.root());
    assert!(launch.tail.contains("--lua-desync=fake"), "{}", launch.tail);
    assert_eq!(launch.sha256, zapret::sha256_hex(launch.tail.as_bytes()));
}

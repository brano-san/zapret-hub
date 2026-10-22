use std::fs;

use tester::engine::{launch, launch_text};

#[test]
fn launch_unknown_name_errors() {
    let err = launch(None, None, "nope").err().unwrap();
    assert!(format!("{err:#}").contains("folder is not set"), "{err:#}");
}

#[test]
fn launch_text_rejects_bad_args() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
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
    fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
    let engine = zapret2::Engine::open(root).unwrap();
    assert!(launch_text(&engine, None, "--lua-desync=nosuchfn").is_err());
    assert!(launch_text(&engine, None, "--lua-desync=").is_err());
}

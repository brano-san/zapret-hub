#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// A zapret2 folder under `root` with `winws2.exe` and its DLLs in `exe_dir` (`""`, `bin`, ...)
/// and `lua\`; `lib` is the text of `lua\zapret-lib.lua`. The files are empty: nothing runs them.
pub fn fake_engine(root: &Path, exe_dir: &str, lib: &str) {
    let exe_dir = root.join(exe_dir);
    fs::create_dir_all(&exe_dir).unwrap();
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        fs::write(exe_dir.join(file), b"").unwrap();
    }
    fs::create_dir_all(root.join("lua")).unwrap();
    fs::write(root.join("lua").join("zapret-lib.lua"), lib).unwrap();
    fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
}

/// `zapret-lib.lua` as a release starts it.
pub const LIB: &str =
    "NFQWS2_COMPAT_VER_REQUIRED=6\n\nif NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then\n";

/// A Flowseal folder with what `Folder::open` checks, `utils\`, and `files` (`lists\x.txt`,
/// `bin\y.bin`) empty.
pub fn fake_flowseal(root: &Path, files: &[&str]) -> zapret::folder::Folder {
    for dir in ["bin", "lists", "utils"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    fs::write(root.join("service.bat"), "set \"LOCAL_VERSION=1.10.3\"\r\n").unwrap();
    for file in [r"bin\winws.exe", r"bin\WinDivert64.sys"]
        .iter()
        .chain(files)
    {
        fs::write(root.join(file), b"").unwrap();
    }
    zapret::folder::Folder::open(root).unwrap()
}

/// `zapret-lib.lua` that defines every function zapret2's own Lua files define.
pub fn lib_with_functions() -> String {
    let names = include_str!("../fixtures/functions.txt");
    let mut lib = String::from(LIB);
    for name in names
        .lines()
        .filter(|line| !line.starts_with('#'))
        .flat_map(str::split_whitespace)
    {
        lib.push_str(&format!("function {name}(ctx, desync)\nend\n"));
    }
    lib
}

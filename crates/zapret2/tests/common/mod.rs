#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// A zapret2 folder under `root` with the engine (and on Windows its DLLs) in `exe_dir` (`""`,
/// `bin`, ...) and `lua\`; `lib` is the text of `lua\zapret-lib.lua`. The files are empty:
/// nothing runs them.
pub fn fake_engine(root: &Path, exe_dir: &str, lib: &str) {
    let exe_dir = root.join(exe_dir);
    fs::create_dir_all(&exe_dir).unwrap();
    for file in [
        zapret2::EXE,
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

/// How the system refuses to start an empty engine file: not a program (Windows), not executable
/// (Linux).
#[cfg(windows)]
pub const CANNOT_RUN: &str = "(os error 193)";
#[cfg(target_os = "linux")]
pub const CANNOT_RUN: &str = "(os error 13)";

/// `zapret-lib.lua` as a release starts it.
pub const LIB: &str =
    "NFQWS2_COMPAT_VER_REQUIRED=6\n\nif NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then\n";

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

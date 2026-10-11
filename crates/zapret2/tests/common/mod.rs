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

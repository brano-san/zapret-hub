//! The manifests go to the linker, not into tauri-build's resource: that resource reaches only
//! binaries, and a test binary without Common-Controls v6 does not start
//! (STATUS_ENTRYPOINT_NOT_FOUND), the workaround Tauri's own examples use.

use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("common-controls.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Only the app asks for elevation: tests and the bindings export run in a plain terminal.
    println!("cargo:rustc-link-arg-bin=zapret-hub=/MANIFESTUAC:level='requireAdministrator'");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("failed to run tauri-build");
}

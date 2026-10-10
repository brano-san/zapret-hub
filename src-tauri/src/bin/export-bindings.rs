//! Writes `frontend/src/lib/ipc/bindings.ts` without starting the app, so without elevation.

fn main() {
    if let Err(err) = zapret_hub_lib::export_bindings() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

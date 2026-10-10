#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = zapret_hub_lib::run() {
        zapret_hub_lib::report(&err);
    }
}

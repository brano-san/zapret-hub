//! The check that runs the engine and the `zapret2` service (zapret2 spec 5.3, 6).
//! `zapret::service` does the `sc`/`systemctl` work; this module knows what zapret2 needs from it.
//! On Linux `nfqws2` gets the arguments `nft::translate` makes, and its queue the table
//! `nft::write_config` writes.

use anyhow::Context as _;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use zapret::service::{self, QuickCheck, SERVICE2};

use crate::Engine;
use crate::strategies::{self, Strategy};

/// `--dry-run` goes last: before an option it does not know, it ends with code 1 and says
/// nothing. `--wf-dup-check=0` lets it pass next to a running winws2 with the same filter.
#[cfg(windows)]
pub fn dry_run_args(tail: &str) -> String {
    format!("{tail} --wf-dup-check=0 --dry-run")
}

/// `--dry-run` goes last (see the Windows one); nfqws2 has no WinDivert filter to duplicate.
#[cfg(target_os = "linux")]
pub fn dry_run_args(tail: &str) -> String {
    format!("{tail} --dry-run")
}

/// The tail as this system's engine reads it: as it is on Windows; on Linux without `--wf-*`,
/// with the queue (`nft::translate`).
#[cfg(windows)]
fn engine_args(tail: &str) -> anyhow::Result<String> {
    Ok(tail.to_owned())
}

#[cfg(target_os = "linux")]
fn engine_args(tail: &str) -> anyhow::Result<String> {
    Ok(crate::nft::translate(tail)?.args)
}

/// Lua starts for real behind an empty WinDivert filter: traffic is not touched.
pub fn intercept_args(tail: &str) -> String {
    format!("{tail} --intercept=0")
}

/// The service's `binPath` (`ExecStart` on Linux): `"<engine>" <tail>`, `tail` as the engine
/// reads it.
pub fn bin_path(engine: &Engine, tail: &str) -> String {
    format!("\"{}\" {tail}", engine.exe().display())
}

/// The whole check (spec 5.3), to the first failure: the static one, `--dry-run`,
/// `--intercept=0`. Runs the engine, so it needs admin rights (Windows) or its capabilities
/// (Linux). An error carries the engine's whole output.
pub fn validate(engine: &Engine, tail: &str) -> anyhow::Result<()> {
    strategies::check(engine, tail)?;
    let tail = &engine_args(tail)?;
    let dry = run(engine, &dry_run_args(tail))?;
    let verified = [&dry.stdout, &dry.stderr]
        .iter()
        .any(|text| text.contains("command line parameters verified"));
    anyhow::ensure!(
        dry.success() && verified,
        "{} --dry-run did not accept the strategy:\n{}",
        crate::EXE,
        dry.describe()
    );
    let init = run(engine, &intercept_args(tail))?;
    anyhow::ensure!(
        init.success(),
        "{} --intercept=0 failed{}:\n{}",
        crate::EXE,
        if init.code == Some(87) {
            " to start the Lua code"
        } else {
            ""
        },
        init.describe()
    );
    Ok(())
}

fn run(engine: &Engine, args: &str) -> anyhow::Result<exec::Output> {
    let mut cmd = Command::new(engine.exe());
    #[cfg(windows)]
    cmd.raw_arg(args);
    #[cfg(target_os = "linux")]
    cmd.args(zapret::spawn_tokens(args));
    cmd.current_dir(engine.root());
    exec::run(&mut cmd, Duration::from_secs(30), None)
        .with_context(|| format!("starting {} {args}", engine.exe().display()))
}

/// Steps 2–5 of spec 6: the previous `zapret2` gone, TCP timestamps on (as Flowseal does),
/// `zapret2` created and started, its strategy recorded. Flowseal's `zapret` is left to the system
/// checks, which name it a conflict and remove it at the user's word. On Linux the queue's table
/// file goes first: the unit applies it at its start.
pub fn install(engine: &Engine, strategy: &Strategy, tail: &str) -> anyhow::Result<()> {
    service::stop_and_delete(SERVICE2)?;
    zapret::procs::kill_all(service::WINWS2)?;
    let _ = service::enable_tcp_timestamps();
    #[cfg(target_os = "linux")]
    let tail = &{
        let translated = crate::nft::translate(tail)?;
        crate::nft::write_config(&translated, engine.excluded_ports())?;
        translated.args
    };
    service::create(SERVICE2, &bin_path(engine, tail), true)?;
    service::start(SERVICE2)?;
    service::set_strategy_of(SERVICE2, &strategy.full_name())
}

/// The check of «Применить» (spec 6), run before any snapshot so that a failure changes nothing.
pub fn prepare(engine: &Engine, home: &Path, name: &str) -> anyhow::Result<(Strategy, String)> {
    let strategy = strategies::find(home, name)?;
    let tail = strategies::tail(engine, &strategy.text)?;
    validate(engine, &tail)?;
    Ok((strategy, tail))
}

/// «Применить» after `prepare`: the service, and after 2 s the quick check (spec 6).
pub fn install_checked(
    engine: &Engine,
    strategy: &Strategy,
    tail: &str,
) -> anyhow::Result<QuickCheck> {
    install(engine, strategy, tail)?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(service::quick_check())
}

/// «Применить» for `zapret2:<name>`: the check while nothing has changed yet, then the service.
pub fn apply(engine: &Engine, home: &Path, name: &str) -> anyhow::Result<QuickCheck> {
    let (strategy, tail) = prepare(engine, home, name)?;
    install_checked(engine, &strategy, &tail)
}

/// `save_zapret2` before the file (spec 5.2): the name, `create` against what exists, the whole
/// check. Returns the claimed name and the command line.
pub fn check_save(
    engine: &Engine,
    home: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<(String, String)> {
    let name = strategies::claim(home, name, create)?;
    let tail = strategies::tail(engine, text)?;
    validate(engine, &tail)?;
    Ok((name, tail))
}

/// `save_zapret2` up to the file (spec 5.2): `check_save`, then the file. Returns what was saved
/// and its command line, for a service that runs it.
pub fn save(
    engine: &Engine,
    home: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<(Strategy, String)> {
    let (name, tail) = check_save(engine, home, name, text, create)?;
    strategies::write(home, &name, text)?;
    let saved = Strategy {
        name,
        text: text.to_owned(),
        builtin: false,
        flowseal_copy: false,
    };
    Ok((saved, tail))
}

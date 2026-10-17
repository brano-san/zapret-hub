//! The check that runs winws2 and the `zapret2` service (zapret2 spec 5.3, 6). `zapret::service`
//! does the `sc` work; this module knows what zapret2 needs from it.

use anyhow::Context as _;
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use zapret::folder::Folder;
use zapret::service::{self, QuickCheck, SERVICE, SERVICE2, SvcState};

use crate::strategies::{self, Strategy};
use crate::{Engine, PREFIX};

/// `--dry-run` goes last: before an option it does not know, it ends with code 1 and says
/// nothing. `--wf-dup-check=0` lets it pass next to a running winws2 with the same filter.
pub fn dry_run_args(tail: &str) -> String {
    format!("{tail} --wf-dup-check=0 --dry-run")
}

/// Lua starts for real behind an empty WinDivert filter: traffic is not touched.
pub fn intercept_args(tail: &str) -> String {
    format!("{tail} --intercept=0")
}

/// The service's `binPath`: `"<winws2.exe>" <tail>`.
pub fn bin_path(engine: &Engine, tail: &str) -> String {
    format!("\"{}\" {tail}", engine.exe().display())
}

/// The whole check (spec 5.3), to the first failure: the static one, `--dry-run`,
/// `--intercept=0`. Runs winws2, so it needs admin rights. An error carries winws2's whole output.
pub fn validate(engine: &Engine, tail: &str) -> anyhow::Result<()> {
    strategies::check(engine, tail)?;
    let dry = run(engine, &dry_run_args(tail))?;
    let verified = [&dry.stdout, &dry.stderr]
        .iter()
        .any(|text| text.contains("command line parameters verified"));
    anyhow::ensure!(
        dry.success() && verified,
        "winws2 --dry-run did not accept the strategy:\n{}",
        dry.describe()
    );
    let init = run(engine, &intercept_args(tail))?;
    anyhow::ensure!(
        init.success(),
        "winws2 --intercept=0 failed{}:\n{}",
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
    cmd.raw_arg(args).current_dir(engine.root());
    exec::run(&mut cmd, Duration::from_secs(30), None)
        .with_context(|| format!("starting {} {args}", engine.exe().display()))
}

/// Steps 2–5 of spec 6: Flowseal's service and winws gone, the previous `zapret2` gone, TCP
/// timestamps on (as Flowseal does), `zapret2` created and started, its strategy recorded.
pub fn install(engine: &Engine, strategy: &Strategy, tail: &str) -> anyhow::Result<()> {
    service::stop_and_delete(SERVICE)?;
    zapret::procs::kill_all("winws.exe")?;
    service::stop_and_delete(SERVICE2)?;
    zapret::procs::kill_all(service::WINWS2)?;
    let _ = service::enable_tcp_timestamps();
    service::create(SERVICE2, &bin_path(engine, tail), true)?;
    service::start(SERVICE2)?;
    service::set_strategy_of(SERVICE2, &strategy.full_name())
}

/// The check of «Применить» (spec 6), run before any snapshot so that a failure changes nothing.
pub fn prepare(
    engine: &Engine,
    folder: Option<&Folder>,
    home: &Path,
    name: &str,
) -> anyhow::Result<(Strategy, String)> {
    let strategy = strategies::find(home, name)?;
    let tail = strategies::tail(engine, folder, &strategy.text)?;
    validate(engine, &tail)?;
    Ok((strategy, tail))
}

/// «Применить» after `prepare`: the service, and after 2 s the quick check (spec 6).
pub fn install_checked(
    engine: &Engine,
    folder: Option<&Folder>,
    strategy: &Strategy,
    tail: &str,
) -> anyhow::Result<QuickCheck> {
    install(engine, strategy, tail)?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(service::quick_check(folder))
}

/// «Применить» for `zapret2:<name>`: the check while nothing has changed yet, then the service.
pub fn apply(
    engine: &Engine,
    folder: Option<&Folder>,
    home: &Path,
    name: &str,
) -> anyhow::Result<QuickCheck> {
    let (strategy, tail) = prepare(engine, folder, home, name)?;
    install_checked(engine, folder, &strategy, &tail)
}

/// After a game filter change: the active zapret2 strategy installed again, its ports are in the
/// service's arguments (spec 6). Nothing to do while `zapret2` is not installed.
pub fn reinstall_active(
    engine: Option<&Engine>,
    folder: &Folder,
    home: &Path,
) -> anyhow::Result<()> {
    if service::state(SERVICE2) == SvcState::NotInstalled {
        return Ok(());
    }
    let engine = engine.ok_or_else(|| {
        anyhow::anyhow!(
            "the zapret2 service is installed, but the zapret2 folder is not usable, so the service cannot be installed again with the new game filter: check the folder in Settings"
        )
    })?;
    let name = service::strategy_of(SERVICE2)
        .and_then(|name| name.strip_prefix(PREFIX).map(str::to_owned))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "the zapret2 service has no zapret2 strategy name in the registry, so it cannot be installed again with the new game filter: apply a strategy first"
            )
        })?;
    let strategy = strategies::find(home, &name)?;
    let tail = strategies::tail(engine, Some(folder), &strategy.text)?;
    validate(engine, &tail)?;
    install(engine, &strategy, &tail)
}

/// `save_zapret2` before the file (spec 5.2): the name, `create` against what exists, the whole
/// check. Returns the claimed name and the command line.
pub fn check_save(
    engine: &Engine,
    folder: Option<&Folder>,
    home: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<(String, String)> {
    let name = strategies::claim(home, name, create)?;
    let tail = strategies::tail(engine, folder, text)?;
    validate(engine, &tail)?;
    Ok((name, tail))
}

/// `save_zapret2` up to the file (spec 5.2): `check_save`, then the file. Returns what was saved
/// and its command line, for a service that runs it.
pub fn save(
    engine: &Engine,
    folder: Option<&Folder>,
    home: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<(Strategy, String)> {
    let (name, tail) = check_save(engine, folder, home, name, text, create)?;
    strategies::write(home, &name, text)?;
    let saved = Strategy {
        name,
        text: text.to_owned(),
        builtin: false,
        flowseal_copy: false,
    };
    Ok((saved, tail))
}

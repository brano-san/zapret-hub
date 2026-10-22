//! Starting a strategy for a while: what a test run does for each strategy, and what other
//! callers (candidates, `hub-circular`) do to try one, without the checks.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use zapret::folder::Folder;
use zapret::procs::Winws;
use zapret::service::WINWS2;
use zapret2::Engine;

/// winws gets this long to report `capture is started` (spec 7.3).
const READY: Duration = Duration::from_secs(5);
/// The pause `test zapret.ps1` makes once winws is up.
const SETTLE: Duration = Duration::from_millis(300);
/// winws2 reports capture started before its Lua starts: it gets this long more, and must still
/// run then (zapret2 spec 7).
const LUA_SETTLE: Duration = Duration::from_secs(2);

/// How a strategy starts: the program, its working folder, the arguments and their hash.
pub struct Launch {
    pub exe: PathBuf,
    pub cwd: PathBuf,
    pub tail: String,
    pub sha256: String,
    pub zapret2: bool,
}

/// The one place a test run tells the engines apart (zapret2 spec 3.1): by the name's prefix.
pub fn launch(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    name: &str,
) -> anyhow::Result<Launch> {
    let Some(name) = zapret2::strip_prefix(name) else {
        let folder = folder.ok_or_else(|| anyhow::anyhow!(zapret::folder::NOT_SET))?;
        let file = zapret::strategies::find(folder, name)?;
        let parsed = zapret::batparse::parse_file(folder, &file.path)?;
        return Ok(Launch {
            exe: folder.winws(),
            cwd: folder.bin(),
            tail: parsed.command_tail(),
            sha256: parsed.sha256(),
            zapret2: false,
        });
    };
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    let strategy = zapret2::strategies::find(engine.root(), name)?;
    launch_text(engine, folder, &strategy.text)
}

/// A zapret2 strategy from its text (a candidate, a `hub-circular` result): placeholders filled
/// (`strategies::tail`), then the checks a saved one passes before it runs (`strategies::check`).
pub fn launch_text(
    engine2: &Engine,
    folder: Option<&Folder>,
    text: &str,
) -> anyhow::Result<Launch> {
    let tail = zapret2::strategies::tail(engine2, folder, text)?;
    zapret2::strategies::check(engine2, &tail)?;
    Ok(Launch {
        exe: engine2.exe().to_path_buf(),
        cwd: engine2.root().to_path_buf(),
        // ponytail: the engine version is not in the hash (the run keeps it); add it if old
        // results start to mislead (zapret2 spec 12).
        sha256: zapret::sha256_hex(tail.as_bytes()),
        tail,
        zapret2: true,
    })
}

/// Every winws.exe and winws2.exe ended, and gone (their WinDivert handles closed).
pub fn stop_engines() -> anyhow::Result<()> {
    for image in ["winws.exe", WINWS2] {
        zapret::procs::kill_all(image)?;
    }
    Ok(())
}

/// A started engine, ready and settled.
pub struct Running(Winws);

/// Nothing else filters, the engine starts and reports ready, and has settled. The error is the
/// launch error a run records.
pub fn start(launch: &Launch) -> anyhow::Result<Running> {
    stop_engines()?;
    let mut winws = Winws::start(&launch.exe, &launch.cwd, &launch.tail).map_err(|err| {
        let image = launch
            .exe
            .file_name()
            .map_or_else(|| "winws".into(), |n| n.to_string_lossy());
        anyhow::anyhow!("{image} did not start: {err}")
    })?;
    winws
        .wait_ready(READY)
        .map_err(|output| anyhow::anyhow!(output))?;
    std::thread::sleep(if launch.zapret2 { LUA_SETTLE } else { SETTLE });
    Ok(Running(winws))
}

impl Running {
    /// Ended on its own, with its status.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        self.0.exited()
    }

    /// Stops it and returns its whole output.
    pub fn stop(self) -> String {
        self.0.stop()
    }
}

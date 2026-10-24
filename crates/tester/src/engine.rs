//! Starting a strategy for a while: what a test run does for each strategy, and what other
//! callers (candidates, `hub-circular`) do to try one, without the checks.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use zapret::procs::Winws;
use zapret::service::WINWS2;
use zapret2::Engine;

/// winws gets this long to report `capture is started` (spec 7.3); nfqws2 to bind its queue.
const READY: Duration = Duration::from_secs(5);
/// The pause `test zapret.ps1` makes once winws is up.
#[cfg(windows)]
const SETTLE: Duration = Duration::from_millis(300);
/// winws2 reports capture started before its Lua starts: it gets this long more, and must still
/// run then (zapret2 spec 7). Linux nfqws2 binds its queue after the Lua: `wait_ready` settles it.
#[cfg(windows)]
const LUA_SETTLE: Duration = Duration::from_secs(2);

/// How a strategy starts: the program, its working folder, the arguments and their hash.
pub struct Launch {
    pub exe: PathBuf,
    pub cwd: PathBuf,
    pub tail: String,
    pub sha256: String,
    pub zapret2: bool,
    /// Linux: the ports the nftables queue feeds nfqws2 (`tail` is already translated).
    #[cfg(target_os = "linux")]
    pub queue: zapret2::nft::Translated,
}

/// A `zapret2:<name>` strategy (zapret2 spec 3.1).
pub fn launch(engine2: Option<&Engine>, name: &str) -> anyhow::Result<Launch> {
    let Some(name) = zapret2::strip_prefix(name) else {
        anyhow::bail!("no zapret2 strategy named {name}");
    };
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    let strategy = zapret2::strategies::find(engine.root(), name)?;
    launch_text(engine, &strategy.text)
}

/// A zapret2 strategy from its text (a candidate, a `hub-circular` result): placeholders filled
/// (`strategies::tail`), then the checks a saved one passes before it runs (`strategies::check`).
pub fn launch_text(engine2: &Engine, text: &str) -> anyhow::Result<Launch> {
    let tail = zapret2::strategies::tail(engine2, text)?;
    zapret2::strategies::check(engine2, &tail)?;
    // ponytail: the engine version is not in the hash (the run keeps it); add it if old results
    // start to mislead (zapret2 spec 12). The hash is of the tail «Стратегии» hashes too
    // (catalog.rs): the Linux translation is launch bookkeeping, not another strategy.
    let sha256 = zapret::sha256_hex(tail.as_bytes());
    #[cfg(target_os = "linux")]
    let queue = zapret2::nft::translate(&tail)?;
    Ok(Launch {
        exe: engine2.exe().to_path_buf(),
        cwd: engine2.root().to_path_buf(),
        sha256,
        #[cfg(target_os = "linux")]
        tail: queue.args.clone(),
        #[cfg(windows)]
        tail,
        zapret2: true,
        #[cfg(target_os = "linux")]
        queue,
    })
}

/// Every winws.exe and winws2.exe ended, and gone (their WinDivert handles closed).
pub fn stop_engines() -> anyhow::Result<()> {
    for image in ["winws.exe", WINWS2] {
        zapret::procs::kill_all(image)?;
    }
    Ok(())
}

/// A started engine, ready and settled. On Linux its nftables table goes when it is dropped.
pub struct Running(Option<Winws>);

/// Nothing else filters, the engine starts and reports ready, and has settled. The error is the
/// launch error a run records. Linux: the queue's table comes first (the excluded ports stay in:
/// a test measures the strategy by itself).
pub fn start(launch: &Launch) -> anyhow::Result<Running> {
    stop_engines()?;
    #[cfg(target_os = "linux")]
    zapret2::nft::apply(&launch.queue, &[]).map_err(|err| anyhow::anyhow!("nftables: {err:#}"))?;
    // From here a failure drops it, and the table with it.
    let mut running = Running(None);
    let winws = Winws::start(&launch.exe, &launch.cwd, &launch.tail).map_err(|err| {
        let image = launch
            .exe
            .file_name()
            .map_or_else(|| "winws".into(), |n| n.to_string_lossy());
        anyhow::anyhow!("{image} did not start: {err}")
    })?;
    let winws = running.0.insert(winws);
    winws
        .wait_ready(READY)
        .map_err(|output| anyhow::anyhow!(output))?;
    #[cfg(windows)]
    std::thread::sleep(if launch.zapret2 { LUA_SETTLE } else { SETTLE });
    Ok(running)
}

impl Running {
    /// Ended on its own, with its status.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        self.0.as_mut()?.exited()
    }

    /// Stops it and returns its whole output.
    pub fn stop(mut self) -> String {
        self.0.take().map(Winws::stop).unwrap_or_default()
    }
}

/// Linux: the table goes after the engine. Root each time: with pkexec that is a dialog.
// ponytail: apply and delete per strategy; one table across a run if the dialogs annoy.
#[cfg(target_os = "linux")]
impl Drop for Running {
    fn drop(&mut self) {
        drop(self.0.take());
        zapret2::nft::clear();
    }
}

//! A test run (spec 7.3): snapshot, zapret stopped, each strategy started as winws (or winws2,
//! zapret2 spec 7) directly with the arguments «Применить» would install, the checks, everything
//! put back.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use probe::curl::CheckKind;
use zapret::folder::Folder;
use zapret::procs::Winws;
use zapret::service::{SERVICE, SERVICE2, WINWS2};
use zapret::snapshot::{Snapshot, pending_path};
use zapret::targets::Target;
use zapret2::Engine;

use crate::dpi::DpiTarget;
use crate::model::{DpiTargetResult, Score, StrategyResult, Suite, TargetResult, TestRun};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Request {
    pub suite: Suite,
    /// Strategy names, in the order to run them.
    pub strategies: Vec<String>,
}

impl Request {
    /// What `run` refuses before it touches anything.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.strategies.is_empty(), "no strategies to test");
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum Progress {
    /// Strategy `index` (from 0) of `total` starts.
    Strategy {
        index: u32,
        total: u32,
        name: String,
    },
    Done {
        result: StrategyResult,
    },
    Restoring,
}

/// winws gets this long to report `capture is started` (spec 7.3).
const READY: Duration = Duration::from_secs(5);
/// The pause `test zapret.ps1` makes once winws is up.
const SETTLE: Duration = Duration::from_millis(300);
/// winws2 reports capture started before its Lua starts: it gets this long more, and must still
/// run then (zapret2 spec 7).
const LUA_SETTLE: Duration = Duration::from_secs(2);

/// What can fail is checked and fetched first, while zapret still works; then the state is saved,
/// zapret stopped and the strategies run. The state is put back even when something panics (the
/// panic goes on afterwards). `folder` runs Flowseal's strategies, `engine2` the `zapret2:` ones:
/// a run with only zapret2 strategies needs no Flowseal folder.
pub fn run(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<TestRun> {
    request.check()?;
    needs_flowseal(folder, request)?;
    let zapret2_version = zapret2_version(engine2, request)?;
    anyhow::ensure!(
        probe::curl::available(),
        "curl.exe was not found: the tests need Windows 10 1803 or newer"
    );
    let id = exec::LocalTime::now().stamp();
    // A Cancel or Exit while the DPI suite downloads: zapret is not touched at all.
    let not_canceled = || -> anyhow::Result<()> {
        anyhow::ensure!(
            !cancel.load(Ordering::Relaxed),
            "the test run was canceled before anything changed"
        );
        Ok(())
    };
    not_canceled()?;
    let work = prepare(folder, data_dir, request, socks5)?;
    not_canceled()?;
    let pending = pending_path(data_dir);
    // An earlier pending.json is restored first; when that fails nothing starts (spec 10).
    let snapshot = zapret::snapshot::begin(folder, "tests", &pending)?;
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let outcome = run_strategies(folder, engine2, data_dir, request, &work, cancel, progress);
        let canceled = cancel.load(Ordering::Relaxed);
        progress(Progress::Restoring);
        (outcome, canceled)
    }));
    let restore_error = snapshot.restore(folder).err().map(|err| format!("{err:#}"));
    match &restore_error {
        // Put back: the file has done its job.
        None => {
            if let Err(err) = Snapshot::clear(&pending) {
                log::error!("{} not removed: {err:#}", pending.display());
            }
        }
        // Not put back: the file stays, the next start or the next operation restores it.
        Some(err) => log::error!(
            "the state before the tests is not back, {} stays: {err}",
            pending.display()
        ),
    }
    let (outcome, canceled) = match ran {
        Ok(done) => done,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let mut strategies = match outcome {
        Ok(strategies) => strategies,
        Err(err) => {
            return Err(match restore_error {
                Some(restore) => anyhow::anyhow!(
                    "{err:#}\n\nthe previous state was not restored either: {restore}"
                ),
                None => err,
            });
        }
    };
    crate::rank::sort(request.suite, &mut strategies);
    let run = TestRun {
        id,
        suite: request.suite,
        flowseal_version: folder.and_then(Folder::version),
        zapret2_version,
        canceled,
        restore_error,
        strategies,
    };
    crate::history::save(&crate::history::dir(data_dir), &run)?;
    Ok(run)
}

/// The engine version for the run's record. A `zapret2:` strategy without a working engine stops
/// the run here, before anything changes (zapret2 spec 7).
pub fn zapret2_version(
    engine2: Option<&Engine>,
    request: &Request,
) -> anyhow::Result<Option<String>> {
    if !request
        .strategies
        .iter()
        .any(|name| zapret2::strip_prefix(name).is_some())
    {
        return Ok(None);
    }
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    Ok(Some(engine.version()?.text))
}

/// A Flowseal strategy without the Flowseal folder stops the run here, before anything changes.
pub fn needs_flowseal(folder: Option<&Folder>, request: &Request) -> anyhow::Result<()> {
    let flowseal = request
        .strategies
        .iter()
        .find(|name| zapret2::strip_prefix(name).is_none());
    if let (None, Some(name)) = (folder, flowseal) {
        anyhow::bail!("{name} is a Flowseal strategy: {}", zapret::folder::NOT_SET);
    }
    Ok(())
}

/// What every strategy is checked against.
enum Work {
    /// Targets with their `extra` flag.
    Standard(Vec<(Target, bool)>),
    Dpi(Vec<DpiTarget>, PathBuf),
}

/// Reads or downloads what the checks need. The DPI suite takes up to a minute to fetch and needs
/// the bypass, so this runs before zapret is stopped, and a failure leaves nothing to restore.
fn prepare(
    folder: Option<&Folder>,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
) -> anyhow::Result<Work> {
    Ok(match request.suite {
        Suite::Standard => {
            let targets = folder.map_or_else(zapret::targets::defaults, zapret::targets::load);
            let extra = crate::rank::extra_targets(&targets);
            Work::Standard(
                targets
                    .into_iter()
                    .map(|t| (t, false))
                    .chain(extra.into_iter().map(|t| (t, true)))
                    .collect(),
            )
        }
        Suite::Dpi => {
            let cache = data_dir.join("cache");
            let targets = crate::dpi::load(&cache, socks5)?;
            let payload = cache.join("dpi-payload.bin");
            probe::curl::write_payload(&payload)?;
            Work::Dpi(targets, payload)
        }
    })
}

fn run_strategies(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    data_dir: &Path,
    request: &Request,
    work: &Work,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<Vec<StrategyResult>> {
    // Both engines: a run may mix them (zapret2 spec 7).
    for (service, image) in [(SERVICE, "winws.exe"), (SERVICE2, WINWS2)] {
        zapret::service::stop(service)?;
        zapret::procs::kill_all(image)?;
    }
    // Every address through the ipset profiles; without Flowseal there are none.
    if let (Work::Dpi(..), Some(folder)) = (work, folder) {
        zapret::ipset::enter_test_any(folder)?;
    }
    let total = request.strategies.len() as u32;
    let mut results = Vec::new();
    for (index, name) in request.strategies.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(Progress::Strategy {
            index: index as u32,
            total,
            name: name.clone(),
        });
        let result = run_one(folder, engine2, data_dir, name, work, cancel);
        if cancel.load(Ordering::Relaxed) {
            break; // cut short: not a result
        }
        progress(Progress::Done {
            result: result.clone(),
        });
        results.push(result);
    }
    Ok(results)
}

/// How a strategy starts: the program, its working folder, the arguments and their hash.
struct Launch {
    exe: PathBuf,
    cwd: PathBuf,
    tail: String,
    sha256: String,
    zapret2: bool,
}

/// The one place a test run tells the engines apart (zapret2 spec 3.1): by the name's prefix.
fn launch(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    data_dir: &Path,
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
    let strategy = zapret2::strategies::find(data_dir, name)?;
    let tail = zapret2::strategies::tail(engine, folder, &strategy.text)?;
    zapret2::strategies::check(engine, &tail)?;
    Ok(Launch {
        exe: engine.exe().to_path_buf(),
        cwd: engine.root().to_path_buf(),
        // ponytail: the engine version is not in the hash (the run keeps it); add it if old
        // results start to mislead (zapret2 spec 12).
        sha256: zapret::sha256_hex(tail.as_bytes()),
        tail,
        zapret2: true,
    })
}

fn run_one(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    data_dir: &Path,
    name: &str,
    work: &Work,
    cancel: &AtomicBool,
) -> StrategyResult {
    let mut result = StrategyResult {
        name: name.to_owned(),
        args_sha256: String::new(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    };
    let launch = match launch(folder, engine2, data_dir, name) {
        Ok(launch) => launch,
        Err(err) => {
            result.launch_error = Some(format!("{err:#}"));
            return result;
        }
    };
    result.args_sha256 = launch.sha256;
    let mut winws = match Winws::start(&launch.exe, &launch.cwd, &launch.tail) {
        Ok(winws) => winws,
        Err(err) => {
            let image = launch
                .exe
                .file_name()
                .map_or_else(|| "winws".into(), |n| n.to_string_lossy());
            result.launch_error = Some(format!("{image} did not start: {err}"));
            return result;
        }
    };
    if let Err(output) = winws.wait_ready(READY) {
        result.launch_error = Some(output);
        return result;
    }
    std::thread::sleep(if launch.zapret2 { LUA_SETTLE } else { SETTLE });
    // A winws2 whose Lua failed is gone by now: nothing to check.
    if !(launch.zapret2 && winws.exited().is_some()) {
        let workers = parallelism();
        match work {
            Work::Standard(targets) => {
                result.targets = par_map(targets, workers, |(target, extra)| {
                    check_target(target, *extra, cancel)
                });
                result.score = crate::rank::score_standard(&result.targets);
            }
            Work::Dpi(targets, payload) => {
                result.dpi = par_map(targets, workers, |target| {
                    check_dpi(target, payload, cancel)
                });
                result.score = crate::rank::score_dpi(&result.dpi);
            }
        }
    }
    // Read after `stop`, which waits for the readers: the output is whole.
    let exited = winws.exited();
    let output = winws.stop();
    result.launch_error = if launch.zapret2 {
        lua_failure(exited, &output)
    } else {
        exited.map(|status| format!("winws exited during the checks ({status}):\n{output}"))
    };
    result
}

/// winws2 prints its ready line before its Lua starts. One that has ended, or printed `LUA ERROR`
/// (a strategy that fails on every packet does nothing), did not run the strategy: a launch error
/// with its whole output, or a strategy that did nothing would rank above working ones (zapret2
/// spec 7). Exit code 87 is a failed Lua start.
pub fn lua_failure(exited: Option<ExitStatus>, output: &str) -> Option<String> {
    if let Some(status) = exited {
        return Some(format!("winws2 exited ({status}):\n{output}"));
    }
    output
        .contains("LUA ERROR")
        .then(|| format!("winws2 printed LUA ERROR:\n{output}"))
}

fn check_target(target: &Target, extra: bool, cancel: &AtomicBool) -> TargetResult {
    let checks = match &target.url {
        Some(url) => CheckKind::ALL
            .iter()
            .map(|kind| probe::curl::check(url, *kind, Some(cancel)))
            .collect(),
        None => Vec::new(),
    };
    TargetResult {
        name: target.name.clone(),
        url: target.url.clone(),
        extra,
        checks,
        ping_ms: probe::ping::ping(&target.ping, 1000),
    }
}

fn check_dpi(target: &DpiTarget, payload: &Path, cancel: &AtomicBool) -> DpiTargetResult {
    let checks = CheckKind::ALL
        .iter()
        .map(|kind| probe::curl::dpi_check(&target.host, *kind, payload, Some(cancel)))
        .collect();
    DpiTargetResult {
        id: target.id.clone(),
        provider: target.provider.clone(),
        country: target.country.clone(),
        host: target.host.clone(),
        checks,
    }
}

/// `min(16, max(8, cores × 2))` targets at a time (spec 7.1).
pub fn parallelism() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .saturating_mul(2)
        .clamp(8, 16)
}

/// `f` over `items` on `workers` threads; the results keep the input order.
pub fn par_map<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    *slots[i].lock().unwrap() = Some(f(item));
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.into_inner().unwrap().expect("every slot is filled"))
        .collect()
}

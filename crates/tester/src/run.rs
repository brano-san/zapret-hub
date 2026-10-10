//! A test run (spec 7.3): snapshot, zapret stopped, each strategy started as winws directly with
//! the arguments «Применить» would install, the checks, everything put back.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use probe::curl::CheckKind;
use zapret::folder::Folder;
use zapret::procs::Winws;
use zapret::snapshot::{Snapshot, pending_path};
use zapret::targets::Target;

use crate::dpi::DpiTarget;
use crate::model::{DpiTargetResult, Score, StrategyResult, Suite, TargetResult, TestRun};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Request {
    pub suite: Suite,
    /// Strategy names, in the order to run them.
    pub strategies: Vec<String>,
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

pub fn run(
    folder: &Folder,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<TestRun> {
    anyhow::ensure!(
        probe::curl::available(),
        "curl.exe was not found: the tests need Windows 10 1803 or newer"
    );
    let id = exec::LocalTime::now().stamp();
    let pending = pending_path(data_dir);
    // An earlier pending.json is restored first; when that fails nothing starts (spec 10).
    let snapshot = zapret::snapshot::begin(folder, "tests", &pending)?;
    let outcome = run_strategies(folder, data_dir, request, socks5, cancel, progress);
    progress(Progress::Restoring);
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
    let mut strategies = outcome?;
    crate::rank::sort(request.suite, &mut strategies);
    let run = TestRun {
        id,
        suite: request.suite,
        flowseal_version: folder.version(),
        canceled: cancel.load(Ordering::Relaxed),
        restore_error,
        strategies,
    };
    crate::history::save(&crate::history::dir(data_dir), &run)?;
    Ok(run)
}

/// What every strategy is checked against.
enum Work {
    /// Targets with their `extra` flag.
    Standard(Vec<(Target, bool)>),
    Dpi(Vec<DpiTarget>, PathBuf),
}

fn run_strategies(
    folder: &Folder,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<Vec<StrategyResult>> {
    zapret::service::stop(zapret::service::SERVICE)?;
    zapret::procs::kill_all("winws.exe")?;
    let work = match request.suite {
        Suite::Standard => {
            let targets = zapret::targets::load(folder);
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
            zapret::ipset::enter_test_any(folder)?;
            Work::Dpi(targets, payload)
        }
    };
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
        let result = run_one(folder, name, &work, cancel);
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

fn run_one(folder: &Folder, name: &str, work: &Work, cancel: &AtomicBool) -> StrategyResult {
    let mut result = StrategyResult {
        name: name.to_owned(),
        args_sha256: String::new(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    };
    let parsed = zapret::strategies::find(folder, name)
        .and_then(|file| zapret::batparse::parse_file(folder, &file.path));
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            result.launch_error = Some(format!("{err:#}"));
            return result;
        }
    };
    result.args_sha256 = parsed.sha256();
    let mut winws = match Winws::start(folder, &parsed.command_tail()) {
        Ok(winws) => winws,
        Err(err) => {
            result.launch_error = Some(format!("winws did not start: {err}"));
            return result;
        }
    };
    if let Err(output) = winws.wait_ready(READY) {
        result.launch_error = Some(output);
        return result;
    }
    std::thread::sleep(SETTLE);
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
    if !winws.is_alive() {
        result.launch_error = Some(format!(
            "winws exited during the checks:\n{}",
            winws.output_text()
        ));
    }
    winws.stop();
    result
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

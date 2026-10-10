//! `data/tests/<id>.json`: every finished run (spec 7.5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{Score, StrategyResult, Suite, TestRun};

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tests")
}

pub fn save(dir: &Path, run: &TestRun) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(format!("{}.json", run.id)),
        serde_json::to_string(run)?,
    )?;
    Ok(())
}

pub fn load(dir: &Path, id: &str) -> anyhow::Result<TestRun> {
    anyhow::ensure!(
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '_'),
        "not a run id: {id}"
    );
    let path = dir.join(format!("{id}.json"));
    let text =
        std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Every readable run, newest first. A damaged file goes to the journal and is skipped: one bad
/// file must not hide the rest.
// ponytail: reads every run on each call; an index file when the history grows to hundreds.
pub fn all(dir: &Path) -> anyhow::Result<Vec<TestRun>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| Ok(serde_json::from_str::<TestRun>(&text)?));
        match parsed {
            Ok(run) => runs.push(run),
            Err(err) => log::error!("test run skipped, {}: {err:#}", path.display()),
        }
    }
    runs.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(runs)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RunSummary {
    pub id: String,
    pub suite: Suite,
    pub canceled: bool,
    pub strategies: u32,
    /// The first of the ranking, unless even it failed to start.
    pub best: Option<String>,
    pub best_score: Option<Score>,
}

pub fn summary(run: &TestRun) -> RunSummary {
    let best = run.strategies.first().filter(|s| s.launch_error.is_none());
    RunSummary {
        id: run.id.clone(),
        suite: run.suite,
        canceled: run.canceled,
        strategies: run.strategies.len() as u32,
        best: best.map(|s| s.name.clone()),
        best_score: best.map(|s| s.score.clone()),
    }
}

pub fn summaries(dir: &Path) -> anyhow::Result<Vec<RunSummary>> {
    Ok(all(dir)?.iter().map(summary).collect())
}

/// A strategy's newest result and the run it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Latest {
    pub run_id: String,
    pub suite: Suite,
    pub result: StrategyResult,
}

/// The newest result of every strategy, keyed by its lower-case name.
pub fn latest_results(dir: &Path) -> anyhow::Result<HashMap<String, Latest>> {
    let mut latest = HashMap::new();
    for run in all(dir)? {
        for result in run.strategies {
            latest
                .entry(result.name.to_lowercase())
                .or_insert_with(|| Latest {
                    run_id: run.id.clone(),
                    suite: run.suite,
                    result,
                });
        }
    }
    Ok(latest)
}

/// The newest standard run that finished: «Применить лучшую», the overview, the tray's top five.
pub fn latest_ranking(dir: &Path) -> anyhow::Result<Option<TestRun>> {
    Ok(all(dir)?
        .into_iter()
        .find(|run| run.suite == Suite::Standard && !run.canceled))
}

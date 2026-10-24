//! The rows and the detail pane of the «Стратегии» tab.

use std::collections::HashMap;
use std::path::Path;

use zapret2::Engine;
use zapret2::strategies::Strategy;

use crate::history::Latest;
use crate::model::{Score, StrategyResult, Suite};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LastResult {
    pub run_id: String,
    pub suite: Suite,
    pub score: Score,
    pub launch_error: bool,
    /// The arguments changed since that run (spec 7.5, «устарел»).
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyRow {
    /// `zapret2:<name>`, or `zapret2:<file>` for a file of the folder that is not a strategy.
    pub name: String,
    /// A strategy of the user's.
    pub custom: bool,
    pub active: bool,
    /// Not readable, named like a built-in one, or not passing the static check of zapret2.
    pub error: Option<String>,
    pub last: Option<LastResult>,
    /// A built-in copy of a Flowseal strategy on zapret2: not run by default (the CLI and the Tests
    /// tab leave it out unless named).
    pub flowseal_copy: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyDetail {
    pub row: StrategyRow,
    pub wf: String,
    pub blocks: Vec<String>,
    /// The command line winws2 gets.
    pub final_args: Option<String>,
    /// Every check of the last run the strategy was in.
    pub result: Option<StrategyResult>,
    /// The strategy's text as written, for its editor.
    pub source: String,
    /// The strategy's profiles, for its circular ones.
    pub circular: zapret2::circular::Analysis,
}

/// zapret2's strategies (with `engine2`); `active` is the registry's strategy name.
pub fn rows(
    engine2: Option<&Engine>,
    data_dir: &Path,
    active: Option<&str>,
) -> anyhow::Result<Vec<StrategyRow>> {
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let mut rows: Vec<StrategyRow> = Vec::new();
    if let Some(engine) = engine2 {
        for strategy in zapret2::strategies::list(engine.root())? {
            rows.push(zapret2_row(engine, &latest, active, &strategy).0);
        }
        // A file dropped into the folder that is not a strategy shows why, not nothing.
        for (file, problem) in zapret2::strategies::skipped(engine.root()) {
            rows.push(StrategyRow {
                name: format!("{}{file}", zapret2::PREFIX),
                custom: true,
                active: false,
                error: Some(problem),
                last: None,
                flowseal_copy: false,
            });
        }
    }
    Ok(rows)
}

/// A strategy's card, by its `zapret2:` name (zapret2 spec 3.1).
pub fn detail(
    engine2: Option<&Engine>,
    data_dir: &Path,
    active: Option<&str>,
    name: &str,
) -> anyhow::Result<StrategyDetail> {
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let result = latest
        .get(&name.to_lowercase())
        .map(|latest| latest.result.clone());
    let Some(name) = zapret2::strip_prefix(name) else {
        anyhow::bail!("no zapret2 strategy named {name}");
    };
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    let strategy = zapret2::strategies::find(engine.root(), name)?;
    let args = zapret2::strategies::args(&strategy.text);
    let (wf, blocks) = zapret2::strategies::split(&args);
    let (row, tail) = zapret2_row(engine, &latest, active, &strategy);
    Ok(StrategyDetail {
        row,
        wf,
        blocks,
        final_args: tail,
        result,
        source: strategy.text,
        circular: zapret2::circular::analyze(&args),
    })
}

/// The row and, when the placeholders could be filled, the command line.
fn zapret2_row(
    engine: &Engine,
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    strategy: &Strategy,
) -> (StrategyRow, Option<String>) {
    let tail = zapret2::strategies::tail(engine, &strategy.text);
    let sha256 = match &tail {
        Ok(tail) => {
            zapret2::strategies::check(engine, tail).map(|()| zapret::sha256_hex(tail.as_bytes()))
        }
        Err(err) => Err(anyhow::anyhow!("{err:#}")),
    };
    let mut row = row(
        latest,
        active,
        strategy.full_name(),
        !strategy.builtin,
        sha256,
    );
    row.flowseal_copy = strategy.flowseal_copy;
    (row, tail.ok())
}

/// `sha256` hashes the arguments the strategy would run with now, or says why it cannot run.
fn row(
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    name: String,
    custom: bool,
    sha256: anyhow::Result<String>,
) -> StrategyRow {
    let last = latest.get(&name.to_lowercase()).map(|latest| LastResult {
        run_id: latest.run_id.clone(),
        suite: latest.suite,
        score: latest.result.score.clone(),
        launch_error: latest.result.launch_error.is_some(),
        stale: sha256
            .as_ref()
            .is_ok_and(|sha256| *sha256 != latest.result.args_sha256),
    });
    StrategyRow {
        active: active.is_some_and(|active| active.eq_ignore_ascii_case(&name)),
        custom,
        error: sha256.err().map(|err| format!("{err:#}")),
        last,
        name,
        flowseal_copy: false,
    }
}

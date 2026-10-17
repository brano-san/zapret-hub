//! The rows and the detail pane of the «Стратегии» tab.

use std::collections::HashMap;
use std::path::Path;

use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;
use zapret::strategies::StrategyFile;
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
    /// File stem: `general (ALT11)`, `general (HUB mine)`; `zapret2:<name>` for zapret2.
    pub name: String,
    /// A `HUB` file with a record in `data/strategies` (spec 5.2), or a zapret2 strategy of the
    /// user's.
    pub custom: bool,
    pub active: bool,
    /// Not a strategy (`service.bat` would capture nothing), not readable, or not passing the
    /// static check of zapret2.
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
    /// `Final args` as `service.bat` prints them.
    pub final_args: Option<String>,
    /// Every check of the last run the strategy was in.
    pub result: Option<StrategyResult>,
    /// The record behind a custom strategy, for the editor.
    pub record: Option<CustomStrategy>,
    /// A zapret2 strategy's text as written, for its editor; empty for Flowseal's.
    pub source: String,
    /// A zapret2 strategy's profiles, for its circular ones; empty for Flowseal's.
    pub circular: zapret2::circular::Analysis,
}

/// Every strategy file in `service.bat`'s order (with a Flowseal `folder`), then zapret2's (with
/// `engine2`); `active` is the registry's strategy name.
pub fn rows(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
) -> anyhow::Result<Vec<StrategyRow>> {
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let mut rows: Vec<StrategyRow> = Vec::new();
    if let Some(folder) = folder {
        for file in zapret::strategies::list(folder)? {
            rows.push(flowseal_row(folder, &records, &latest, active, &file));
        }
    }
    if let Some(engine) = engine2 {
        for strategy in zapret2::strategies::list(engine.root())? {
            rows.push(zapret2_row(engine, folder, &latest, active, &strategy).0);
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

/// Flowseal's or zapret2's card, by the name's prefix (zapret2 spec 3.1).
pub fn detail(
    folder: Option<&Folder>,
    engine2: Option<&Engine>,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
    name: &str,
) -> anyhow::Result<StrategyDetail> {
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let result = latest
        .get(&name.to_lowercase())
        .map(|latest| latest.result.clone());
    if let Some(name) = zapret2::strip_prefix(name) {
        let engine = engine2.ok_or_else(|| {
            anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings")
        })?;
        let strategy = zapret2::strategies::find(engine.root(), name)?;
        let args = zapret2::strategies::args(&strategy.text);
        let (wf, blocks) = zapret2::strategies::split(&args);
        let (row, tail) = zapret2_row(engine, folder, &latest, active, &strategy);
        return Ok(StrategyDetail {
            row,
            wf,
            blocks,
            final_args: tail,
            result,
            record: None,
            source: strategy.text,
            circular: zapret2::circular::analyze(&args),
        });
    }
    let folder = folder.ok_or_else(|| anyhow::anyhow!(zapret::folder::NOT_SET))?;
    let file = zapret::strategies::find(folder, name)?;
    let records = store.list()?;
    let row = flowseal_row(folder, &records, &latest, active, &file);
    let (wf, blocks) =
        zapret::custom::split_bat(&zapret::read_text(&file.path)?).unwrap_or_default();
    Ok(StrategyDetail {
        row,
        wf,
        blocks,
        final_args: zapret::batparse::parse_file(folder, &file.path)
            .ok()
            .map(|parsed| parsed.final_args),
        result,
        record: records
            .into_iter()
            .find(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
        source: String::new(),
        circular: Default::default(),
    })
}

fn flowseal_row(
    folder: &Folder,
    records: &[CustomStrategy],
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    file: &StrategyFile,
) -> StrategyRow {
    let custom = records
        .iter()
        .any(|r| r.file_stem().eq_ignore_ascii_case(&file.name));
    let sha256 = zapret::batparse::parse_file(folder, &file.path).map(|parsed| parsed.sha256());
    row(latest, active, file.name.clone(), custom, sha256)
}

/// The row and, when the placeholders could be filled, the command line.
fn zapret2_row(
    engine: &Engine,
    folder: Option<&Folder>,
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    strategy: &Strategy,
) -> (StrategyRow, Option<String>) {
    let tail = zapret2::strategies::tail(engine, folder, &strategy.text);
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

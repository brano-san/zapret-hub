//! The rows and the detail pane of the «Стратегии» tab.

use std::collections::HashMap;
use std::path::Path;

use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;
use zapret::strategies::StrategyFile;

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
    /// File stem: `general (ALT11)`, `general (HUB mine)`.
    pub name: String,
    /// A `HUB` file with a record in `data/strategies` (spec 5.2).
    pub custom: bool,
    pub active: bool,
    /// Not a strategy (`service.bat` would capture nothing) or not readable.
    pub error: Option<String>,
    pub last: Option<LastResult>,
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
}

/// Every strategy file in `service.bat`'s order; `active` is the registry's strategy name.
pub fn rows(
    folder: &Folder,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
) -> anyhow::Result<Vec<StrategyRow>> {
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    Ok(zapret::strategies::list(folder)?
        .iter()
        .map(|file| row(folder, &records, &latest, active, file))
        .collect())
}

pub fn detail(
    folder: &Folder,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
    name: &str,
) -> anyhow::Result<StrategyDetail> {
    let file = zapret::strategies::find(folder, name)?;
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let row = row(folder, &records, &latest, active, &file);
    let (wf, blocks) =
        zapret::custom::split_bat(&zapret::read_text(&file.path)?).unwrap_or_default();
    Ok(StrategyDetail {
        row,
        wf,
        blocks,
        final_args: zapret::batparse::parse_file(folder, &file.path)
            .ok()
            .map(|parsed| parsed.final_args),
        result: latest
            .get(&file.name.to_lowercase())
            .map(|latest| latest.result.clone()),
        record: records
            .into_iter()
            .find(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
    })
}

fn row(
    folder: &Folder,
    records: &[CustomStrategy],
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    file: &StrategyFile,
) -> StrategyRow {
    let parsed = zapret::batparse::parse_file(folder, &file.path);
    let last = latest
        .get(&file.name.to_lowercase())
        .map(|latest| LastResult {
            run_id: latest.run_id.clone(),
            suite: latest.suite,
            score: latest.result.score.clone(),
            launch_error: latest.result.launch_error.is_some(),
            stale: parsed
                .as_ref()
                .is_ok_and(|parsed| parsed.sha256() != latest.result.args_sha256),
        });
    StrategyRow {
        name: file.name.clone(),
        custom: records
            .iter()
            .any(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
        active: active.is_some_and(|active| active.eq_ignore_ascii_case(&file.name)),
        error: parsed.err().map(|err| format!("{err:#}")),
        last,
    }
}

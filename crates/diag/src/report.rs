//! Saved results: `data\diag\<id>.json`, the newest 20 kept (spec 9).

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

use serde_json::Value;

use router::observe::FlowCounters;

use crate::culprits::Group;
use crate::summary::{GroupSummary, Summary};

const KEEP: usize = 20;

pub fn dir(data: &Path) -> PathBuf {
    data.join("diag")
}

/// `YYYY-MM-DD_HH-MM-SS`: also keeps an id from the CLI out of other folders.
fn is_id(s: &str) -> bool {
    s.len() == 19
        && s.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 13 | 16 => b == b'-',
            10 => b == b'_',
            _ => b.is_ascii_digit(),
        })
}

/// Report ids, newest first (ids sort as dates do).
fn ids(dir: &Path) -> anyhow::Result<Vec<String>> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| dir.display().to_string()),
    };
    let mut ids: Vec<String> = rd
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
        .filter(|n| is_id(n))
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    Ok(ids)
}

pub fn save(dir: &Path, summary: &Summary) -> anyhow::Result<()> {
    ensure!(is_id(&summary.id), "неверный номер отчёта: {}", summary.id);
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let path = dir.join(format!("{}.json", summary.id));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(summary)?)
        .with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, &path).with_context(|| path.display().to_string())?;
    // Best effort: a report an editor or antivirus holds stays until the next save.
    let olds = ids(dir).unwrap_or_else(|err| {
        log::warn!("old diagnostics reports are kept: {err:#}");
        Vec::new()
    });
    for old in olds.into_iter().skip(KEEP).filter(|old| *old != summary.id) {
        let path = dir.join(format!("{old}.json"));
        if let Err(err) = std::fs::remove_file(&path) {
            log::warn!("cannot remove the old report {}: {err}", path.display());
        }
    }
    Ok(())
}

/// `(id, profile)`, newest first; unreadable reports are left out.
pub fn list(dir: &Path) -> anyhow::Result<Vec<(String, String)>> {
    Ok(ids(dir)?
        .into_iter()
        .filter_map(|id| {
            let profile = load(dir, &id).ok()?.profile;
            Some((id, profile))
        })
        .collect())
}

pub fn load(dir: &Path, id: &str) -> anyhow::Result<Summary> {
    ensure!(is_id(id), "неверный номер отчёта: {id}");
    let path = dir.join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let mut value: Value =
        serde_json::from_str(&text).with_context(|| path.display().to_string())?;
    fill_defaults(&mut value)?;
    serde_json::from_value(value).with_context(|| path.display().to_string())
}

/// The fields a report from another version lacks, from `Default`. Not `#[serde(default)]`:
/// specta would make every field optional in the page's types.
pub fn fill_defaults(report: &mut Value) -> anyhow::Result<()> {
    fn fill(value: &mut Value, defaults: Value) {
        if let (Value::Object(map), Value::Object(defaults)) = (value, defaults) {
            for (key, default) in defaults {
                map.entry(key).or_insert(default);
            }
        }
    }
    fill(report, serde_json::to_value(Summary::default())?);
    if let Some(agent) = report.get_mut("agent") {
        fill(
            agent,
            serde_json::to_value(crate::agent::AgentView::default())?,
        );
    }
    let summary = serde_json::to_value(GroupSummary::default())?;
    let group = serde_json::to_value(Group::default())?;
    let flow = serde_json::to_value(FlowCounters::default())?;
    if let Some(Value::Array(groups)) = report.get_mut("groups") {
        for g in groups {
            fill(g, summary.clone());
            let Some(g) = g.get_mut("group") else {
                continue;
            };
            fill(g, group.clone());
            for list in ["members", "representatives"] {
                if let Some(Value::Array(flows)) = g.get_mut(list) {
                    for f in flows {
                        fill(f, flow.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_of_0_8_0_with_flowseals_fields_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let id = "2026-10-20_10-00-00";
        let mut value = serde_json::to_value(Summary {
            id: id.into(),
            ..Summary::default()
        })
        .unwrap();
        let context = &mut value["agent"]["context"];
        context["flowseal_outdated"] = Value::Bool(true);
        context["flowseal_version"] = Value::String("1.10.3".into());
        std::fs::write(dir.path().join(format!("{id}.json")), value.to_string()).unwrap();
        let summary = load(dir.path(), id).unwrap();
        assert_eq!(summary.id, id);
    }
}

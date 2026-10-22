//! «Применить» (diag spec 8) and the `hub-circular` text (diag spec 7).

use anyhow::Context as _;
use diag::circular_store::{self, Store};
use diag::report;
use diag::summary::{self, Fix};
use router::rules::{self, Rule};
use tauri::AppHandle;
use zapret::service::QuickCheck;

use super::circular_path;
use crate::actions::{self, state};
use crate::{routing, texts};

/// The line `hub-circular` keeps the strategy it was built on in.
pub(super) const BASE_LINE: &str = "# base: ";

/// `hub-circular` from `store` on the zapret2 strategy in use (diag spec 7), the base named on its
/// first line.
pub(super) fn hub_text(engine: &zapret2::Engine, store: &Store) -> anyhow::Result<String> {
    let (base, text) = base(engine, zapret::service::strategy().as_deref());
    let generated = circular_store::generate(&text, store)?;
    Ok(format!("{BASE_LINE}{base}\n{generated}"))
}

/// The zapret2 strategy `hub-circular` builds on: the active zapret2 one; for an active Flowseal
/// strategy its copy (`general` is `general (Flowseal)`); for `hub-circular` itself the base it
/// names. None (empty) for the user's own Flowseal strategy or nothing active.
pub(super) fn base(engine: &zapret2::Engine, active: Option<&str>) -> (String, String) {
    let Some(active) = active else {
        return Default::default();
    };
    let (name, zapret2) = match zapret2::strip_prefix(active) {
        Some(circular_store::NAME) => {
            let named = zapret2::strategies::find(engine.root(), circular_store::NAME)
                .ok()
                .and_then(|s| {
                    s.text
                        .lines()
                        .find_map(|l| l.strip_prefix(BASE_LINE).map(|n| n.trim().to_owned()))
                })
                .filter(|n| !n.is_empty() && n != circular_store::NAME);
            match named {
                Some(name) => (name, true),
                None => return Default::default(),
            }
        }
        Some(short) => (short.to_owned(), true),
        None if active == "general" => ("general (Flowseal)".to_owned(), false),
        None => (active.to_owned(), false),
    };
    match zapret2::strategies::find(engine.root(), &name) {
        Ok(s) if zapret2 || s.flowseal_copy => (name, s.text),
        _ => Default::default(),
    }
}

/// «Применить» of a saved report's fix (diag spec 8). `None` for a rule: no quick check.
pub fn apply(app: &AppHandle, id: &str, fix: u32) -> anyhow::Result<Option<QuickCheck>> {
    let state = state(app);
    state.not_diagnosing()?;
    let summary = report::load(&state.paths.diag(), id)?;
    let fix = summary
        .fixes
        .get(fix as usize)
        .with_context(|| format!("report {id} has no fix {fix}"))?
        .clone();
    match fix {
        Fix::AddRule { rule } => {
            state.router.edit(|list| {
                summary::add_rule(list, rule);
                Ok(())
            })?;
            let enabled = routing::set_enabled(app, true);
            actions::changed(app);
            enabled.map(|_| None)
        }
        Fix::EditRule { name, protocol } => {
            state.router.edit(|list| {
                let rule = Rule {
                    protocol,
                    ..rules::find(list, &name)?.clone()
                };
                rules::replace(list, rule)
            })?;
            actions::changed(app);
            Ok(None)
        }
        Fix::ApplyStrategy { name } => actions::apply(app, &name).map(Some),
        Fix::ApplyHubCircular { tricks, filter } => {
            let engine = state.engine2_required()?;
            let path = circular_path(&state);
            state.change_any(|folder| {
                let mut store = circular_store::load(&path)?;
                circular_store::add(&mut store, &filter, tricks);
                let text = hub_text(&engine, &store)?;
                let create = !zapret2::strategies::dir(engine.root())
                    .join(format!("{}.txt", circular_store::NAME))
                    .exists();
                zapret2::service::save(
                    &engine,
                    folder,
                    engine.root(),
                    circular_store::NAME,
                    &text,
                    create,
                )?;
                // After the strategy: a text zapret2 refuses leaves the store as it was.
                circular_store::save(&path, &store)
            })?;
            let name = format!("{}{}", zapret2::PREFIX, circular_store::NAME);
            actions::apply(app, &name).map(Some)
        }
        Fix::TurnOff { .. } | Fix::Manual { .. } => anyhow::bail!(texts::MANUAL),
    }
}

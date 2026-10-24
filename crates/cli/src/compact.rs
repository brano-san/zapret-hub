//! The compact report an agent reads: `diag run --wait` and `diag show` print it; `--full` prints
//! the raw `Summary` (agent proposal 2). Pure: no I/O. Empty and default fields are left out.

use diag::agent::{AgentView, ClientState, Context, Observed};
use diag::evaluate::works;
use diag::next_steps::apply_cli;
use diag::paths::PathKind;
use diag::summary::{Fix, Summary};
use serde::Serialize;
use serde_json::{Map, Value, json};
use zapret::checks::CheckLevel;

/// `UdpNoReply` -> `udp_no_reply`.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A serde-tagged value as `(snake tag, the other fields)`.
fn tagged<T: Serialize>(v: &T, tag: &str) -> (String, Value) {
    let mut map = match serde_json::to_value(v) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let kind = map.remove(tag).and_then(|k| k.as_str().map(snake));
    (kind.unwrap_or_default(), Value::Object(map))
}

/// `v` without the fields equal to those of `T::default()`; `Null` when nothing is left.
fn minus_default<T: Serialize + Default>(v: &T) -> Value {
    let (Ok(Value::Object(mut map)), Ok(Value::Object(def))) =
        (serde_json::to_value(v), serde_json::to_value(T::default()))
    else {
        return Value::Null;
    };
    map.retain(|k, x| def.get(k) != Some(x));
    if map.is_empty() {
        Value::Null
    } else {
        Value::Object(map)
    }
}

fn source_code(name: &str) -> String {
    match name {
        "журнал NekoBox" => "neko_log".into(),
        "маршруты NekoBox" => "neko_routes".into(),
        "журнал Discord" => "discord_log".into(),
        other => other.into(),
    }
}

/// What the fix is about: warnings that name it belong to it.
fn subject(fix: &Fix) -> Option<&str> {
    match fix {
        Fix::AddRule { rule } => Some(&rule.name),
        Fix::EditRule { name, .. } | Fix::ApplyStrategy { name } => Some(name),
        _ => None,
    }
}

fn fix_text(fix: &Fix) -> String {
    match fix {
        Fix::AddRule { rule } => format!("добавить правило «{}»", rule.name),
        Fix::EditRule { name, protocol } => {
            format!("расширить правило «{name}»: {protocol:?}")
        }
        Fix::ApplyStrategy { name } => format!("применить стратегию {name}"),
        Fix::ApplyHubCircular { tricks, .. } => {
            format!("применить приёмы хаба: {}", tricks.len())
        }
        Fix::TurnOff { text, .. } | Fix::Manual { text } => text.clone(),
    }
}

fn path_source(p: &PathKind) -> Option<String> {
    match p {
        PathKind::Strategy { name } => Some(name.clone()),
        PathKind::Trick { candidate } => {
            Some(format!("{}#{}", candidate.source, candidate.profile + 1))
        }
        PathKind::ProxyPoint { rule } | PathKind::ProxyNoZapret { rule } => Some(rule.name.clone()),
        PathKind::ExtendRule { rule, .. } => Some(rule.clone()),
        PathKind::Current | PathKind::Direct => None,
    }
}

pub fn compact(s: &Summary) -> Value {
    let a: &AgentView = &s.agent;
    let status = if s.canceled {
        "canceled"
    } else if s.stopped {
        "stopped"
    } else if s.budget_hit {
        "budget_hit"
    } else {
        "done"
    };
    let tried = |g: &diag::summary::GroupSummary| {
        g.results
            .iter()
            .filter(|(_, r)| r.total > 0 || r.answer.is_some())
            .count()
    };
    let working =
        |g: &diag::summary::GroupSummary| g.results.iter().filter(|(_, r)| works(r)).count();
    let culprits: Vec<Value> = s
        .groups
        .iter()
        .map(|g| {
            let (code, args) = tagged(&g.group.fault, "kind");
            let mut c = json!({
                "key": g.group.key,
                "proto": g.group.protocol,
                "ports": g.group.ports,
                "fault": { "code": code, "args": args },
                "tried": tried(g),
                "working": working(g),
            });
            if let Some(cov) = a.coverage.iter().find(|c| c.key == g.group.key) {
                c["route"] = json!(cov.route);
            }
            c
        })
        .collect();
    let best_paths: Vec<Value> = s
        .groups
        .iter()
        .filter_map(|g| {
            let (path, score) = g.results.get(g.best? as usize)?;
            let mut b = json!({
                "culprit": g.group.key,
                "path": tagged(path, "kind").0,
                "ok": format!("{}/{}", score.ok, score.total),
                "median_ms": score.median_ms,
                "tried": tried(g),
                "working": working(g),
            });
            if let Some(src) = path_source(path) {
                b["source"] = json!(src);
            }
            Some(b)
        })
        .collect();
    let mut used = vec![false; s.warnings.len()];
    let fixes: Vec<Value> = s
        .fixes
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut warnings = Vec::new();
            for (j, w) in s.warnings.iter().enumerate() {
                if subject(f).is_some_and(|n| w.contains(n)) {
                    used[j] = true;
                    warnings.push(w);
                }
            }
            json!({
                "i": i,
                "kind": tagged(f, "kind").0,
                "text": fix_text(f),
                "cli": apply_cli(&s.id, i, f),
                "warnings": warnings,
            })
        })
        .collect();
    let rest: Vec<&String> = s
        .warnings
        .iter()
        .zip(&used)
        .filter_map(|(w, u)| (!u).then_some(w))
        .collect();
    let checks: Vec<Value> = s
        .system_checks
        .iter()
        .filter(|c| c.level != CheckLevel::Ok)
        .map(|c| {
            json!({
                "id": format!("{:?}", c.id),
                "level": format!("{:?}", c.level).to_lowercase(),
                "detail": c.detail,
            })
        })
        .collect();
    let mut out = json!({
        "report_id": s.id,
        "profile": s.profile,
        "status": status,
        "duration_s": a.duration_s,
        "verdict": { "code": a.verdict, "text": s.verdict_text },
        "confidence": a.confidence,
        "culprits": culprits,
        "best_paths": best_paths,
        "fixes": fixes,
        "evidence": a.evidence,
        "checks": checks,
        "not_checked": a.not_checked,
        "missing": s.sources_missing.iter().map(|m| source_code(m)).collect::<Vec<_>>(),
        "coverage": a.coverage,
        "changes": a.changes,
        "warnings": rest,
        "rollback_error": s.rollback_error,
        "next_steps": a.next_steps,
    });
    out["client"] = minus_default::<ClientState>(&a.client);
    out["observed"] = minus_default::<Observed>(&a.observed);
    out["context"] = minus_default::<Context>(&a.context);
    // Missing or empty fields are simply absent.
    if let Value::Object(map) = &mut out {
        map.retain(|k, v| {
            k == "verdict"
                || !(v.is_null()
                    || v.as_str() == Some("")
                    || v.as_array().is_some_and(Vec::is_empty))
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use diag::agent::VerdictCode;

    fn load(text: &str) -> Summary {
        let mut v: Value = serde_json::from_str(text).unwrap();
        diag::report::fill_defaults(&mut v).unwrap();
        serde_json::from_value(v).unwrap()
    }

    fn size(v: &Value) -> usize {
        serde_json::to_string(v).unwrap().len()
    }

    #[test]
    fn broken_run_shrinks_under_4_kb() {
        let text = include_str!("../tests/fixtures/diag-broken-old.json");
        assert!(text.len() > 38_000);
        let out = compact(&load(text));
        assert!(size(&out) < 4096, "{} bytes: {out}", size(&out));
        assert_eq!(out["report_id"], "2026-10-24_15-29-04");
        assert_eq!(out["status"], "done");
        let c = &out["culprits"][0];
        assert_eq!(c["key"], "203.0.113.0/24");
        assert_eq!(c["proto"], "udp");
        assert_eq!(c["fault"]["code"], "udp_no_reply");
        assert_eq!(c["fault"]["args"]["up"], 285);
        assert_eq!(c["working"], 0);
        assert!(out["best_paths"].is_null());
        // Manual: no command, the original index.
        assert_eq!(out["fixes"][0]["i"], 0);
        assert_eq!(out["fixes"][0]["kind"], "manual");
        assert!(out["fixes"][0]["cli"].is_null());
        assert!(!out.to_string().contains("--wf-"));
    }

    #[test]
    fn clean_run_keeps_only_the_warning_check() {
        let out = compact(&load(include_str!("../tests/fixtures/diag-clean.json")));
        assert_eq!(out["checks"].as_array().unwrap().len(), 1);
        assert_eq!(out["checks"][0]["id"], "Proxy");
        assert_eq!(out["missing"], json!(["neko_log", "neko_routes"]));
        // An old report has an empty agent part: nothing of it is printed.
        assert!(out.get("client").is_none() && out.get("context").is_none());
        assert!(out.get("evidence").is_none() && out.get("next_steps").is_none());
    }

    #[test]
    fn fix_cli_keeps_the_index_and_warnings_follow_their_fix() {
        let mut s = Summary {
            id: "2026-10-24_18-38-46".into(),
            warnings: vec!["сломает: YouTube при ALT2".into(), "прочее".into()],
            fixes: vec![
                Fix::Manual { text: "x".into() },
                Fix::ApplyStrategy {
                    name: "ALT2".into(),
                },
            ],
            ..Default::default()
        };
        s.agent.verdict = VerdictCode::CulpritsFixed;
        let out = compact(&s);
        assert!(out["fixes"][0]["cli"].is_null());
        assert_eq!(
            out["fixes"][1]["cli"],
            "zapret-hub-cli diag apply 2026-10-24_18-38-46 --fix 1 --yes"
        );
        assert_eq!(
            out["fixes"][1]["warnings"],
            json!(["сломает: YouTube при ALT2"])
        );
        assert_eq!(out["warnings"], json!(["прочее"]));
        assert_eq!(out["verdict"]["code"], "culprits_fixed");
    }
}

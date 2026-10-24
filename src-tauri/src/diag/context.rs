//! The agent's view of the setup the run happened in (agent proposal 1): context, coverage per
//! culprit and the changes since the last report of the profile.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::SystemTime;

use diag::coverage::{self, Input};
use diag::paths::{Ctx, routed_by};
use diag::summary::Summary;
use diag::{compare, report};
use router::RouterState;
use zapret::folder::Folder;

use super::args_of;
use super::evidence::egress_ip;
use crate::state::AppState;

const HUB_ERRORS: usize = 5;
const LINE_CHARS: usize = 200;

/// Lines of the hub's journal at WARN/ERROR, newest first: `files` are `(date, text)` newest
/// first. A journal line is `HH:MM:SS LEVEL [target] text`.
pub(super) fn recent_errors(files: &[(String, String)], max: usize) -> Vec<String> {
    files
        .iter()
        .flat_map(|(date, text)| text.lines().rev().map(move |l| (date, l)))
        .filter(|(_, l)| matches!(l.split_whitespace().nth(1), Some("WARN" | "ERROR")))
        .take(max)
        .map(|(date, l)| format!("{date} {l}").chars().take(LINE_CHARS).collect())
        .collect()
}

/// Today's and yesterday's journal files: the two newest by name.
fn journal_files(dir: &Path) -> Vec<(String, String)> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".log"))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
        .into_iter()
        .take(2)
        .filter_map(|n| {
            let text = std::fs::read_to_string(dir.join(&n)).ok()?;
            Some((n.trim_end_matches(".log").to_owned(), text))
        })
        .collect()
}

/// 1-based place of `strategy` in the run, and the run's best (the first that started).
pub(super) fn rank_in(
    run: &tester::model::TestRun,
    strategy: Option<&str>,
) -> (Option<u32>, Option<String>) {
    let started = || run.strategies.iter().filter(|s| s.launch_error.is_none());
    let rank = strategy.and_then(|n| started().position(|s| s.name == n));
    (
        rank.map(|i| i as u32 + 1),
        started().next().map(|s| s.name.clone()),
    )
}

fn lines_of(path: &Path) -> Option<Vec<String>> {
    Some(
        std::fs::read_to_string(path)
            .ok()?
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

fn age_days(path: &Path) -> Option<u32> {
    let age = SystemTime::now().duration_since(path.metadata().ok()?.modified().ok()?);
    Some((age.ok()?.as_secs() / 86_400) as u32)
}

fn v4(dst: &str) -> Option<Ipv4Addr> {
    match dst.parse() {
        Ok(SocketAddr::V4(a)) => Some(*a.ip()),
        _ => None,
    }
}

/// Fills `summary.agent.{context, coverage, changes}`; runs after `summarize`.
pub(super) fn fill(
    state: &AppState,
    ctx: &Ctx,
    folder: Option<&Folder>,
    engine2: Option<&zapret2::Engine>,
    summary: &mut Summary,
) {
    let (service, image) = zapret::service::active();
    let engine_running = zapret::procs::is_running(image);
    let engine = engine_running.then_some(if service == zapret::service::SERVICE2 {
        "zapret2"
    } else {
        "zapret"
    });
    let strategy = zapret::service::strategy();
    let socks5 = state.socks5();
    let ranking = tester::history::latest_ranking(&tester::history::dir(&state.paths.data()))
        .ok()
        .flatten();
    let (tests_rank, tests_best) = ranking
        .as_ref()
        .map_or((None, None), |run| rank_in(run, strategy.as_deref()));
    let routing = match state.router.status().state {
        RouterState::Running => "enabled",
        RouterState::Paused => "paused",
        _ => "off",
    };
    summary.agent.context = diag::agent::Context {
        strategy: strategy.clone(),
        engine_running,
        flowseal_outdated: state.update().is_some(),
        flowseal_version: folder.and_then(Folder::version),
        routing: routing.into(),
        proxy: socks5.clone(),
        proxy_alive: socks5.as_ref().map(|_| ctx.proxy_ok),
        proxy_udp: None,
        egress_ip_direct: egress_ip(None),
        egress_ip_proxy: socks5
            .as_deref()
            .filter(|_| ctx.proxy_ok)
            .and_then(|s| egress_ip(Some(s))),
        tests_rank,
        tests_best,
        hub_errors: recent_errors(&journal_files(&state.paths.logs()), HUB_ERRORS),
    };
    // ponytail: the lists are Flowseal's; zapret2 keeps its own, so only the zapret route has them.
    let args = strategy
        .as_deref()
        .and_then(|n| args_of(folder, engine2, n).ok());
    let lists = folder.filter(|_| engine == Some("zapret")).map(|f| {
        let dir = f.lists();
        let (ipset, general) = (dir.join("ipset-all.txt"), dir.join("list-general.txt"));
        let mut host = lines_of(&general);
        if let (Some(h), Some(user)) = (host.as_mut(), lines_of(&dir.join("list-general-user.txt")))
        {
            h.extend(user);
        }
        (lines_of(&ipset), host, age_days(&ipset), age_days(&general))
    });
    summary.agent.coverage = summary
        .groups
        .iter()
        .map(|g| {
            let g = &g.group;
            let subnet = g.key.contains('/');
            let hosts: Vec<String> = (!subnet)
                .then(|| g.key.clone())
                .into_iter()
                .chain(
                    g.members
                        .iter()
                        .flat_map(|f| [f.sni.clone(), f.domain.clone()])
                        .flatten(),
                )
                .collect();
            let ips: Vec<Ipv4Addr> = g
                .representatives
                .iter()
                .filter_map(|f| v4(&f.dst))
                .collect();
            let by_system = g.members.iter().any(|f| f.via_system_proxy);
            coverage::coverage(&Input {
                key: &g.key,
                route: coverage::route(routed_by(g, ctx).as_deref(), by_system, engine),
                protocol: g.protocol,
                ports: &g.ports,
                ips: &ips,
                hosts: &hosts,
                args: args.as_deref(),
                ipset: lists.as_ref().and_then(|l| l.0.as_deref()),
                hostlist: lists.as_ref().and_then(|l| l.1.as_deref()),
                list_age_days: lists.as_ref().and_then(|l| if subnet { l.2 } else { l.3 }),
            })
        })
        .collect();
    let dir = state.paths.diag();
    let previous = report::list(&dir).ok().and_then(|all| {
        let id = all
            .into_iter()
            .find(|(id, profile)| *profile == summary.profile && *id != summary.id)?
            .0;
        report::load(&dir, &id).ok()
    });
    if let Some(prev) = previous {
        summary.agent.changes = compare::changes(&prev, summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_newest_first_cut_and_capped() {
        let today =
            "10:00:00 INFO  [a] fine\n10:00:01 WARN  [a] first\n10:00:02 ERROR [b] second\n";
        let long = format!("09:00:00 ERROR [c] {}", "x".repeat(500));
        let files = vec![
            ("2026-10-24".to_owned(), today.to_owned()),
            ("2026-10-23".to_owned(), long),
        ];
        let out = recent_errors(&files, 3);
        assert!(out[0].contains("second") && out[1].contains("first"));
        assert_eq!(out[2].chars().count(), 200);
        assert_eq!(recent_errors(&files, 1).len(), 1);
        assert!(recent_errors(&[], 5).is_empty());
    }

    #[test]
    fn rank_skips_strategies_that_did_not_start() {
        let mut run: tester::model::TestRun = serde_json::from_str(
            r#"{"id":"x","suite":"Standard","flowseal_version":null,"canceled":false,
            "restore_error":null,"strategies":[]}"#,
        )
        .unwrap();
        assert_eq!(rank_in(&run, Some("a")), (None, None));
        let mut s = |name: &str, err: bool| {
            let mut r: tester::model::StrategyResult = serde_json::from_str(&format!(
                r#"{{"name":"{name}","args_sha256":"","launch_error":{},"targets":[],"dpi":[],
                "score":{}}}"#,
                if err { r#""boom""# } else { "null" },
                serde_json::to_string(&tester::model::Score::default()).unwrap()
            ))
            .unwrap();
            r.name = name.into();
            run.strategies.push(r);
        };
        s("bad", true);
        s("best", false);
        s("cur", false);
        assert_eq!(rank_in(&run, Some("cur")), (Some(2), Some("best".into())));
    }
}

//! What an agent does next (agent proposal 2): exact commands and `ask_user:` lines, from a
//! finished `Summary`. Pure: the app calls `fill` after `summarize`.

use zapret::checks::{CheckAction, CheckLevel};

use crate::agent::VerdictCode;
use crate::profiles::{ANY, Profile};
use crate::summary::{Fix, Summary};

/// `zapret-hub-cli diag apply <id> --fix <i> --yes`; none for fixes the user does by hand.
pub fn apply_cli(id: &str, i: usize, fix: &Fix) -> Option<String> {
    match fix {
        Fix::Manual { .. } | Fix::TurnOff { .. } => None,
        _ => Some(format!("zapret-hub-cli diag apply {id} --fix {i} --yes")),
    }
}

/// The run's start command, for the check after a fix or after the user did what was asked.
pub fn rerun(profile: &Profile) -> String {
    match (profile.id.as_str(), profile.apps.first()) {
        (ANY, Some(app)) => format!("zapret-hub-cli diag run --app {app} --wait --timeout 590"),
        (id, _) => format!("zapret-hub-cli diag run --profile {id} --wait --timeout 590"),
    }
}

fn check_cli(action: CheckAction) -> String {
    let (name, yes) = match action {
        CheckAction::EnableTcpTimestamps => ("enable-tcp-timestamps", ""),
        CheckAction::RemoveWinDivert => ("remove-windivert", " --yes"),
        CheckAction::RemoveConflicts => ("remove-conflicts", " --yes"),
        CheckAction::GrantCapabilities => ("grant-capabilities", ""),
    };
    format!("zapret-hub-cli checks fix {name}{yes}")
}

pub fn steps(s: &Summary, profile: &Profile) -> Vec<String> {
    let mut out = Vec::new();
    let again = rerun(profile);
    let cli: Vec<String> = s
        .fixes
        .iter()
        .enumerate()
        .filter_map(|(i, f)| apply_cli(&s.id, i, f))
        .collect();
    let fixed = !cli.is_empty();
    out.extend(cli);
    let v = s.agent.verdict;
    let not_running = s.agent.client.app == "not_running";
    if !fixed {
        if not_running {
            out.push(format!(
                "ask_user: Запустите {}",
                profile.apps.join(" или ")
            ));
            out.push(again.clone());
        } else if matches!(
            v,
            VerdictCode::NoFlows | VerdictCode::NotReproducible | VerdictCode::LiveOnly
        ) {
            out.push(format!("ask_user: {}", profile.hint));
            out.push(again.clone());
        } else if v == VerdictCode::Inconclusive
            && s.agent.not_checked.iter().any(|f| f.code == "proxy_unseen")
        {
            out.push(format!(
                "ask_user: {} ходит через системный прокси, и хаб не видит этих соединений: \
                 выключите прокси для программы или добавьте правило «Маршруты», затем повторите",
                profile.name
            ));
            out.push(again.clone());
        }
    }
    for c in s.system_checks.iter().filter(|c| c.level != CheckLevel::Ok) {
        if let Some(a) = c.action {
            let line = check_cli(a);
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    if fixed {
        out.push(again);
    }
    out
}

/// Fills `agent.next_steps`.
pub fn fill(s: &mut Summary, profile: &Profile) {
    s.agent.next_steps = steps(s, profile);
}

/// The report of a run that ended before watching: none of the profile's programs runs.
pub fn not_running(profile: &Profile, id: String) -> Summary {
    let mut s = Summary {
        id,
        profile: profile.name.clone(),
        verdict_text: format!(
            "программа не запущена ({}): запустите её и повторите",
            profile.apps.join(", ")
        ),
        ..Default::default()
    };
    s.agent.verdict = VerdictCode::NotReproducible;
    s.agent.client.app = "not_running".into();
    fill(&mut s, profile);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use zapret::checks::{CheckId, SystemCheck};

    fn profile() -> Profile {
        crate::profiles::builtin().remove(0)
    }

    fn summary(v: VerdictCode) -> Summary {
        let mut s = Summary {
            id: "r1".into(),
            ..Default::default()
        };
        s.agent.verdict = v;
        s
    }

    #[test]
    fn fixes_then_verify_and_manual_has_none() {
        let mut s = summary(VerdictCode::CulpritsFixed);
        s.fixes = vec![
            Fix::Manual { text: "x".into() },
            Fix::ApplyStrategy {
                name: "ALT2".into(),
            },
        ];
        assert_eq!(
            steps(&s, &profile()),
            [
                "zapret-hub-cli diag apply r1 --fix 1 --yes",
                "zapret-hub-cli diag run --profile discord --wait --timeout 590"
            ]
        );
    }

    #[test]
    fn no_flows_asks_the_hint() {
        let out = steps(&summary(VerdictCode::NoFlows), &profile());
        assert_eq!(out[0], "ask_user: Зайдите в голосовой канал");
        assert!(out[1].contains("diag run --profile discord"));
    }

    #[test]
    fn traffic_through_the_system_proxy_asks_to_route_it_and_repeat() {
        let mut s = summary(VerdictCode::Inconclusive);
        s.agent.not_checked.push(crate::agent::Fact {
            code: "proxy_unseen".into(),
            args: vec![],
            text: String::new(),
        });
        let out = steps(&s, &profile());
        assert!(
            out[0].starts_with("ask_user: Discord ходит через системный прокси"),
            "{out:?}"
        );
        assert!(out[0].contains("«Маршруты»"), "{out:?}");
        assert!(out[1].contains("diag run --profile discord"), "{out:?}");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn failed_check_with_an_action() {
        let mut s = summary(VerdictCode::Clean);
        s.system_checks = vec![
            SystemCheck {
                id: CheckId::TcpTimestamps,
                level: CheckLevel::Warn,
                detail: String::new(),
                action: Some(CheckAction::EnableTcpTimestamps),
            },
            SystemCheck {
                id: CheckId::Bfe,
                level: CheckLevel::Ok,
                detail: String::new(),
                action: Some(CheckAction::RemoveWinDivert),
            },
        ];
        assert_eq!(
            steps(&s, &profile()),
            ["zapret-hub-cli checks fix enable-tcp-timestamps"]
        );
    }

    #[test]
    fn not_running_report_asks_to_start() {
        let s = not_running(&profile(), "r2".into());
        assert_eq!(s.agent.verdict, VerdictCode::NotReproducible);
        assert!(s.agent.next_steps[0].starts_with("ask_user: Запустите Discord.exe"));
        assert!(s.agent.next_steps[1].contains("--profile discord"));
    }

    #[test]
    fn any_program_reruns_by_app() {
        let p = crate::profiles::any_program("game.exe");
        assert!(rerun(&p).contains("--app game.exe"));
    }
}

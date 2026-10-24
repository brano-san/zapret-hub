//! What differs from the previous report of the same profile (agent proposal 1, `changes`).

use crate::agent::Fact;
use crate::summary::Summary;

fn fact(code: &str, args: &[&str], text: String) -> Fact {
    Fact {
        code: code.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        text,
    }
}

fn best(g: &crate::summary::GroupSummary) -> Option<String> {
    Some(g.results.get(g.best? as usize)?.0.label())
}

fn show(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("—")
}

/// Facts for what changed from `prev` to `now`; empty when nothing did.
pub fn changes(prev: &Summary, now: &Summary) -> Vec<Fact> {
    let (p, n) = (&prev.agent, &now.agent);
    let mut out = Vec::new();
    if p.context.strategy != n.context.strategy {
        let (a, b) = (show(&p.context.strategy), show(&n.context.strategy));
        out.push(fact(
            "strategy_changed",
            &[a, b],
            format!("Стратегия изменилась: {a} → {b}"),
        ));
    }
    if p.verdict != n.verdict {
        let (a, b) = (format!("{:?}", p.verdict), format!("{:?}", n.verdict));
        out.push(fact(
            "verdict_changed",
            &[&a, &b],
            format!("Вердикт изменился: {a} → {b}"),
        ));
    }
    for g in &now.groups {
        let key = &g.group.key;
        match prev.groups.iter().find(|o| o.group.key == *key) {
            None => out.push(fact(
                "culprit_new",
                &[key],
                format!("Новый виновник: {key}"),
            )),
            Some(o) if best(o) != best(g) => {
                let (a, b) = (show(&best(o)).to_owned(), show(&best(g)).to_owned());
                out.push(fact(
                    "best_path_changed",
                    &[key, &a, &b],
                    format!("Лучший путь для {key} изменился: {a} → {b}"),
                ));
            }
            Some(_) => {}
        }
    }
    for o in &prev.groups {
        let key = &o.group.key;
        if !now.groups.iter().any(|g| g.group.key == *key) {
            out.push(fact(
                "culprit_gone",
                &[key],
                format!("Виновника больше нет: {key}"),
            ));
        }
    }
    if p.context.flowseal_version != n.context.flowseal_version {
        let (a, b) = (
            show(&p.context.flowseal_version),
            show(&n.context.flowseal_version),
        );
        out.push(fact(
            "flowseal_changed",
            &[a, b],
            format!("Версия Flowseal изменилась: {a} → {b}"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::culprits::Group;
    use crate::paths::PathKind;
    use crate::summary::GroupSummary;

    fn group(key: &str, best_path: Option<&str>) -> GroupSummary {
        GroupSummary {
            group: Group {
                key: key.into(),
                ..Group::default()
            },
            results: best_path
                .map(|n| {
                    vec![(
                        PathKind::Strategy { name: n.into() },
                        crate::evaluate::Score::default(),
                    )]
                })
                .unwrap_or_default(),
            best: best_path.map(|_| 0),
            ..GroupSummary::default()
        }
    }

    fn codes(f: &[Fact]) -> Vec<&str> {
        f.iter().map(|f| f.code.as_str()).collect()
    }

    #[test]
    fn nothing_changed() {
        let s = Summary::default();
        assert!(changes(&s, &s).is_empty());
    }

    #[test]
    fn every_kind_of_change() {
        let mut a = Summary::default();
        a.agent.context.strategy = Some("general".into());
        a.agent.context.flowseal_version = Some("1.9.7".into());
        a.groups = vec![group("old.example", None), group("kept.example", Some("a"))];
        let mut b = Summary::default();
        b.agent.context.strategy = Some("alt".into());
        b.agent.context.flowseal_version = Some("1.9.8".into());
        b.agent.verdict = crate::agent::VerdictCode::Clean;
        b.groups = vec![group("new.example", None), group("kept.example", Some("b"))];
        let f = changes(&a, &b);
        assert_eq!(
            codes(&f),
            [
                "strategy_changed",
                "verdict_changed",
                "culprit_new",
                "best_path_changed",
                "culprit_gone",
                "flowseal_changed"
            ]
        );
        assert_eq!(f[3].args[0], "kept.example");
    }
}

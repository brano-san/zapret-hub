//! Which temporary configurations to try on a culprit, in order (spec section 6).

use router::rules::{Protocol, Rule};
use serde::{Deserialize, Serialize};

use crate::candidates::{Candidate, candidates, is_subnet};
use crate::cover::strategy_covers;
use crate::culprits::Group;
use crate::profiles::Profile;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum PathKind {
    Current,
    Direct,
    ProxyPoint { rule: Rule },
    ProxyNoZapret { rule: Rule },
    Strategy { name: String },
    Trick { candidate: Candidate },
    ExtendRule { rule: String, protocol: Protocol },
}

fn protocol_text(p: Protocol) -> &'static str {
    match p {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
        Protocol::Both => "TCP и UDP",
    }
}

impl PathKind {
    pub fn label(&self) -> String {
        match self {
            Self::Current => "Как сейчас".into(),
            Self::Direct => "Напрямую мимо всего".into(),
            Self::ProxyPoint { .. } => "Прокси точечно".into(),
            Self::ProxyNoZapret { .. } => "Прокси без zapret".into(),
            Self::Strategy { name } => format!("Стратегия {name}"),
            Self::Trick { candidate } => format!(
                "Приём zapret2: {}, профиль {}",
                candidate.source,
                candidate.profile + 1
            ),
            Self::ExtendRule { rule, protocol } => {
                format!("Расширить правило «{rule}» до {}", protocol_text(*protocol))
            }
        }
    }

    /// Changes only the culprit's traffic, not the global strategy.
    pub fn pointed(&self) -> bool {
        matches!(
            self,
            Self::ProxyPoint { .. } | Self::Trick { .. } | Self::ExtendRule { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ctx {
    pub proxy_ok: bool,
    /// Strategy full names, by the last test ranking.
    pub ranking: Vec<String>,
    /// (full name, args for `strategy_covers`).
    pub strategies: Vec<(String, String)>,
    /// (full name, text) for `candidates`.
    pub zapret2: Vec<(String, String)>,
    /// Route rules with `apps`.
    pub app_rules: Vec<Rule>,
    pub profile: Profile,
}

fn point_rule(group: &Group, profile: &Profile) -> Rule {
    let (ips, domains) = if is_subnet(&group.key) {
        (vec![group.key.clone()], vec![])
    } else {
        (vec![], vec![group.key.clone()])
    };
    Rule {
        name: format!("diag-{}", profile.id),
        enabled: true,
        protocol: group.protocol,
        ports: group.ports.iter().map(u16::to_string).collect(),
        ips,
        domains,
        apps: profile.apps.clone(),
    }
}

/// Paths in the spec's order, and the paths left out with the reason.
pub fn plan(group: &Group, ctx: &Ctx) -> (Vec<PathKind>, Vec<String>) {
    let mut paths = vec![PathKind::Current, PathKind::Direct];
    let mut skipped = vec![];
    if ctx.proxy_ok {
        let rule = point_rule(group, &ctx.profile);
        paths.push(PathKind::ProxyPoint { rule: rule.clone() });
        paths.push(PathKind::ProxyNoZapret { rule });
    } else {
        for p in ["Прокси точечно", "Прокси без zapret"] {
            skipped.push(format!("{p}: прокси не отвечает"));
        }
    }
    // Pattern R1: the program's rule lacks the culprit's protocol.
    for rule in &ctx.app_rules {
        let mine = rule
            .apps
            .iter()
            .any(|a| ctx.profile.apps.iter().any(|b| a.eq_ignore_ascii_case(b)));
        if mine && rule.protocol != Protocol::Both && rule.protocol != group.protocol {
            paths.push(PathKind::ExtendRule {
                rule: rule.name.clone(),
                protocol: Protocol::Both,
            });
        }
    }
    for name in &ctx.ranking {
        let Some((_, args)) = ctx.strategies.iter().find(|(n, _)| n == name) else {
            continue;
        };
        if group
            .ports
            .iter()
            .any(|&port| strategy_covers(args, group.protocol, port))
        {
            paths.push(PathKind::Strategy { name: name.clone() });
        } else {
            skipped.push(format!(
                "Стратегия {name}: фильтр не покрывает {} {}",
                protocol_text(group.protocol),
                group.ports.first().map_or(String::new(), u16::to_string)
            ));
        }
    }
    paths.extend(
        candidates(&ctx.zapret2, group)
            .into_iter()
            .map(|candidate| PathKind::Trick { candidate }),
    );
    (paths, skipped)
}

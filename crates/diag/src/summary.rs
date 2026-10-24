//! The result in words: per culprit what was seen and why, what "Apply" does, warnings (spec 8, 16).

use router::rules::{Protocol, Rule};
use serde::{Deserialize, Serialize};
use zapret::checks::{CheckId, CheckLevel, SystemCheck};
use zapret::custom::quoted_tokens;

use crate::circular_store::{Trick, is_filter};
use crate::culprits::{Fault, Group, Verdict};
use crate::evaluate::{Score, works};
use crate::paths::{Ctx, PROXY_NO_UDP, PROXY_SILENT, PROXY_UNSET, PathKind, protocol_text};
use crate::patterns::{Pattern, unique};
use crate::profiles::ProbeKind;
use crate::run::{GroupResult, Outcome};

const WHITELIST: &str =
    "провайдер пускает только в разрешённые сети; ни zapret, ни прокси это не обходят";
const ONLY_PROXY: &str = "провайдер блокирует адрес; zapret не помогает; через прокси работает";
const NOTHING: &str = "адрес недоступен ни одним путём, включая прокси: вероятно, проблема на стороне сервиса или прокси";
const NOTHING_NO_PROXY: &str =
    "адрес недоступен ни одним проверенным путём; путь через прокси не проверен";
pub const STOPPED: &str = "перебор остановлен: результаты по проверенным путям";
const LIVE_ONLY: &str = "не проверяется автоматически: нужен режим „со мной“";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum Fix {
    AddRule { rule: Rule },
    EditRule { name: String, protocol: Protocol },
    ApplyStrategy { name: String },
    ApplyHubCircular { tricks: Vec<Trick>, filter: String },
    TurnOff { target: TurnOffTarget, text: String },
    Manual { text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TurnOffTarget {
    /// The zapret service.
    Zapret,
    /// Whatever the culprit goes through now: the zapret strategy or a routing rule.
    Current,
}

/// `Default` fills what a report from another version lacks (`report::load`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct GroupSummary {
    pub group: Group,
    pub seen: String,
    /// Ranked, as tried.
    pub results: Vec<(PathKind, Score)>,
    /// Index into `results`.
    pub best: Option<u32>,
    pub explanation: String,
    /// Paths left out or not checked and why, and how the paths were set.
    pub notes: Vec<String>,
}

/// `Default` fills what a report from another version lacks (`report::load`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Summary {
    /// `YYYY-MM-DD_HH-MM-SS`.
    pub id: String,
    pub profile: String,
    pub verdict_text: String,
    pub groups: Vec<GroupSummary>,
    pub patterns: Vec<Pattern>,
    pub explanations: Vec<String>,
    pub fixes: Vec<Fix>,
    pub warnings: Vec<String>,
    pub canceled: bool,
    /// «Остановить»: the paths not measured yet were left out; the rest is the result.
    pub stopped: bool,
    pub budget_hit: bool,
    pub rollback_error: Option<String>,
    pub sources_missing: Vec<String>,
    /// `commands::system_checks` at the start (read-only); empty without a zapret folder.
    pub system_checks: Vec<SystemCheck>,
    /// Codes and facts for an agent (agent proposal); filled by the app after `summarize`.
    pub agent: crate::agent::AgentView,
}

fn seen(f: &Fault) -> String {
    match f {
        Fault::SynUnanswered { syns } => format!("TCP: {syns} SYN без ответа"),
        Fault::ResetEarly => "TCP: сброс (RST) в начале ответа".into(),
        Fault::RedirectEarly => "TCP: редирект провайдера в начале ответа".into(),
        Fault::DataRetrans { count } => format!("TCP: {count} повторов данных"),
        Fault::UdpNoReply { up, down } => {
            format!("UDP уходит, ответов нет ({up} туда, {down} обратно)")
        }
        Fault::Slow { median_ms } => format!("медленно: задержка {median_ms} мс"),
        Fault::Lossy { loss_pct } => format!("потери {loss_pct} %"),
    }
}

fn measured(s: &Score) -> bool {
    s.total > 0 || s.answer.is_some()
}

fn is_proxy(p: &PathKind) -> bool {
    matches!(
        p,
        PathKind::ProxyPoint { .. } | PathKind::ProxyNoZapret { .. }
    )
}

fn current_works(gr: &GroupResult) -> bool {
    gr.results
        .iter()
        .any(|(p, s)| *p == PathKind::Current && works(s))
}

fn explain(gr: &GroupResult, best: Option<&PathKind>) -> String {
    let Some(best) = best else {
        let checked: Vec<String> = gr
            .results
            .iter()
            .filter(|(_, s)| measured(s))
            .map(|(p, _)| p.label())
            .collect();
        let proxy_measured = gr.results.iter().any(|(p, s)| is_proxy(p) && measured(s));
        // Why the proxy paths were left out, if they were.
        let why = gr.skipped.iter().find_map(|s| {
            [PROXY_SILENT, PROXY_UNSET, PROXY_NO_UDP]
                .into_iter()
                .find(|w| s.ends_with(w))
        });
        let head = match why {
            _ if proxy_measured => NOTHING.to_owned(),
            Some(why) => format!("{NOTHING_NO_PROXY}: {why}"),
            None => NOTHING_NO_PROXY.to_owned(),
        };
        return if checked.is_empty() {
            "пути не проверены".into()
        } else {
            format!("{head}. Проверено: {}", checked.join(", "))
        };
    };
    match best {
        PathKind::Direct if !current_works(gr) => "мешает текущая стратегия/маршрутизация".into(),
        PathKind::Current | PathKind::Direct => "сейчас работает: сбой не повторился".into(),
        p @ (PathKind::ProxyPoint { .. } | PathKind::ProxyNoZapret { .. }) => {
            if gr.results.iter().all(|(p, s)| is_proxy(p) || !works(s)) {
                ONLY_PROXY.into()
            } else {
                format!("лучший путь — {}", p.label())
            }
        }
        PathKind::Strategy { name } => format!("DPI провайдера; помогает приём {name}"),
        PathKind::Trick { candidate } => format!(
            "DPI провайдера; помогает приём {}, профиль {}",
            candidate.source,
            candidate.profile + 1
        ),
        PathKind::ExtendRule { rule, protocol } => format!(
            "правило «{rule}» не ловит {}; расширенное до {} работает",
            protocol_text(gr.group.protocol),
            protocol_text(*protocol)
        ),
    }
}

/// `rule` into `old`: ports, addresses, domains and programs united; both protocols if they
/// differ.
pub fn unite(old: &mut Rule, rule: &Rule) {
    if old.protocol != rule.protocol {
        old.protocol = Protocol::Both;
    }
    for (to, from) in [
        (&mut old.ports, &rule.ports),
        (&mut old.ips, &rule.ips),
        (&mut old.domains, &rule.domains),
        (&mut old.apps, &rule.apps),
    ] {
        for x in from {
            if !to.contains(x) {
                to.push(x.clone());
            }
        }
    }
}

/// «Применить» of an `AddRule`: united with the rule of that name (names match regardless of
/// case, as in `rules`) and turned on, so what earlier diagnostics fixed stays fixed; else added.
pub fn add_rule(list: &mut Vec<Rule>, rule: Rule) {
    let name = rule.name.to_lowercase();
    match list.iter_mut().find(|r| r.name.to_lowercase() == name) {
        Some(old) => {
            unite(old, &rule);
            old.enabled = true;
        }
        None => list.push(rule),
    }
}

/// `AddRule`s of the same name become one rule; other duplicates are dropped.
fn push(fixes: &mut Vec<Fix>, fix: Fix) {
    if let Fix::AddRule { rule } = &fix
        && let Some(old) = fixes.iter_mut().find_map(|f| match f {
            Fix::AddRule { rule: r } if r.name == rule.name => Some(r),
            _ => None,
        })
    {
        unite(old, rule);
        return;
    }
    if !fixes.contains(&fix) {
        fixes.push(fix);
    }
}

fn fixes_for(gr: &GroupResult, best: &PathKind, ctx: &Ctx, id: &str, out: &mut Vec<Fix>) {
    let add_rule = |rule: &Rule, out: &mut Vec<Fix>| {
        let rule = Rule {
            name: ctx.profile.rule_name(),
            ..rule.clone()
        };
        push(out, Fix::AddRule { rule });
    };
    match best {
        PathKind::Current => {}
        PathKind::Direct => {
            if !current_works(gr) {
                push(
                    out,
                    Fix::TurnOff {
                        target: TurnOffTarget::Current,
                        text: format!(
                            "стратегию zapret или правило маршрутизации, через которые идёт {}",
                            gr.group.key
                        ),
                    },
                );
            }
        }
        PathKind::ProxyPoint { rule } => add_rule(rule, out),
        PathKind::ProxyNoZapret { rule } => {
            add_rule(rule, out);
            push(
                out,
                Fix::TurnOff {
                    target: TurnOffTarget::Zapret,
                    text: "службу zapret: через прокси адрес работает только без неё".into(),
                },
            );
        }
        PathKind::Strategy { name } => push(out, Fix::ApplyStrategy { name: name.clone() }),
        PathKind::Trick { candidate } => {
            let found = id.split('_').next().unwrap_or(id).to_owned();
            let tricks = gr
                .results
                .iter()
                .filter(|(_, s)| works(s))
                .filter_map(|(p, s)| match p {
                    PathKind::Trick { candidate: c } => Some(Trick {
                        source: c.source.clone(),
                        text: c.text.clone(),
                        found: found.clone(),
                        median_ms: s.median_ms,
                        ok: s.ok,
                        total: s.total,
                    }),
                    _ => None,
                })
                .collect();
            let filter = quoted_tokens(&candidate.text)
                .into_iter()
                .filter(|t| is_filter(t))
                .collect::<Vec<_>>()
                .join(" ");
            push(out, Fix::ApplyHubCircular { tricks, filter });
        }
        PathKind::ExtendRule { rule, protocol } => push(
            out,
            Fix::EditRule {
                name: rule.clone(),
                protocol: *protocol,
            },
        ),
    }
}

/// Explanations and advice of the patterns other than R1 (its group explains it) and R5.
fn pattern_texts(patterns: &[Pattern], explanations: &mut Vec<String>, fixes: &mut Vec<Fix>) {
    let mut spoofed = vec![];
    for p in patterns {
        match p {
            Pattern::R1RuleMissesProtocol { rule, has, needs } => explanations.push(format!(
                "правило «{rule}» ведёт через прокси только {}, а программа ходит и по {}: напрямую провайдер не пускает",
                protocol_text(*has),
                protocol_text(*needs)
            )),
            Pattern::R1ViaSystemProxy {
                domain,
                direct_ok,
                proxy_ok,
                route,
                direct,
                rule,
            } => {
                let ok = |b: bool| if b { "работает" } else { "не отвечает" };
                explanations.push(format!(
                    "{domain} идёт через системный прокси: напрямую {}, через прокси {}; NekoBox отправляет его {}",
                    ok(*direct_ok),
                    ok(*proxy_ok),
                    route.as_deref().unwrap_or("неизвестно куда: в журнале NekoBox его нет")
                ));
                if !direct_ok && *proxy_ok && *direct {
                    let what = rule.as_deref().unwrap_or(domain);
                    push(
                        fixes,
                        Fix::Manual {
                            text: format!(
                                "NekoBox: Настройки → Маршрутизация → Basic: убрать «{what}» из Direct, добавить «{domain}» в Proxy; затем «Проверить снова»"
                            ),
                        },
                    );
                }
            }
            Pattern::R2VoiceUdp { server, via_vpn } => explanations.push(if *via_vpn {
                format!("голос Discord ({server}) идёт через VPN: медиасервер видит адрес прокси")
            } else {
                format!(
                    "голосовой UDP Discord до {server} напрямую не проходит; путь — прокси точечно (готовое правило «Discord голос»)"
                )
            }),
            Pattern::R3SlowProxyDns { median_ms, direct_dns } => {
                explanations.push(
                    "NekoBox долго ищет адреса через свой прямой DNS; каждое соединение ждёт"
                        .into(),
                );
                push(
                    fixes,
                    Fix::Manual {
                        text: format!(
                            "соединения NekoBox ждут {median_ms} мс: смените в NekoBox прямой DNS ({direct_dns}) на рабочий, например https://common.dot.dns.yandex.net/dns-query"
                        ),
                    },
                );
            }
            Pattern::R4DnsSpoofed { domain } => spoofed.push(domain.as_str()),
            Pattern::R5Whitelist => {}
            // `alone` is never set now: alone, DAVE waits for a peer (`judge`).
            Pattern::R6DaveStalled { direct, .. } => {
                explanations.push(
                    "сеть в порядке: соединение с голосовым сервером есть, но собеседник не прислал ключи шифрования"
                        .into(),
                );
                push(
                    fixes,
                    Fix::Manual {
                        text: "пусть собеседник перезайдёт или перезапустит Discord; проверить себя — зайти в канал одному".into(),
                    },
                );
                if *direct {
                    push(
                        fixes,
                        Fix::Manual {
                            text: "Настройки → Маршрутизация → Basic: убрать discord.media из Direct, добавить в Proxy".into(),
                        },
                    );
                }
            }
        }
    }
    if !spoofed.is_empty() {
        explanations.push(format!("провайдер подменяет DNS ({})", spoofed.join(", ")));
        push(
            fixes,
            Fix::Manual {
                text: "включите в Windows DoH «только зашифрованный», шаблон автоматический; доменные правила маршрутизации при этом перестанут срабатывать (правила по IP — нет)".into(),
            },
        );
    }
}

pub fn summarize(
    verdict: &Verdict,
    outcome: &Outcome,
    patterns: &[Pattern],
    ctx: &Ctx,
    id: String,
    observe_secs: u32,
) -> Summary {
    let whitelist = patterns.contains(&Pattern::R5Whitelist);
    let mut fixes = vec![];
    let mut groups = vec![];
    for gr in &outcome.per_group {
        // Ranked: the first one is the best if it works.
        let best = (!whitelist && gr.results.first().is_some_and(|(_, s)| works(s))).then_some(0);
        let best_path = best.map(|_| &gr.results[0].0);
        if let Some(p) = best_path {
            fixes_for(gr, p, ctx, &id, &mut fixes);
        }
        groups.push(GroupSummary {
            group: gr.group.clone(),
            seen: seen(&gr.group.fault),
            results: gr.results.clone(),
            best,
            explanation: if whitelist {
                WHITELIST.into()
            } else {
                explain(gr, best_path)
            },
            notes: gr.skipped.iter().chain(&gr.not_checked).cloned().collect(),
        });
    }
    let mut explanations = vec![];
    pattern_texts(patterns, &mut explanations, &mut fixes);
    let live_only = !ctx.profile.probes.is_empty()
        && ctx.profile.probes.iter().all(|p| p.kind == ProbeKind::Live);
    let unmeasured = !outcome.per_group.is_empty()
        && outcome
            .per_group
            .iter()
            .all(|g| g.results.iter().all(|(_, s)| !measured(s)));
    // In "ask" mode every path has an answer, so unmeasured paths mean "auto".
    if live_only && unmeasured {
        explanations.push(LIVE_ONLY.into());
    }
    let explanations = unique(explanations);
    // What fails the way NekoBox sends it now: direct, through the VPN, or (not known) both.
    let proxied_broken: Vec<&str> = patterns
        .iter()
        .filter_map(|p| match p {
            Pattern::R1ViaSystemProxy {
                domain,
                direct_ok,
                proxy_ok,
                route,
                direct,
                rule,
            } => {
                let fails = if *direct {
                    !direct_ok
                } else if route.is_some() || rule.is_some() {
                    !proxy_ok
                } else {
                    !direct_ok && !proxy_ok
                };
                fails.then_some(domain.as_str())
            }
            _ => None,
        })
        .collect();
    let verdict_text = match verdict {
        _ if whitelist => WHITELIST.into(),
        Verdict::NoFlows => {
            // The hub sees only connections opened while it watches.
            format!(
                "программа не открывала новых соединений за {observe_secs} с: запустите её и повторите проблему во время наблюдения"
            )
        }
        Verdict::Clean if !proxied_broken.is_empty() => format!(
            "напрямую соединения программы проходят; через системный прокси не работает: {}",
            proxied_broken.join(", ")
        ),
        Verdict::Clean => "соединения программы проходят; если проблема есть, она не в сети".into(),
        Verdict::Culprits(g) => format!(
            "проблемные адреса: {}",
            g.iter()
                .map(|g| g.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let verdict_text = if outcome.stopped {
        format!("{STOPPED}. {verdict_text}")
    } else {
        verdict_text
    };
    Summary {
        id,
        profile: ctx.profile.name.clone(),
        verdict_text,
        groups,
        patterns: patterns.to_vec(),
        explanations,
        fixes,
        warnings: vec![],
        canceled: outcome.canceled,
        stopped: outcome.stopped,
        budget_hit: outcome.budget_hit,
        rollback_error: outcome.rollback_error.clone(),
        sources_missing: vec![],
        system_checks: vec![],
        agent: Default::default(),
    }
}

/// What a failed system check means, in a word or two.
fn check_text(id: CheckId) -> &'static str {
    match id {
        CheckId::Bfe => "служба Base Filtering Engine не запущена, без неё zapret не работает",
        CheckId::Proxy => "включён системный прокси",
        CheckId::TcpTimestamps => "TCP timestamps выключены, а стратегиям они нужны",
        CheckId::Adguard => "запущен AdGuard, он может мешать Discord",
        CheckId::Killer => "службы Killer конфликтуют с zapret",
        CheckId::IntelConnectivity => "служба Intel Connectivity Network конфликтует с zapret",
        CheckId::CheckPoint => "Check Point конфликтует с zapret",
        CheckId::SmartByte => "SmartByte конфликтует с zapret",
        CheckId::CyrillicPath => "в пути к zapret есть кириллица",
        CheckId::OneDrive => "zapret лежит в папке OneDrive",
        CheckId::WinDivertSys => "в папке bin нет WinDivert64.sys",
        CheckId::Vpn => "найдены службы VPN, они могут конфликтовать с zapret",
        CheckId::SecureDns => "безопасный DNS не настроен",
        CheckId::HostsYoutube => "в hosts есть записи YouTube",
        CheckId::WinDivertOrphan => "служба WinDivert работает без winws",
        CheckId::Conflicts => "установлены службы других обходов",
    }
}

/// An explanation line per check that is not `Ok`.
pub fn check_lines(checks: &[SystemCheck]) -> Vec<String> {
    checks
        .iter()
        .filter(|c| c.level != CheckLevel::Ok)
        .map(|c| {
            let level = match c.level {
                CheckLevel::Problem => "проблема",
                _ => "внимание",
            };
            let detail = if c.detail.is_empty() {
                String::new()
            } else {
                format!(" ({})", c.detail)
            };
            format!("системная проверка, {level}: {}{detail}", check_text(c.id))
        })
        .collect()
}

/// Keys are `<profile>: <what is probed>` (or just the profile); one warning per regressed key.
pub fn warn_breaks(before: &[(String, bool)], after: &[(String, bool)]) -> Vec<String> {
    before
        .iter()
        .filter(|(k, ok)| *ok && after.iter().any(|(a, ok)| a == k && !ok))
        .map(|(k, _)| match k.split_once(": ") {
            Some((profile, what)) => format!("сломает: {profile} ({what} не отвечает)"),
            None => format!("сломает: {k}"),
        })
        .collect()
}

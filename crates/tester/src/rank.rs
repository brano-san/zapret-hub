//! Scores and order (spec 7.4).

use probe::curl::{CheckStatus, DpiStatus};
use zapret::targets::{Target, url_host};

use crate::model::{
    DpiTargetResult, Pass, ProfileScore, Score, StrategyResult, Suite, TargetResult,
};

pub const DISCORD: [&str; 4] = [
    "discord.com",
    "gateway.discord.gg",
    "cdn.discordapp.com",
    "updates.discord.com",
];
pub const YOUTUBE: [&str; 3] = [
    "www.youtube.com",
    "i.ytimg.com",
    "redirector.googlevideo.com",
];
pub const DBD: [&str; 3] = [
    "steam.live.bhvrdbd.com",
    "api.epicgames.dev",
    "store.steampowered.com",
];

/// Profile hosts `targets` does not check: they run on the side, outside the count.
pub fn extra_targets(targets: &[Target]) -> Vec<Target> {
    let covered: Vec<String> = targets
        .iter()
        .filter_map(|t| t.url.as_deref())
        .map(|url| url_host(url).to_ascii_lowercase())
        .collect();
    DISCORD
        .into_iter()
        .chain(YOUTUBE)
        .chain(DBD)
        .filter(|host| !covered.iter().any(|c| c == host))
        .map(|host| Target {
            name: host.to_owned(),
            url: Some(format!("https://{host}")),
            ping: host.to_owned(),
        })
        .collect()
}

pub fn score_standard(targets: &[TargetResult]) -> Score {
    let mut score = Score::default();
    let (mut times, mut pings) = (Vec::new(), Vec::new());
    for target in targets.iter().filter(|t| !t.extra) {
        for check in &target.checks {
            score.total += 1;
            match check.status {
                CheckStatus::Ok => {
                    score.ok += 1;
                    times.extend(check.time_ms);
                }
                CheckStatus::Ssl => score.ssl += 1,
                CheckStatus::Unsup => score.unsup += 1,
                CheckStatus::Error => score.errors += 1,
            }
        }
        score.ping_total += 1;
        if let Some(ms) = target.ping_ms {
            score.ping_ok += 1;
            pings.push(ms);
        }
    }
    score.median_ms = median(&mut times);
    score.ping_median_ms = median(&mut pings);
    score.discord = profile(targets, &DISCORD);
    score.youtube = profile(targets, &YOUTUBE);
    score.dbd = profile(targets, &DBD);
    score
}

/// Every check of the targets whose host is in `hosts`, extra ones included.
fn profile(targets: &[TargetResult], hosts: &[&str]) -> ProfileScore {
    let mut profile = ProfileScore::default();
    let mut times = Vec::new();
    let in_profile = |t: &&TargetResult| {
        t.url.as_deref().is_some_and(|url| {
            hosts
                .iter()
                .any(|host| url_host(url).eq_ignore_ascii_case(host))
        })
    };
    for check in targets.iter().filter(in_profile).flat_map(|t| &t.checks) {
        profile.total += 1;
        if check.status == CheckStatus::Ok {
            profile.ok += 1;
            times.extend(check.time_ms);
        }
    }
    profile.median_ms = median(&mut times);
    profile
}

pub fn score_dpi(targets: &[DpiTargetResult]) -> Score {
    let mut score = Score::default();
    for check in targets.iter().flat_map(|t| &t.checks) {
        score.total += 1;
        match check.status {
            DpiStatus::Ok => score.ok += 1,
            DpiStatus::Fail => score.fail += 1,
            DpiStatus::Unsupported => score.unsup += 1,
            DpiStatus::LikelyBlocked => score.likely_blocked += 1,
        }
    }
    score
}

/// Best first. Launch errors go last; then standard: fewer failed checks, lower median latency;
/// DPI: fewer LIKELY_BLOCKED, fewer FAIL; then `service.bat`'s menu order.
pub fn sort(suite: Suite, results: &mut [StrategyResult]) {
    results.sort_by_cached_key(|r| {
        let s = &r.score;
        let (first, second) = match suite {
            Suite::Standard => (s.not_ok(), s.median_ms.unwrap_or(u32::MAX)),
            Suite::Dpi => (s.likely_blocked, s.fail),
        };
        (
            r.launch_error.is_some(),
            first,
            second,
            zapret::strategies::natural_key(&format!("{}.bat", r.name)),
        )
    });
}

/// A strategy's passes (in pass order) as one result. Counts are summed: every strategy of a run
/// gets the same number of passes, so the sorting keys (failed checks, LIKELY_BLOCKED, FAIL) rank
/// as their averages would. Latencies are the median of the passes' medians. A launch error in
/// any pass is the result's: a strategy whose engine fails now and then ranks last. The targets
/// shown are the last pass's; each pass stays in `passes`. One pass comes back as it is.
pub fn aggregate(mut passes: Vec<StrategyResult>) -> StrategyResult {
    if passes.len() <= 1 {
        return passes.pop().expect("at least one pass");
    }
    let n = passes.len();
    let scores: Vec<Score> = passes.iter().map(|p| p.score.clone()).collect();
    let sum = |f: fn(&Score) -> u32| scores.iter().map(f).sum::<u32>();
    let mid =
        |f: fn(&Score) -> Option<u32>| median(&mut scores.iter().filter_map(f).collect::<Vec<_>>());
    let profile = |f: fn(&Score) -> &ProfileScore| ProfileScore {
        ok: scores.iter().map(|s| f(s).ok).sum(),
        total: scores.iter().map(|s| f(s).total).sum(),
        median_ms: median(
            &mut scores
                .iter()
                .filter_map(|s| f(s).median_ms)
                .collect::<Vec<_>>(),
        ),
    };
    let score = Score {
        ok: sum(|s| s.ok),
        total: sum(|s| s.total),
        errors: sum(|s| s.errors),
        ssl: sum(|s| s.ssl),
        unsup: sum(|s| s.unsup),
        median_ms: mid(|s| s.median_ms),
        ping_ok: sum(|s| s.ping_ok),
        ping_total: sum(|s| s.ping_total),
        ping_median_ms: mid(|s| s.ping_median_ms),
        fail: sum(|s| s.fail),
        likely_blocked: sum(|s| s.likely_blocked),
        discord: profile(|s| &s.discord),
        youtube: profile(|s| &s.youtube),
        dbd: profile(|s| &s.dbd),
    };
    let launch_error = passes.iter().enumerate().find_map(|(i, p)| {
        p.launch_error
            .as_ref()
            .map(|err| format!("pass {} of {n}: {err}", i + 1))
    });
    let args_sha256 = passes
        .iter()
        .map(|p| p.args_sha256.clone())
        .find(|sha| !sha.is_empty())
        .unwrap_or_default();
    let last = passes.last().expect("more than one pass");
    StrategyResult {
        name: last.name.clone(),
        args_sha256,
        launch_error,
        targets: last.targets.clone(),
        dpi: last.dpi.clone(),
        score,
        passes: passes
            .into_iter()
            .map(|p| Pass {
                launch_error: p.launch_error,
                targets: p.targets,
                dpi: p.dpi,
                score: p.score,
            })
            .collect(),
    }
}

/// The lower middle: `[1, 2, 3, 4]` → 2.
pub fn median(values: &mut [u32]) -> Option<u32> {
    values.sort_unstable();
    values.get(values.len().checked_sub(1)? / 2).copied()
}

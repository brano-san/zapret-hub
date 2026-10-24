//! What the program's own logs say (agent proposal 3): `Summary.agent.client` and the kinds of
//! the culprits. Read-only; it runs `reg query` and nothing else.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use diag::agent::{CulpritKind, Fact};
use diag::culprits::Verdict;
use diag::logs::{self, discord_client, steam, webrtc};
use diag::paths::Ctx;
use diag::summary::Summary;

use super::evidence::{Marks, wall_secs};

const WEBRTC_LOGS: [&str; 2] = ["discord-webrtc_0", "discord-webrtc_1"];
/// The renderer's state needs the start of the session, which may be hours back.
const RENDERER_BYTES: u64 = 2 << 20;
/// Of the media engine's log, only what was written during the diagnostics.
const WEBRTC_BYTES: u64 = 4 << 20;
const UPDATER_BYTES: u64 = 1 << 20;
const STEAM_BYTES: u64 = 1 << 20;

/// The sizes of the media engine's logs when watching began.
/// ponytail: a rename of `_0` to `_1` during the run loses `_0`'s new lines; mark by time if
/// Discord turns out to rotate that way.
pub(super) fn webrtc_marks() -> Vec<(PathBuf, u64)> {
    logs::discord_log_dirs()
        .into_iter()
        .flat_map(|dir| WEBRTC_LOGS.map(|f| dir.join(f)))
        .map(|p| {
            let at = logs::size(&p).unwrap_or(0);
            (p, at)
        })
        .collect()
}

fn read(path: &Path, at: u64, max: u64) -> Option<Vec<String>> {
    logs::tail_from(path, at, max).ok().flatten()
}

/// Fills `summary.agent.client` and, for Discord, `culprit_kinds`.
pub(super) fn fill(summary: &mut Summary, ctx: &Ctx, marks: &Marks, verdict: &Verdict) {
    summary.agent.client.app = app_state(&ctx.profile.apps).into();
    match ctx.profile.id.as_str() {
        "discord" => discord(summary, marks, verdict),
        "steam" => summary.agent.client.facts.extend(steam_facts()),
        _ => {}
    }
}

fn app_state(apps: &[String]) -> &'static str {
    if apps.is_empty() {
        "unknown"
    } else if apps.iter().any(|a| !zapret::procs::find(a).is_empty()) {
        "running"
    } else {
        "not_running"
    }
}

fn discord(summary: &mut Summary, marks: &Marks, verdict: &Verdict) {
    let parsed: Vec<_> = logs::discord_log_dirs()
        .into_iter()
        .filter_map(|dir| {
            let lines = read(&dir.join("renderer_js.log"), 0, RENDERER_BYTES)?;
            Some((dir, discord_client::parse(&lines, Some(wall_secs()))))
        })
        .collect();
    // Discord, PTB or Canary: the one with a media connection
    let Some((dir, mut client)) = parsed
        .iter()
        .find(|(_, c)| c.voice.is_some() || c.stream.is_some())
        .or(parsed.first())
        .cloned()
    else {
        return;
    };
    let mut files: Vec<_> = marks
        .webrtc
        .iter()
        .filter(|(p, _)| p.parent() == Some(dir.as_path()))
        .collect();
    // the older file first
    files.sort_by_key(|(p, _)| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let lines: Vec<String> = files
        .into_iter()
        .filter_map(|(p, at)| read(p, *at, WEBRTC_BYTES))
        .flatten()
        .collect();
    let media = webrtc::parse(&lines);
    if let Some(voice) = client.voice.as_mut() {
        media.fill(voice, "default");
    }
    if let Some(stream) = client.stream.as_mut() {
        media.fill(stream, "stream");
    }
    let agent = &mut summary.agent;
    agent.client.voice = client.voice.clone();
    agent.client.stream = client.stream.clone();
    agent.client.gateway = client.gateway.clone();
    agent.client.facts.extend(media.facts());
    agent.client.facts.extend(
        read(&dir.join("Discord_updater_rCURRENT.log"), 0, UPDATER_BYTES)
            .and_then(|l| discord_client::updater_error(&l)),
    );
    if let Verdict::Culprits(groups) = verdict {
        agent.culprit_kinds = groups
            .iter()
            .map(|g| CulpritKind {
                key: g.key.clone(),
                kind: client
                    .kind_of_group(g.members.iter().map(|f| f.dst.as_str()))
                    .into(),
            })
            .collect();
    }
}

/// `SteamPath` from the registry, as Steam itself keeps it.
fn steam_dir() -> Option<PathBuf> {
    let mut cmd = Command::new("reg");
    cmd.args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]);
    let out = exec::run(&mut cmd, Duration::from_secs(10), None).ok()?;
    out.success()
        .then(|| steam::steam_path(&out.stdout))
        .flatten()
        .map(PathBuf::from)
}

fn steam_facts() -> Vec<Fact> {
    let Some(dir) = steam_dir() else {
        return Vec::new();
    };
    let logs = dir.join("logs");
    let today = exec::LocalTime::now().date();
    let tail = |name: &str| read(&logs.join(name), 0, STEAM_BYTES).unwrap_or_default();
    [
        steam::cm(&tail("connection_log.txt"), &today),
        steam::content(&tail("content_log.txt"), &today),
        steam::cef(&tail("cef_log.txt"), &today),
    ]
    .into_iter()
    .flatten()
    .collect()
}

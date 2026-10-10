//! A run as `test zapret.ps1` writes its results file: «Копировать».

use probe::curl::{CheckStatus, DpiStatus};

use crate::model::{Suite, TestRun};

pub fn to_text(run: &TestRun) -> String {
    let suite = match run.suite {
        Suite::Standard => "standard",
        Suite::Dpi => "dpi",
    };
    let mut lines: Vec<String> = Vec::new();
    for s in &run.strategies {
        lines.push(format!("Config: {}.bat (Type: {suite})", s.name));
        if let Some(err) = &s.launch_error {
            lines.extend(err.lines().map(|line| format!("  {line}")));
        }
        for t in s.targets.iter().filter(|t| !t.extra) {
            let http: Vec<String> = t
                .checks
                .iter()
                .map(|c| format!("{}:{}", c.kind.label(), token(c.status)))
                .collect();
            let ping = t
                .ping_ms
                .map_or_else(|| "Timeout".to_owned(), |ms| format!("{ms} ms"));
            lines.push(format!("  {} : {} | Ping: {ping}", t.name, http.join(" ")));
        }
        for t in &s.dpi {
            lines.push(if t.country.is_empty() {
                format!("  Target: {} ({})", t.id, t.provider)
            } else {
                format!("  Target: [{}] {} ({})", t.country, t.id, t.provider)
            });
            for c in &t.checks {
                lines.push(format!(
                    "    {}: code={}  up={} KB  down={} KB  time={}s  status={}",
                    c.kind.label(),
                    c.code,
                    kb(c.up_bytes),
                    kb(c.down_bytes),
                    c.time_s,
                    dpi_status(c.status)
                ));
            }
        }
        lines.push(String::new());
    }
    lines.push("=== ANALYTICS ===".to_owned());
    let width = run
        .strategies
        .iter()
        .map(|s| s.name.len() + 4)
        .max()
        .unwrap_or(0);
    for s in &run.strategies {
        let config = format!("{:width$}", format!("{}.bat", s.name));
        let sc = &s.score;
        lines.push(match run.suite {
            Suite::Standard => format!(
                "{config} : HTTP OK: {:>3}, ERR: {:>3}, UNSUP: {:>3}, Ping OK: {:>3}, Fail: {:>3}",
                sc.ok,
                sc.errors + sc.ssl,
                sc.unsup,
                sc.ping_ok,
                sc.ping_total - sc.ping_ok
            ),
            Suite::Dpi => format!(
                "{config} : OK: {:>3}, FAIL: {:>3}, UNSUP: {:>3}, BLOCKED: {:>3}",
                sc.ok, sc.fail, sc.unsup, sc.likely_blocked
            ),
        });
    }
    if let Some(best) = run.strategies.first().filter(|s| s.launch_error.is_none()) {
        lines.push(format!("Best strategy: {}.bat", best.name));
    }
    lines.join("\r\n") + "\r\n"
}

fn token(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "OK   ",
        CheckStatus::Ssl => "SSL  ",
        CheckStatus::Unsup => "UNSUP",
        CheckStatus::Error => "ERROR",
    }
}

fn dpi_status(status: DpiStatus) -> &'static str {
    match status {
        DpiStatus::Ok => "OK",
        DpiStatus::Fail => "FAIL",
        DpiStatus::Unsupported => "UNSUPPORTED",
        DpiStatus::LikelyBlocked => "LIKELY_BLOCKED",
    }
}

/// `[math]::Round(bytes / 1024, 1)` as PowerShell prints it: `64`, `63.5`.
fn kb(bytes: u32) -> String {
    format!("{}", (f64::from(bytes) / 1024.0 * 10.0).round() / 10.0)
}

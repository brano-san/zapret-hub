//! Steam's own logs (`<SteamPath>\logs`, agent proposal 3): connections to its servers (CM),
//! downloads, and the built-in browser's store and community pages. Only today's lines count:
//! Steam writes little, so the minutes of the diagnostics alone would rarely hold anything.

use crate::agent::Fact;

/// `[2026-10-24 10:24:16] message` of `today` (`2026-10-24`) -> message.
fn of_today<'a>(line: &'a str, today: &str) -> Option<&'a str> {
    let rest = line.strip_prefix('[')?.strip_prefix(today)?;
    Some(rest.split_once("] ")?.1)
}

const CM_FAILS: [&str; 3] = [
    "ConnectFailed(",
    "FailDueToNoHearbeatResponse",
    "GetCMListForConnect -- web API call failed",
];

/// `connection_log.txt`: failed connections to the CM servers today, and whether the client
/// logged on after the last one.
pub fn cm(lines: &[String], today: &str) -> Option<Fact> {
    let (mut fails, mut last, mut recovered) = (0u32, None, false);
    for msg in lines.iter().filter_map(|l| of_today(l, today)) {
        if CM_FAILS.iter().any(|m| msg.contains(m)) {
            fails += 1;
            last = Some(msg);
            recovered = false;
        } else if msg.contains("RecvMsgClientLogOnResponse()") {
            recovered = true;
        }
    }
    let last = last?;
    let (state, text) = if recovered {
        (
            "recovered",
            format!("Steam: связь с серверами (CM) рвалась сегодня {fails} раз, сейчас подключён"),
        )
    } else {
        (
            "failing",
            format!("Steam: не подключается к серверам (CM), ошибок сегодня: {fails}"),
        )
    };
    Some(Fact {
        code: "steam_cm_fail".into(),
        args: vec![fails.to_string(), state.into(), last.to_owned()],
        text,
    })
}

/// `content_log.txt`: download errors today and the last download rate.
pub fn content(lines: &[String], today: &str) -> Option<Fact> {
    let (mut errors, mut last, mut rate) = (0u32, None, String::new());
    for msg in lines.iter().filter_map(|l| of_today(l, today)) {
        if let Some(r) = msg.strip_prefix("Current download rate: ") {
            r.trim().clone_into(&mut rate);
        } else if (msg.contains("update canceled") || msg.to_lowercase().contains("fail"))
            && !msg.contains("ownership ticket")
        {
            errors += 1;
            last = Some(msg);
        }
    }
    let last = last?;
    Some(Fact {
        code: "steam_download_error".into(),
        args: vec![errors.to_string(), last.to_owned(), rate],
        text: format!("Steam: ошибки загрузки сегодня: {errors}, последняя: {last}"),
    })
}

/// `cef_log.txt`: network errors of the built-in browser today (store, community). Its lines
/// are `[pid:tid:MMDD/HHMMSS.mmm:ERROR:file(line)] message`; `today` is `2026-10-24`.
pub fn cef(lines: &[String], today: &str) -> Option<Fact> {
    let mmdd: String = today.get(5..)?.replace('-', "");
    let (mut errors, mut last) = (0u32, None);
    for l in lines {
        // `pid:tid:` may be missing
        let Some(stamp) = l.split(':').take(3).find(|p| p.contains('/')) else {
            continue;
        };
        let stamp = stamp.trim_start_matches('[');
        if !stamp.starts_with(&mmdd) {
            continue;
        }
        let code = l
            .split_once("net_error ")
            .map(|(_, rest)| format!("net_error {}", rest.split_whitespace().next().unwrap_or("")))
            .or_else(|| {
                l.split_once("net::ERR_").map(|(_, rest)| {
                    format!("net::ERR_{}", rest.split_whitespace().next().unwrap_or(""))
                })
            });
        if let Some(code) = code {
            errors += 1;
            last = Some(code);
        }
    }
    let last = last?;
    Some(Fact {
        code: "steam_store_error".into(),
        args: vec![errors.to_string(), last.clone()],
        text: format!(
            "Steam: магазин и сообщество не загружаются, сетевых ошибок сегодня: {errors} ({last})"
        ),
    })
}

/// The path of `HKCU\Software\Valve\Steam /v SteamPath` from the output of `reg query`.
pub fn steam_path(reg_output: &str) -> Option<String> {
    reg_output.lines().find_map(|l| {
        let (_, value) = l.split_once("REG_SZ")?;
        let value = value.trim();
        (l.trim_start().starts_with("SteamPath") && !value.is_empty()).then(|| value.to_owned())
    })
}

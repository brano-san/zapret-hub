//! Discord `renderer_js.log`: the state of the `default` voice connection as the client sees it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dave {
    NotStarted,
    KeyPackageSent { secs_waiting: u32 },
    Done,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    pub media_server: Option<String>,
    pub local_address: Option<String>,
    pub own_ssrc: Option<u32>,
    pub peers: Vec<u32>,
    pub dave: Dave,
    pub no_voice_state: bool,
    pub ws_closed: Vec<String>,
}

impl Voice {
    fn new() -> Self {
        Voice {
            media_server: None,
            local_address: None,
            own_ssrc: None,
            peers: Vec::new(),
            dave: Dave::NotStarted,
            no_voice_state: false,
            ws_closed: Vec::new(),
        }
    }
}

/// Seconds since midnight from the `[YYYY-MM-DD HH:MM:SS.mmm]` prefix.
fn line_secs(line: &str) -> Option<f64> {
    let t = line.strip_prefix('[')?.get(11..23)?;
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<f64>().ok();
    Some(n(0..2)? * 3600.0 + n(3..5)? * 60.0 + n(6..12)?)
}

fn num_after(s: &str, key: &str) -> Option<u32> {
    let rest = &s[s.find(key)? + key.len()..];
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// "Now" is the timestamp of the last timestamped line.
pub fn parse(lines: &[String]) -> Voice {
    let now = lines.iter().rev().find_map(|l| line_secs(l)).unwrap_or(0.0);
    let mut v = Voice::new();
    let mut key_package_at = None;
    for l in lines {
        if let Some((_, rest)) = l.split_once("[Connection(default)] Creating connection to ") {
            // a new connection starts the picture over
            v = Voice {
                own_ssrc: num_after(rest, "audio ssrc: "),
                ..Voice::new()
            };
            key_package_at = None;
        } else if let Some((_, rest)) =
            l.split_once("[Connection(default)] Connected with local address ")
        {
            v.local_address = rest.split(' ').next().map(str::to_owned);
        } else if let Some((_, rest)) = l.split_once(", default)] RTC connected to media server: ")
        {
            v.media_server = Some(rest.trim().to_owned());
        } else if let Some((_, rest)) = l.split_once("[Connection(default)] Creating user: ") {
            if let Some(s) = num_after(rest, "audio SSRC: ")
                && !v.peers.contains(&s)
            {
                v.peers.push(s);
            }
        } else if l.contains(", default)] DAVE protocol init with protocol version: ") {
            if num_after(l, "protocol version: ") == Some(0) {
                v.dave = Dave::Disabled;
            }
        } else if l.contains("[RTCControlSocket(default)] Sending MLS key package") {
            key_package_at = line_secs(l);
            v.dave = Dave::KeyPackageSent { secs_waiting: 0 };
        } else if key_package_at.is_some()
            && (l.contains("Received MLS commit")
                || l.contains("Sending MLS commit + welcome message")
                || (l.contains("DAVE protocol state update:")
                    && l.contains("\"epochAuthenticator\":\"")
                    && !l.contains("\"epochAuthenticator\":\"\"")))
        {
            v.dave = Dave::Done;
        } else if l.contains("[RTCConnectionStore] No VOICE_STATE_UPDATE received") {
            v.no_voice_state = true;
        } else if let Some((_, rest)) = l.split_once("[RTCControlSocket(default)] [WS CLOSED]") {
            v.ws_closed.push(rest.trim().to_owned());
        }
    }
    if let (Dave::KeyPackageSent { .. }, Some(t)) = (&v.dave, key_package_at) {
        let d = if now >= t { now - t } else { now + 86400.0 - t }; // midnight rollover
        v.dave = Dave::KeyPackageSent {
            secs_waiting: d as u32,
        };
    }
    if let Some(own) = v.own_ssrc {
        v.peers.retain(|p| *p != own);
    }
    v
}

/// The client has waited 15 s or more for the end-to-end encryption handshake.
pub fn stalled(v: &Voice) -> bool {
    matches!(v.dave, Dave::KeyPackageSent { secs_waiting } if secs_waiting >= 15)
}

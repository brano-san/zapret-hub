//! Discord's own view of its connections (agent proposal 3): the stage of the voice
//! (`Connection(default)`) and Go Live (`Connection(stream)`) connections and the gateway's state
//! from `renderer_js.log`, and the updater's last error.

use super::discord;
use crate::agent::{Fact, MediaState, VoiceStage};

/// The newest session of one connection, as far as the lines go.
#[derive(Debug, Default)]
struct Session {
    no_endpoint: bool,
    ws_connect: bool,
    ws_connected: bool,
    udp_started: bool,
    udp: bool,
    no_route: bool,
    ended: bool,
    server: Option<String>,
    own_ssrc: Option<u32>,
    peers: Vec<u32>,
}

/// What the renderer log says about the client's connections.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Client {
    pub voice: Option<MediaState>,
    pub stream: Option<MediaState>,
    /// `connecting`, `connected`, `resumed`, `connect_failed`, `closed`.
    pub gateway: Option<String>,
    /// Every media server (`ip:port`) the voice connection used in the lines.
    pub voice_servers: Vec<String>,
    /// Every media server the Go Live connections used: one per stream watched or shown.
    pub stream_servers: Vec<String>,
}

fn num_after(s: &str, key: &str) -> Option<u32> {
    let rest = &s[s.find(key)? + key.len()..];
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The newest session of `Connection(<tag>)` and every media server it used.
fn session(lines: &[String], tag: &str) -> (Option<Session>, Vec<String>) {
    let socket = format!("[RTCControlSocket({tag})] ");
    let conn = format!("[Connection({tag})] ");
    let rtc_state = format!(", {tag})] RTC connection state: ");
    let mut s: Option<Session> = None;
    let mut servers = Vec::new();
    for l in lines {
        if tag == "default" && l.contains("[RTCConnectionStore] No VOICE_STATE_UPDATE received") {
            // a working session goes on: the warning came from a channel switch
            if s.as_ref().is_some_and(|s| s.udp && !s.ended) {
                continue;
            }
            s = Some(Session {
                no_endpoint: true,
                ..Session::default()
            });
        } else if let Some((_, rest)) = l.split_once(&socket) {
            if rest.starts_with("[CONNECT] ") {
                // a WebSocket after the session ended starts a new one; else it resumes
                let cur = match s.take() {
                    Some(cur) if !cur.ended => cur,
                    _ => Session::default(),
                };
                s = Some(Session {
                    ws_connect: true,
                    ws_connected: false,
                    no_endpoint: false,
                    ..cur
                });
            } else if rest.starts_with("[CONNECTED] ")
                && let Some(cur) = s.as_mut()
            {
                cur.ws_connected = true;
            }
        } else if let Some((_, rest)) = l.split_once(&conn) {
            let cur = s.get_or_insert_with(Session::default);
            if let Some(to) = rest.strip_prefix("Creating connection to ") {
                let server = to.split(' ').next().unwrap_or_default().to_owned();
                if !servers.contains(&server) {
                    servers.push(server.clone());
                }
                cur.server = Some(server);
                cur.own_ssrc = num_after(rest, "audio ssrc: ");
                cur.udp_started = true;
                cur.udp = false;
                cur.no_route = false;
                cur.peers.clear();
            } else if rest.starts_with("Connected with local address ") {
                cur.udp = true;
            } else if rest.starts_with("Creating user: ") {
                // SSRC 0: the user is announced before their audio
                if let Some(ssrc) = num_after(rest, "audio SSRC: ")
                    && ssrc != 0
                    && Some(ssrc) != cur.own_ssrc
                    && !cur.peers.contains(&ssrc)
                {
                    cur.peers.push(ssrc);
                }
            } else if rest.starts_with("Connection state change: ") && rest.ends_with("=> NO_ROUTE")
            {
                cur.no_route = true;
            }
        } else if let Some((_, to)) = l.split_once(&rtc_state)
            && let Some(cur) = s.as_mut()
        {
            let to = to.rsplit("=> ").next().unwrap_or_default().trim();
            match to {
                "DISCONNECTED" => cur.ended = true,
                "AWAITING_ENDPOINT" => cur.no_endpoint = true,
                _ => {}
            }
        }
    }
    (s, servers)
}

/// The stage a session got to; `dave` is the voice connection's DAVE state, `None` for a stream.
fn stage(s: &Session, dave: Option<&discord::Voice>) -> VoiceStage {
    let failed = s.no_route || (s.udp_started && !s.udp);
    if s.ended && !failed {
        // left the channel, or a stream ended normally
        return VoiceStage::None;
    }
    if s.no_endpoint {
        return VoiceStage::NoEndpoint;
    }
    // the socket did not open, or opened and stalled before the server sent where to send UDP
    if (s.ws_connect && !s.ws_connected) || (s.ws_connected && !s.udp_started) {
        return VoiceStage::WsFailed;
    }
    if failed {
        return VoiceStage::UdpFailed;
    }
    if !s.udp {
        return VoiceStage::None;
    }
    match dave {
        Some(v) if matches!(v.dave, discord::Dave::KeyPackageSent { .. }) && s.peers.is_empty() => {
            VoiceStage::DaveWaitingPeer
        }
        Some(v) if discord::stalled(v) => VoiceStage::DaveStalled,
        _ => VoiceStage::Connected,
    }
}

fn media(s: &Session, dave: Option<&discord::Voice>) -> MediaState {
    MediaState {
        stage: stage(s, dave),
        server: s.server.clone(),
        peers: s.peers.len() as u32,
        ..MediaState::default()
    }
}

/// The gateway's last state, from `[GatewaySocket]` lines.
fn gateway(lines: &[String]) -> Option<String> {
    let mut state: Option<&str> = None;
    for l in lines {
        let Some((_, rest)) = l.split_once("[GatewaySocket] ") else {
            continue;
        };
        state = Some(if rest.starts_with("[CONNECT] ") {
            "connecting"
        } else if rest.starts_with("[CONNECTED] ") {
            "connected"
        } else if rest.starts_with("[RESUMED] ") {
            "resumed"
        } else if rest.starts_with("[WS CLOSED] ") {
            if state == Some("connecting") {
                "connect_failed"
            } else {
                "closed"
            }
        } else {
            continue;
        });
    }
    state.map(str::to_owned)
}

/// The client's connections from the tail of `renderer_js.log`; `wall` as for `discord::parse`.
pub fn parse(lines: &[String], wall: Option<f64>) -> Client {
    let dave = discord::parse(lines, wall);
    let (voice, voice_servers) = session(lines, "default");
    let (stream, stream_servers) = session(lines, "stream");
    Client {
        voice: voice.map(|s| media(&s, Some(&dave))),
        stream: stream.map(|s| media(&s, None)),
        gateway: gateway(lines),
        voice_servers,
        stream_servers,
    }
}

/// The IP of `ip:port`.
fn host(address: &str) -> &str {
    address.rsplit_once(':').map_or(address, |(h, _)| h)
}

impl Client {
    /// `stream` when an address (`ip:port` or a bare IP) is a Go Live server of the lines.
    pub fn is_stream(&self, address: &str) -> bool {
        self.kind_of(address) == "stream"
    }

    /// `voice`, `stream` or `other`: the exact `ip:port` first, then the IP alone.
    pub fn kind_of(&self, address: &str) -> &'static str {
        let exact = |servers: &[String]| servers.iter().any(|s| s == address);
        let ip = |servers: &[String]| servers.iter().any(|s| host(s) == host(address));
        if exact(&self.stream_servers) {
            "stream"
        } else if exact(&self.voice_servers) {
            "voice"
        } else if ip(&self.stream_servers) {
            "stream"
        } else if ip(&self.voice_servers) {
            "voice"
        } else {
            "other"
        }
    }

    /// A culprit group's kind from its flows' destinations: stream wins over voice.
    pub fn kind_of_group<'a>(&self, dsts: impl IntoIterator<Item = &'a str>) -> &'static str {
        let kinds: Vec<_> = dsts.into_iter().map(|d| self.kind_of(d)).collect();
        ["stream", "voice"]
            .into_iter()
            .find(|k| kinds.contains(k))
            .unwrap_or("other")
    }
}

const UPDATER_TEXT_CHARS: usize = 200;

/// The last error of `Discord_updater_rCURRENT.log`, unless an update finished after it.
pub fn updater_error(lines: &[String]) -> Option<Fact> {
    let mut last = None;
    for l in lines {
        if l.contains(" ERROR [") {
            last = Some(l);
        } else if l.contains("Update to latest complete") || l.contains("Already up to date") {
            last = None;
        }
    }
    let line = last?;
    let message = line.split_once("]: ").map_or(line.as_str(), |(_, m)| m);
    let message: String = message.chars().take(UPDATER_TEXT_CHARS).collect();
    let host = line
        .split_once("Domain(\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(h, _)| h.to_owned())
        .unwrap_or_default();
    let text = if host.is_empty() {
        "Discord не обновляется: ошибка обновления".to_owned()
    } else {
        format!("Discord не обновляется: нет связи с {host}")
    };
    Some(Fact {
        code: "updater_error".into(),
        args: vec![host, message],
        text,
    })
}

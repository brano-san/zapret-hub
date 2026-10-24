//! The clients' own logs: Discord's renderer stages, its media engine log and updater, Steam.

use std::path::PathBuf;

use diag::agent::VoiceStage;
use diag::logs::{discord_client, steam, webrtc};

fn lines(name: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn client(lines: &[&str]) -> discord_client::Client {
    let lines: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    discord_client::parse(&lines, None)
}

fn voice_stage(lines: &[&str]) -> VoiceStage {
    client(lines).voice.map(|v| v.stage).unwrap_or_default()
}

const CONNECT: &str = "[2026-10-24 19:00:44.081] [info]  [RTCControlSocket(default)] [CONNECT] wss://finland14000.discord.media:443/";
const CONNECTED: &str = "[2026-10-24 19:00:44.314] [info]  [RTCControlSocket(default)] [CONNECTED] wss://finland14000.discord.media:443/ in 233 ms";
const CREATING: &str = "[2026-10-24 19:00:44.389] [info]  [Connection(default)] Creating connection to 203.0.113.20:50003 with audio ssrc: 11029";
const LOCAL: &str = "[2026-10-24 19:00:44.458] [info]  [Connection(default)] Connected with local address 203.0.113.10:59820 and protocol: udp";
const PEER: &str = "[2026-10-24 19:00:44.460] [info]  [Connection(default)] Creating user: 111111111111111112 with audio SSRC: 10923 and video SSRCs: ";
const KEY_PACKAGE: &str =
    "[2026-10-24 19:00:44.500] [info]  [RTCControlSocket(default)] Sending MLS key package";
const LATER: &str = "[2026-10-24 19:01:10.000] [info]  [RTCControlSocket(default)] Sending heartbeat with last received sequence number: 5";

#[test]
fn voice_stages_from_the_renderer_log() {
    assert_eq!(voice_stage(&[]), VoiceStage::None);
    assert_eq!(
        voice_stage(&[
            "[2026-10-24 19:00:00.000] [warn]  [RTCConnectionStore] No VOICE_STATE_UPDATE received within 30000ms of VOICE_CHANNEL_SELECT [object Object]"
        ]),
        VoiceStage::NoEndpoint
    );
    assert_eq!(
        voice_stage(&[
            CONNECT,
            "[2026-10-24 19:00:46.000] [warn]  [RTCControlSocket(default)] [WS CLOSED] (clean: false, code: 1006, reason: ) retrying in 2.66 seconds.",
            CONNECT
        ]),
        VoiceStage::WsFailed
    );
    assert_eq!(voice_stage(&[CONNECT, CONNECTED]), VoiceStage::WsFailed);
    assert_eq!(
        voice_stage(&[CONNECT, CONNECTED, CREATING]),
        VoiceStage::UdpFailed
    );
    assert_eq!(
        voice_stage(&[CONNECT, CONNECTED, CREATING, LOCAL]),
        VoiceStage::Connected
    );
    assert_eq!(
        voice_stage(&[CONNECT, CONNECTED, CREATING, LOCAL, KEY_PACKAGE, LATER]),
        VoiceStage::DaveWaitingPeer
    );
    assert_eq!(
        voice_stage(&[
            CONNECT,
            CONNECTED,
            CREATING,
            LOCAL,
            PEER,
            KEY_PACKAGE,
            LATER
        ]),
        VoiceStage::DaveStalled
    );
    // left the channel after a working session
    assert_eq!(
        voice_stage(&[
            CONNECT,
            CONNECTED,
            CREATING,
            LOCAL,
            "[2026-10-24 19:05:00.000] [info]  [RTCConnection(111111111111111111, default)] RTC connection state: RTC_DISCONNECTED => DISCONNECTED"
        ]),
        VoiceStage::None
    );
    // the warning of a channel switch does not end a working session
    assert_eq!(
        voice_stage(&[
            CONNECT,
            CONNECTED,
            CREATING,
            LOCAL,
            "[2026-10-24 19:44:50.510] [warn]  [RTCConnectionStore] No VOICE_STATE_UPDATE received within 30000ms of VOICE_CHANNEL_SELECT [object Object]"
        ]),
        VoiceStage::Connected
    );
    // a resumed WebSocket keeps the UDP of the session
    assert_eq!(
        voice_stage(&[CONNECT, CONNECTED, CREATING, LOCAL, CONNECT, CONNECTED]),
        VoiceStage::Connected
    );
}

#[test]
fn voice_and_stream_apart() {
    let c = discord_client::parse(&lines("discord-client-stream.log"), None);
    let voice = c.voice.as_ref().unwrap();
    assert_eq!(voice.stage, VoiceStage::Connected);
    assert_eq!(voice.server.as_deref(), Some("203.0.113.20:50004"));
    assert_eq!(voice.peers, 1);
    let stream = c.stream.as_ref().unwrap();
    // NO_ROUTE stays a failure after Discord closes the attempt
    assert_eq!(stream.stage, VoiceStage::UdpFailed);
    assert_eq!(stream.server.as_deref(), Some("203.0.113.30:50008"));
    assert_eq!(c.gateway.as_deref(), Some("resumed"));
    assert!(c.is_stream("203.0.113.30:50008"));
    assert!(!c.is_stream("203.0.113.20:50004"));
    assert_eq!(c.kind_of("203.0.113.20:50004"), "voice");
    assert_eq!(c.kind_of("203.0.113.99:443"), "other");
    assert_eq!(
        c.kind_of_group(["203.0.113.20:50004", "203.0.113.30:50008"]),
        "stream"
    );
}

#[test]
fn gateway_that_does_not_connect() {
    let c = client(&[
        "[2026-10-24 14:25:33.653] [info]  [GatewaySocket] [CONNECT] wss://gateway-us-east1-d.discord.gg, encoding: etf",
        "[2026-10-24 14:25:43.653] [info]  [GatewaySocket] [WS CLOSED] (false, 1006, ) retrying in 1.82 seconds.",
    ]);
    assert_eq!(c.gateway.as_deref(), Some("connect_failed"));
}

#[test]
fn webrtc_pings_counters_and_facts() {
    let w = webrtc::parse(&lines("discord-webrtc.log"));
    // 461..464 count (465 may be on its way), 463 is lost
    assert_eq!(w.loss_pct("203.0.113.20:50004"), Some(25));
    assert_eq!(w.rtt_ms("203.0.113.20:50004"), Some(27));
    assert_eq!(w.loss_pct("203.0.113.30:50008"), None);
    // routine echoes succeed; only the failed one counts, on its socket's server
    assert_eq!(w.reconnects("203.0.113.20:50004"), 0);
    assert_eq!(w.reconnects("203.0.113.30:50008"), 1);
    assert!(!w.one_way("default"));
    assert!(w.one_way("stream"));
    let facts = w.facts();
    let dave = facts
        .iter()
        .find(|f| f.code == "dave_decrypt_failed")
        .unwrap();
    assert_eq!(dave.args, ["1", "27"]);
    let region = facts.iter().find(|f| f.code == "region_rtt").unwrap();
    assert_eq!(region.args[..2], ["finland", "31"]);
    assert_eq!(region.args.len(), 5);

    let mut media = diag::agent::MediaState {
        server: Some("203.0.113.20:50004".into()),
        ..Default::default()
    };
    w.fill(&mut media, "default");
    assert_eq!((media.loss_pct, media.rtt_ms), (Some(25), Some(27)));
}

#[test]
fn updater_error_until_an_update_completes() {
    let mut log = lines("discord-updater.log");
    let fact = discord_client::updater_error(&log).unwrap();
    assert_eq!(fact.code, "updater_error");
    assert_eq!(fact.args[0], "updates.discord.com");
    log.push("[2026-10-20 19:00:00.000000 +03:00] INFO [updater_client::install]: Update to latest complete.".into());
    assert_eq!(discord_client::updater_error(&log), None);
}

#[test]
fn steam_logs_of_today() {
    let today = "2026-10-24";
    let cm = steam::cm(&lines("steam-connection_log.txt"), today).unwrap();
    assert_eq!(cm.code, "steam_cm_fail");
    assert_eq!(cm.args[..2], ["3", "recovered"]);
    assert!(steam::cm(&lines("steam-connection_log.txt"), "2026-10-25").is_none());

    let content = steam::content(&lines("steam-content_log.txt"), today).unwrap();
    assert_eq!(content.code, "steam_download_error");
    assert_eq!(content.args[0], "1");
    assert_eq!(content.args[2], "0.000 Mbps");

    let cef = steam::cef(&lines("steam-cef_log.txt"), today).unwrap();
    assert_eq!(cef.code, "steam_store_error");
    assert_eq!(cef.args, ["2", "net_error -100"]);

    let reg = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    d:/steam\r\n\r\n";
    assert_eq!(steam::steam_path(reg).as_deref(), Some("d:/steam"));
}

#[test]
fn an_agent_view_with_fewer_fields_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = diag::report::dir(tmp.path());
    std::fs::create_dir_all(&dir).unwrap();
    let text = r#"{"id": "2026-10-24_12-00-00", "profile": "discord",
        "agent": {"verdict": "clean", "client": {"app": "running", "voice": null,
            "stream": null, "gateway": null, "facts": []}}}"#;
    std::fs::write(dir.join("2026-10-24_12-00-00.json"), text).unwrap();
    let s = diag::report::load(&dir, "2026-10-24_12-00-00").unwrap();
    assert_eq!(s.agent.client.app, "running");
    assert!(s.agent.culprit_kinds.is_empty());
}

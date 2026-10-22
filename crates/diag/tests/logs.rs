use diag::logs::{discord, neko, neko_routes, tail_since};
use std::io::Write;
use std::path::PathBuf;

fn fx(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn lines(name: &str) -> Vec<String> {
    tail_since(&fx(name), 8 << 20, |_| true).unwrap().unwrap()
}

#[test]
fn neko_pairs_inbound_with_outbound() {
    let conns = neko::parse(&lines("neko.log"));
    assert_eq!(conns.len(), 2);
    let c = &conns[0];
    assert_eq!(
        (c.host.as_str(), c.port, &c.route),
        ("c-arn08-a4ab867f.discord.media", 2096, &neko::Route::Direct)
    );
    assert_eq!(c.outbound_ms, Some(12));
    assert_eq!(conns[1].route, neko::Route::Proxy);
    assert_eq!(conns[1].outbound_ms, Some(166));
    // the later connection wins, as after a rule edit
    assert_eq!(
        neko::route_of(&conns, "discord.media"),
        Some(neko::Route::Proxy)
    );
    assert_eq!(neko::route_of(&conns, "example.org"), None);
}

#[test]
fn neko_slow_dns() {
    let slow = neko::parse(&lines("neko-slow-dns.log"));
    assert_eq!(slow.len(), 6);
    assert!(neko::slow_dns(&slow));
    assert!(!neko::slow_dns(&neko::parse(&lines("neko.log"))));
}

#[test]
fn routes_box_rule_for_discord_media() {
    let r = neko_routes::read(&fx("routes_box.json")).unwrap();
    assert_eq!(r.remote_dns, "https://dns.google/dns-query");
    assert_eq!(
        neko_routes::rule_for(&r, "c-arn08-a4ab867f.discord.media"),
        Some((neko::Route::Direct, "discord.media".to_owned()))
    );
    assert_eq!(
        neko_routes::rule_for(&r, "claude.ai").unwrap().0,
        neko::Route::Proxy
    );
    assert_eq!(neko_routes::rule_for(&r, "notdiscord.media"), None);
}

#[test]
fn discord_stalled_with_peer() {
    let v = discord::parse(&lines("discord-dave-stalled.log"));
    assert_eq!(v.own_ssrc, Some(11029));
    assert_eq!(v.peers, vec![10923]);
    assert_eq!(v.local_address.as_deref(), Some("203.0.113.10:16444"));
    assert_eq!(v.media_server.as_deref(), Some("104.29.149.39:19327"));
    assert!(v.no_voice_state);
    assert_eq!(v.ws_closed.len(), 1);
    assert!(discord::stalled(&v));
}

#[test]
fn discord_alone_stalled() {
    let v = discord::parse(&lines("discord-dave-alone.log"));
    assert!(v.peers.is_empty());
    assert!(discord::stalled(&v));
}

#[test]
fn discord_done() {
    let v = discord::parse(&lines("discord-dave-done.log"));
    assert_eq!(v.dave, discord::Dave::Done);
    assert!(!discord::stalled(&v));
}

#[test]
fn missing_log_is_skipped() {
    assert!(
        tail_since(&fx("nope.log"), 1024, |_| true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn reads_only_lines_since_start() {
    let l = tail_since(&fx("discord-dave-done.log"), 8 << 20, |l| {
        l >= "[2026-10-18 00:37:55.600"
    })
    .unwrap()
    .unwrap();
    assert_eq!(l.len(), 2);
}

#[test]
fn tail_limits_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.log");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
    let line = format!("{}\n", "x".repeat(99));
    for _ in 0..(10 << 20) / 100 {
        f.write_all(line.as_bytes()).unwrap();
    }
    f.write_all(b"LAST\n").unwrap();
    drop(f);
    let l = tail_since(&p, 8 << 20, |_| true).unwrap().unwrap();
    assert!(l.iter().map(|s| s.len() as u64 + 1).sum::<u64>() <= 8 << 20);
    assert_eq!(l.last().unwrap(), "LAST");
}

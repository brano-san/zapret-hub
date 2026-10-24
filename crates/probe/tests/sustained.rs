//! The sustained probes: an IP discovery burst, a WebSocket upgrade, a bulk exchange.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;

use probe::curl::{BulkStatus, Via, bulk_args, bulk_classify, ws_args, ws_classify};
use probe::udp::{Burst, burst_stats};

#[test]
fn a_burst_counts_loss_and_the_median() {
    assert_eq!(
        burst_stats(40, vec![30, 10, 20]),
        Burst {
            sent: 40,
            received: 3,
            loss_pct: 92,
            median_ms: Some(20)
        }
    );
    assert_eq!(burst_stats(0, vec![]).loss_pct, 100);
    assert_eq!(burst_stats(4, vec![]).median_ms, None);
    assert_eq!(burst_stats(2, vec![5, 9]).median_ms, Some(5));
}

#[test]
fn only_101_is_an_upgrade() {
    assert_eq!(ws_classify("101 0.366734"), Some(367));
    assert_eq!(ws_classify("400 0.2"), None);
    assert_eq!(ws_classify("000 0.000"), None);
    assert_eq!(ws_classify(""), None);
}

#[test]
fn the_upgrade_is_pinned_and_routed() {
    let ip = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    let args = ws_args("c-1.discord.media", 2096, ip, &Via::Direct).join(" ");
    assert!(args.contains("--http1.1"));
    assert!(args.contains("Upgrade: websocket"));
    assert!(args.contains("--resolve c-1.discord.media:2096:203.0.113.7"));
    assert!(args.contains("--noproxy *"));
    assert!(args.ends_with("https://c-1.discord.media:2096/?v=8"));
    let args = ws_args(
        "gateway.discord.gg",
        443,
        None,
        &Via::Socks5("127.0.0.1:2080".into()),
    )
    .join(" ");
    assert!(args.contains("--socks5-hostname 127.0.0.1:2080"));
    assert!(!args.contains("--resolve"));
}

#[test]
fn a_freeze_after_bytes_moved_is_throttling() {
    let ok = bulk_classify(Some(0), "200 65536 65536 0.512");
    assert_eq!(ok.status, BulkStatus::Ok);
    assert_eq!(ok.kbps, Some(250));
    // Stalled by --speed-limit after the 16-20 KB cut.
    let cut = bulk_classify(Some(28), "000 65536 0 5.003");
    assert_eq!(cut.status, BulkStatus::Throttled);
    assert_eq!(cut.up_bytes, 65536);
    // Nothing moved: not reached at all.
    assert_eq!(
        bulk_classify(Some(28), "000 0 0 3.001").status,
        BulkStatus::Fail
    );
    assert_eq!(bulk_classify(Some(7), "").status, BulkStatus::Fail);
    assert_eq!(bulk_classify(None, "").kbps, None);
}

#[test]
fn the_bulk_exchange_stalls_out_on_its_own() {
    let args = bulk_args("cdn.example.com", 443, None, Path::new("p.bin")).join(" ");
    assert!(args.contains("--speed-limit 1 --speed-time 3"));
    assert!(args.contains("@p.bin"));
    assert!(args.ends_with("https://cdn.example.com:443/"));
}

#[test]
#[ignore = "network"]
fn a_burst_to_a_voice_server() {
    let dst = "203.0.113.1:50001".parse().unwrap();
    let b = probe::udp::ip_discovery_burst(dst, 40, Duration::from_secs(4));
    assert_eq!(b.sent, 40);
}

#[test]
#[ignore = "network"]
fn the_gateway_upgrades() {
    assert!(probe::curl::ws_upgrade("gateway.discord.gg", 443, None, &Via::Direct).is_some());
}

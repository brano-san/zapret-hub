use router::divert::Address;
use router::flows::{self, DirectReason, FlowKey, Pending, ProxyState, Stats, Via};
use router::owner::{self, Owners, PID_WAIT};
use router::rules::Protocol;
use std::time::{Duration, Instant};

#[test]
fn the_queue_keeps_the_newest_64() {
    let mut queue = Pending::default();
    for i in 0..70u8 {
        queue.push(vec![i], Address([0; 80]));
    }
    let left = queue.drain();
    assert_eq!(left.len(), 64);
    assert_eq!((left[0].0[0], left[63].0[0]), (6, 69));
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[test]
fn new_flows_follow_the_proxy_state() {
    let ok = ProxyState::Ok { latency_ms: 3 };
    assert_eq!(flows::decide(&ok, Some(500), 1, Some(2)), None);
    assert_eq!(flows::decide(&ProxyState::Unknown, None, 1, None), None);
    assert_eq!(
        flows::decide(&ProxyState::NoUdp, None, 1, None),
        None,
        "asked again per flow"
    );
    assert_eq!(
        flows::decide(
            &ProxyState::Down {
                error: "refused".into()
            },
            None,
            1,
            None
        ),
        Some(DirectReason::ProxyDown)
    );
    assert_eq!(
        flows::decide(&ProxyState::AuthRequired, None, 1, None),
        Some(DirectReason::AuthRequired)
    );
}

#[test]
fn traffic_of_the_proxy_and_of_the_hub_goes_direct() {
    let ok = ProxyState::Ok { latency_ms: 3 };
    assert_eq!(
        flows::decide(&ok, Some(2), 1, Some(2)),
        Some(DirectReason::Loop)
    );
    assert_eq!(
        flows::decide(&ok, Some(1), 1, Some(2)),
        Some(DirectReason::Loop)
    );
}

#[test]
fn socks_errors_become_reasons() {
    use router::socks::SocksError;
    assert_eq!(
        DirectReason::from(&SocksError::Refused(7)),
        DirectReason::NoUdp
    );
    assert_eq!(
        DirectReason::from(&SocksError::Timeout),
        DirectReason::Timeout
    );
    assert!(matches!(
        DirectReason::from(&SocksError::Refused(1)),
        DirectReason::Failed { .. }
    ));
}

#[test]
fn a_late_socket_event_is_waited_for_50_ms() {
    let owners = std::sync::Arc::new(Owners::default());
    let late = owners.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        late.record(50001, 77);
    });
    assert_eq!(owners.wait(50001, PID_WAIT), Some(77));
    writer.join().unwrap();
    let started = Instant::now();
    assert_eq!(owners.wait(50002, PID_WAIT), None);
    assert!(started.elapsed() >= PID_WAIT && started.elapsed() < Duration::from_millis(500));
    owners.forget(50001);
    assert_eq!(owners.wait(50001, Duration::ZERO), None);
}

#[test]
fn the_listener_of_a_port_is_found() {
    assert_eq!(owner::port_of(u32::from(2080u16.to_be())), 2080);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_eq!(owner::listener_pid(port), Some(std::process::id()));
    drop(listener);
    assert_eq!(owner::listener_pid(port), None);
}

#[test]
fn closed_flows_stay_30_seconds_and_traffic_adds_up() {
    let t0 = Instant::now();
    let key = |port| FlowKey {
        protocol: Protocol::Udp,
        local_port: port,
        dst: "104.29.1.1:50001".parse().unwrap(),
    };
    let mut stats = Stats::default();
    stats.open(key(1), "voice", Via::Opening, t0);
    stats.add(key(1), 100, 0, t0);
    stats.set_via(key(1), Via::Proxy);
    stats.add(key(1), 0, 50, t0);
    stats.open(
        key(2),
        "voice",
        Via::Direct {
            reason: DirectReason::ProxyDown,
        },
        t0,
    );
    stats.add(key(2), 7, 0, t0);
    let totals = stats.totals();
    assert_eq!(
        (totals.proxied.flows, totals.proxied.up, totals.proxied.down),
        (1, 100.0, 50.0)
    );
    assert_eq!((totals.direct.flows, totals.direct.up), (1, 7.0));
    assert_eq!(stats.rule_traffic("voice").up, 107.0);
    assert_eq!(stats.idle(t0 + Duration::from_secs(31)).len(), 2);
    stats.close(key(1), t0 + Duration::from_secs(1));
    let rows = stats.rows(t0 + Duration::from_secs(30), true);
    assert!(rows.iter().any(|r| r.local_port == 1 && r.closed));
    assert!(
        stats
            .rows(t0 + Duration::from_secs(32), true)
            .iter()
            .all(|r| r.local_port != 1)
    );
    assert_eq!(rows[0].destination, "104.29.1.1:50001");
}

#[test]
fn one_failed_check_keeps_a_live_proxy() {
    let ok = ProxyState::Ok { latency_ms: 3 };
    let down = || ProxyState::Down {
        error: "refused".into(),
    };
    let mut failures = 0;
    assert_eq!(flows::checked(&ok, down(), &mut failures), None);
    assert_eq!(
        flows::checked(&ok, ok.clone(), &mut failures),
        Some(ok.clone())
    );
    assert_eq!(
        flows::checked(&ok, down(), &mut failures),
        None,
        "the count restarts"
    );
    assert_eq!(flows::checked(&ok, down(), &mut failures), Some(down()));
    let mut failures = 0;
    assert_eq!(
        flows::checked(&ProxyState::Unknown, down(), &mut failures),
        Some(down()),
        "nothing to keep yet"
    );
    assert_eq!(
        flows::checked(&ProxyState::NoUdp, ok.clone(), &mut failures),
        None,
        "NoUdp stays until an associate succeeds"
    );
}

#[test]
fn only_flows_sent_direct_by_a_proxy_failure_retry_it() {
    for reason in [
        DirectReason::ProxyDown,
        DirectReason::AuthRequired,
        DirectReason::NoUdp,
    ] {
        assert!(reason.proxy_failure(), "{reason:?}");
    }
    for reason in [
        DirectReason::Timeout,
        DirectReason::Loop,
        DirectReason::Failed { error: "x".into() },
    ] {
        assert!(!reason.proxy_failure(), "{reason:?}");
    }
}

#[test]
fn tcp_flows_never_go_idle_and_rows_name_protocol_and_domain() {
    let t0 = Instant::now();
    let tcp = FlowKey {
        protocol: Protocol::Tcp,
        local_port: 50000,
        dst: "162.159.135.232:443".parse().unwrap(),
    };
    let udp = FlowKey {
        protocol: Protocol::Udp,
        ..tcp
    };
    let mut stats = Stats::default();
    stats.open(tcp, "Discord целиком", Via::Proxy, t0);
    stats.open(udp, "Discord целиком", Via::Proxy, t0);
    stats.set_domain(tcp, "discord.com".into());
    assert_eq!(
        stats.idle(t0 + Duration::from_secs(600)),
        [udp],
        "a quiet TCP connection is still open"
    );
    let rows = stats.rows(t0, true);
    assert_eq!(rows.len(), 2, "same port and address, two protocols");
    let row = rows.iter().find(|r| r.protocol == "tcp").unwrap();
    assert_eq!(row.domain.as_deref(), Some("discord.com"));
}

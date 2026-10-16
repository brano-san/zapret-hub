use router::divert::Address;
use router::flows::{self, DirectReason, FlowKey, Pending, ProxyState, Stats, Via};
use router::owner::{self, Owners, PID_WAIT};
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

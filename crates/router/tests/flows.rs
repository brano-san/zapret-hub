use router::divert::Address;
use router::flows::{self, DirectReason, FlowKey, MAX_UNRULED, Pending, ProxyState, Stats, Via};
use router::owner::{self, Images, Owners, PID_WAIT, table_pid};
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
fn prune_drops_only_old_closed_flows() {
    let t0 = Instant::now();
    let key = |port| FlowKey {
        protocol: Protocol::Udp,
        local_port: port,
        dst: "104.29.1.1:50001".parse().unwrap(),
    };
    let mut stats = Stats::default();
    stats.open(key(1), "voice", Via::Opening, t0);
    stats.open(key(2), "voice", Via::Opening, t0);
    stats.close(key(1), t0);
    stats.prune(t0 + Duration::from_secs(29));
    assert_eq!(stats.rows(t0, true).len(), 2);
    stats.prune(t0 + Duration::from_secs(30));
    let rows = stats.rows(t0, true);
    assert_eq!((rows.len(), rows[0].local_port), (1, 2));
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
        DirectReason::Marked,
        DirectReason::NotAccepted,
        DirectReason::NoRule,
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

#[test]
fn an_unknown_owner_is_looked_up_then_remembered() {
    let owners = Owners::default();
    assert_eq!(
        owners.pid(5000, PID_WAIT, |_| Some(42)),
        Some(42),
        "from the tables"
    );
    assert_eq!(owners.pid(5000, PID_WAIT, |_| panic!("known")), Some(42));
    let started = Instant::now();
    assert_eq!(owners.pid(5001, PID_WAIT, |_| None), None);
    assert!(started.elapsed() >= PID_WAIT);
    let started = Instant::now();
    assert_eq!(owners.pid(5001, PID_WAIT, |_| panic!("remembered")), None);
    assert!(
        started.elapsed() < PID_WAIT,
        "a port without an owner is not waited for again"
    );
    owners.record(5001, 7);
    assert_eq!(
        owners.pid(5001, PID_WAIT, |_| None),
        Some(7),
        "a late event still lands"
    );
    owners.forget(5001);
    assert_eq!(
        owners.pid(5001, Duration::ZERO, |_| Some(8)),
        Some(8),
        "a closed socket is looked up again"
    );
}

#[test]
fn the_owner_of_a_bound_port_is_found_in_the_tables() {
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let me = Some(std::process::id());
    assert_eq!(
        table_pid(Protocol::Udp, udp.local_addr().unwrap().port()),
        me
    );
    assert_eq!(
        table_pid(Protocol::Tcp, tcp.local_addr().unwrap().port()),
        me
    );
}

#[test]
fn the_udp_table_lists_this_process() {
    let _udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let pids = owner::udp_pids();
    assert!(pids.contains(&std::process::id()), "{pids:?}");
    assert!(!pids.contains(&0));
}

#[test]
fn a_process_image_is_cached_by_pid_and_start_time() {
    let images = Images::default();
    let dbd = r"C:\Games\DeadByDaylight-Win64-Shipping.exe";
    assert_eq!(
        images.lookup(10, 1, || Some(dbd.into())).as_deref(),
        Some(dbd)
    );
    assert_eq!(
        images.lookup(10, 1, || panic!("cached")).as_deref(),
        Some(dbd)
    );
    assert_eq!(
        images
            .lookup(10, 2, || Some(r"C:\x\other.exe".into()))
            .as_deref(),
        Some(r"C:\x\other.exe"),
        "a reused PID"
    );
    images.prune(|pid| (pid == 10).then_some(2));
    assert_eq!(images.paths().len(), 1);
    images.prune(|_| None);
    assert!(images.paths().is_empty());
}

#[test]
fn this_process_has_a_start_time_and_an_image() {
    let pid = std::process::id();
    let start = owner::process_start(pid).unwrap();
    assert_eq!(owner::process_start(pid), Some(start));
    let exe = owner::image_path(pid).unwrap();
    let me = std::env::current_exe().unwrap();
    assert_eq!(exe.to_lowercase(), me.to_string_lossy().to_lowercase());
    assert_eq!(Images::default().get(pid).as_deref(), Some(exe.as_str()));
    assert_eq!(
        owner::process_start(0),
        None,
        "the idle process does not open"
    );
}

#[test]
fn a_pid_zero_row_never_owns_a_port() {
    let dw = u32::from(4000u16.to_be());
    assert_eq!(owner::row_owner(dw, 0, 4000), None, "TIME_WAIT");
    assert_eq!(owner::row_owner(dw, 9, 4000), Some(9));
    assert_eq!(owner::row_owner(dw, 9, 4001), None);
}

#[test]
fn an_ipv6_socket_is_found_in_the_ipv6_tables() {
    let tcp = std::net::TcpListener::bind("[::1]:0").unwrap();
    let udp = std::net::UdpSocket::bind("[::1]:0").unwrap();
    let me = Some(std::process::id());
    assert_eq!(
        table_pid(Protocol::Tcp, tcp.local_addr().unwrap().port()),
        me
    );
    assert_eq!(
        table_pid(Protocol::Udp, udp.local_addr().unwrap().port()),
        me
    );
}

#[test]
fn a_failed_image_lookup_is_cached() {
    let images = Images::default();
    assert_eq!(images.lookup(4, 1, || None), None);
    assert_eq!(images.lookup(4, 1, || panic!("cached miss")), None);
    assert!(images.paths().is_empty());
}

#[test]
fn rule_less_flows_show_only_in_all_with_their_program() {
    let t0 = Instant::now();
    let key = |local_port| FlowKey {
        protocol: Protocol::Udp,
        local_port,
        dst: "8.8.8.8:443".parse().unwrap(),
    };
    let none = || Via::Direct {
        reason: DirectReason::NoRule,
    };
    let mut stats = Stats::default();
    stats.open(key(1), "Dead by Daylight", Via::Proxy, t0);
    stats.set_program(key(1), r"C:\Games\DeadByDaylight-Win64-Shipping.exe".into());
    stats.open(key(2), "", none(), t0);
    stats.set_program(key(2), r"C:\x\chrome.exe".into());
    stats.add(key(2), 100, 0, t0);
    let ruled = stats.rows(t0, false);
    assert_eq!(ruled.len(), 1);
    assert_eq!(
        ruled[0].program.as_deref(),
        Some(r"C:\Games\DeadByDaylight-Win64-Shipping.exe")
    );
    let rows = stats.rows(t0, true);
    let row = rows.iter().find(|r| r.local_port == 2).unwrap();
    assert_eq!(
        (row.rule.as_deref(), row.program.as_deref()),
        (None, Some(r"C:\x\chrome.exe"))
    );
    assert_eq!(
        (stats.totals().direct.flows, stats.totals().proxied.flows),
        (0, 1),
        "rule-less flows are not in the totals"
    );
    for port in 0..MAX_UNRULED as u16 + 10 {
        stats.open(key(100 + port), "", none(), t0);
    }
    assert_eq!(
        stats.rows(t0, true).len(),
        MAX_UNRULED + 1,
        "rule-less rows are capped"
    );
}

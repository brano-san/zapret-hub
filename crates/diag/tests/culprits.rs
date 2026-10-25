use diag::culprits::*;
use router::observe::FlowCounters;
use router::rules::Protocol;

fn flow(dst: &str) -> FlowCounters {
    FlowCounters {
        protocol: Protocol::Tcp,
        local_port: 50000,
        dst: dst.into(),
        domain: None,
        sni: None,
        ipv6: false,
        via_system_proxy: false,
        via_router: false,
        up_packets: 10,
        down_packets: 10,
        up_bytes: 0.0,
        down_bytes: 0.0,
        syns: 1,
        syn_answered: true,
        syn_ack_ms: None,
        rst_early: false,
        redirect_early: false,
        data_retrans: 0,
        udp_first_reply_ms: None,
        udp_windows: 0,
        udp_silent_windows: 0,
        rtt_ms: vec![],
    }
}

fn syn(n: u32) -> FlowCounters {
    FlowCounters {
        syns: n,
        syn_answered: false,
        ..flow("1.2.3.4:443")
    }
}

fn udp(up: u32, down: u32) -> FlowCounters {
    FlowCounters {
        protocol: Protocol::Udp,
        up_packets: up,
        down_packets: down,
        ..flow("1.2.3.4:3478")
    }
}

fn rtt(v: Vec<u32>) -> FlowCounters {
    FlowCounters {
        rtt_ms: v,
        ..flow("1.2.3.4:443")
    }
}

#[test]
fn two_syns_are_not_enough() {
    assert_eq!(fault(&syn(2)), None);
}

#[test]
fn three_syns_fault() {
    assert_eq!(fault(&syn(3)), Some(Fault::SynUnanswered { syns: 3 }));
}

#[test]
fn udp_four_up_one_down_fault() {
    assert_eq!(
        fault(&udp(4, 1)),
        Some(Fault::UdpNoReply { up: 4, down: 1 })
    );
}

#[test]
fn tcp_four_up_one_down_is_not_udp_fault() {
    let f = FlowCounters {
        up_packets: 4,
        down_packets: 1,
        ..flow("1.2.3.4:443")
    };
    assert_eq!(fault(&f), None);
}

#[test]
fn udp_four_up_two_down_ok() {
    assert_eq!(fault(&udp(4, 2)), None);
}

#[test]
fn median_251_is_slow() {
    assert_eq!(fault(&rtt(vec![251])), Some(Fault::Slow { median_ms: 251 }));
    // Even count: the lower middle.
    assert_eq!(fault(&rtt(vec![900, 10, 300, 20])), None);
}

#[test]
fn median_250_is_not() {
    assert_eq!(fault(&rtt(vec![250])), None);
}

#[test]
fn loss_21_pct_lossy() {
    let f = |silent| FlowCounters {
        udp_windows: 100,
        udp_silent_windows: silent,
        ..udp(10, 10)
    };
    assert_eq!(fault(&f(21)), Some(Fault::Lossy { loss_pct: 21 }));
    assert_eq!(fault(&f(20)), None);
    let tcp = FlowCounters {
        protocol: Protocol::Tcp,
        ..f(50)
    };
    assert_eq!(fault(&tcp), None, "UDP windows mean nothing for TCP");
}

#[test]
fn ipv6_flows_are_judged_and_system_proxy_ones_are_not() {
    let v6 = FlowCounters {
        ipv6: true,
        ..syn(5)
    };
    let proxy = FlowCounters {
        via_system_proxy: true,
        via_router: false,
        ..syn(5)
    };
    assert_eq!(
        (fault(&v6), fault(&proxy)),
        (Some(Fault::SynUnanswered { syns: 5 }), None)
    );
}

#[test]
fn ipv6_flows_group_by_slash64() {
    let flows: Vec<_> = [
        "[2606:4700:10::1]:443",
        "[2606:4700:10::2]:443",
        "[2606:4700:11::1]:443",
    ]
    .iter()
    .map(|dst| FlowCounters {
        rst_early: true,
        ipv6: true,
        ..flow(dst)
    })
    .collect();
    let keys: Vec<_> = group(&flows)
        .into_iter()
        .map(|g| (g.key, g.members.len()))
        .collect();
    assert_eq!(
        keys,
        [
            ("2606:4700:10::/64".to_string(), 2),
            ("2606:4700:11::/64".to_string(), 1)
        ]
    );
}

#[test]
fn groups_by_domain_then_slash24() {
    let flows: Vec<_> = (4..14)
        .map(|i| FlowCounters {
            dst: format!("1.2.3.{i}:443"),
            up_packets: i,
            rst_early: true,
            ..flow("")
        })
        .collect();
    let g = group(&flows);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].key, "1.2.3.0/24");
    assert_eq!(g[0].members.len(), 10);
    assert_eq!(g[0].ports, vec![443]);
    let ups: Vec<_> = g[0].representatives.iter().map(|f| f.up_packets).collect();
    assert_eq!(ups, vec![13, 12, 11]);
    assert_eq!(g[0].fault, Fault::ResetEarly);

    let named = FlowCounters {
        domain: Some("a.example".into()),
        sni: Some("b.example".into()),
        ..flows[0].clone()
    };
    let sni_only = FlowCounters {
        sni: Some("b.example".into()),
        ..flows[1].clone()
    };
    let keys: Vec<_> = group(&[named, sni_only])
        .into_iter()
        .map(|g| g.key)
        .collect();
    assert_eq!(keys, vec!["a.example", "b.example"]);
}

#[test]
fn no_flows_is_not_a_clean_bill() {
    assert_eq!(verdict(&[]), Verdict::NoFlows);
    assert_eq!(verdict(&[flow("1.2.3.4:443")]), Verdict::Clean);
    assert!(matches!(verdict(&[syn(3)]), Verdict::Culprits(_)));
}

#[test]
fn a_flow_the_router_proxies_is_judged_by_its_replies() {
    // Live 2026-10-24: voice through the router's rule, 285 packets out, the replies injected.
    let routed = |protocol, down_bytes| FlowCounters {
        protocol,
        via_router: true,
        down_packets: 0,
        down_bytes,
        udp_windows: 19,
        udp_silent_windows: 19,
        ..udp(285, 0)
    };
    assert_eq!(
        fault(&routed(Protocol::Udp, 21_000.0)),
        None,
        "the router saw replies"
    );
    assert_eq!(
        fault(&routed(Protocol::Udp, 0.0)),
        Some(Fault::UdpNoReply { up: 285, down: 0 }),
        "nothing came back through the proxy either"
    );
    let tcp = FlowCounters {
        syns: 3,
        syn_answered: false,
        ..routed(Protocol::Tcp, 0.0)
    };
    assert_eq!(fault(&tcp), None, "the router's acceptor answers the SYN");
}

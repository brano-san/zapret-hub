use router::divert::MAX_FILTER_RANGES;
use router::observe::{FlowCounters, Tally, of_programs, sniff_filter};
use router::packet::{ACK, RST, SYN, build_udp};
use router::rules::Protocol;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

const ME: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 10), 50000);
const SERVER: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 5), 443);

fn tcp(
    src: SocketAddrV4,
    dst: SocketAddrV4,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> Vec<u8> {
    let total = (40 + payload.len()) as u16;
    let mut p = vec![0x45, 0];
    p.extend_from_slice(&total.to_be_bytes());
    p.extend_from_slice(&[0, 0, 0x40, 0, 64, 6, 0, 0]);
    p.extend_from_slice(&src.ip().octets());
    p.extend_from_slice(&dst.ip().octets());
    p.extend_from_slice(&src.port().to_be_bytes());
    p.extend_from_slice(&dst.port().to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&ack.to_be_bytes());
    p.extend_from_slice(&[0x50, flags, 0xFF, 0xFF, 0, 0, 0, 0]);
    p.extend_from_slice(payload);
    p
}

fn only(t: &Tally) -> FlowCounters {
    let rows = t.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows[0].clone()
}

/// SYN, SYN-ACK, then `n` bytes of response in 1000-byte segments.
fn connected(n: usize, now: Instant) -> Tally {
    let mut t = Tally::new(None);
    t.outbound(&tcp(ME, SERVER, 100, 0, SYN, &[]), now);
    t.inbound(
        &tcp(SERVER, ME, 900, 101, SYN | ACK, &[]),
        now + Duration::from_millis(40),
    );
    let mut seq = 901;
    for chunk in vec![b'x'; n].chunks(1000) {
        t.inbound(&tcp(SERVER, ME, seq, 101, ACK, chunk), now);
        seq += chunk.len() as u32;
    }
    t
}

#[test]
fn three_unanswered_syns_count() {
    let now = Instant::now();
    let mut t = Tally::new(None);
    for i in 0..3 {
        t.outbound(
            &tcp(ME, SERVER, 100, 0, SYN, &[]),
            now + Duration::from_secs(i),
        );
    }
    let row = only(&t);
    assert_eq!((row.syns, row.syn_answered), (3, false), "{row:?}");
    assert_eq!(row.protocol, Protocol::Tcp);
    assert_eq!(row.dst, "203.0.113.5:443");
    assert_eq!(row.local_port, 50000);
    assert_eq!(row.up_packets, 3);
}

#[test]
fn a_syn_ack_answers_and_times_the_syn() {
    let now = Instant::now();
    let row = only(&connected(0, now));
    assert!(row.syn_answered);
    assert_eq!(row.syn_ack_ms, Some(40));
    assert_eq!(row.rtt_ms, vec![40]);
    assert_eq!((row.up_packets, row.down_packets), (1, 1));
}

#[test]
fn rst_in_first_4k_is_early() {
    let now = Instant::now();
    let mut t = connected(1000, now);
    t.inbound(&tcp(SERVER, ME, 1901, 101, RST, &[]), now);
    assert!(only(&t).rst_early);
}

#[test]
fn rst_after_4k_is_not() {
    let now = Instant::now();
    let mut t = connected(5000, now);
    t.inbound(&tcp(SERVER, ME, 5901, 101, RST, &[]), now);
    assert!(!only(&t).rst_early);
}

#[test]
fn http_redirect_in_first_4k() {
    let now = Instant::now();
    let server = SocketAddrV4::new(*SERVER.ip(), 80);
    let mut t = Tally::new(None);
    t.outbound(&tcp(ME, server, 100, 0, SYN, &[]), now);
    t.inbound(&tcp(server, ME, 900, 101, SYN | ACK, &[]), now);
    t.outbound(
        &tcp(
            ME,
            server,
            101,
            901,
            ACK,
            b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
        ),
        now,
    );
    t.inbound(
        &tcp(
            server,
            ME,
            901,
            140,
            ACK,
            b"HTTP/1.1 302 Found\r\nLocation: http://warning.rt.ru/\r\n\r\n",
        ),
        now,
    );
    let row = only(&t);
    assert!(row.redirect_early);
    assert_eq!(
        row.sni.as_deref(),
        Some("example.com"),
        "Host of plain HTTP"
    );
}

#[test]
fn data_retrans_only_within_32k() {
    let now = Instant::now();
    let mut t = connected(0, now);
    let data = [b'd'; 1000];
    let mut seq = 101u32;
    for _ in 0..40 {
        t.outbound(&tcp(ME, SERVER, seq, 901, ACK, &data), now);
        seq += 1000;
    }
    // Resent: bytes 0..1000 and 2000..3000 (within 32 KB), 35000..36000 (beyond).
    for at in [101, 2101, 35101] {
        t.outbound(&tcp(ME, SERVER, at, 901, ACK, &data), now);
    }
    assert_eq!(only(&t).data_retrans, 2);
}

#[test]
fn an_ack_times_the_data() {
    let now = Instant::now();
    let mut t = connected(0, now);
    t.outbound(&tcp(ME, SERVER, 101, 901, ACK, &[b'd'; 100]), now);
    t.inbound(
        &tcp(SERVER, ME, 901, 201, ACK, &[]),
        now + Duration::from_millis(70),
    );
    assert_eq!(only(&t).rtt_ms, vec![40, 70]);
}

#[test]
fn udp_silent_windows() {
    let start = Instant::now();
    let server = SocketAddrV4::new(Ipv4Addr::new(35, 217, 1, 1), 50001);
    let mut t = Tally::new(None);
    for w in 0..5u64 {
        let now = start + Duration::from_secs(w);
        for i in 0..4 {
            t.outbound(&build_udp(ME, server, b"ping", i), now);
        }
        if w >= 3 {
            t.inbound(
                &build_udp(server, ME, b"pong", 0),
                now + Duration::from_millis(30),
            );
        }
        t.tick(now + Duration::from_millis(1000));
    }
    let row = only(&t);
    assert_eq!(row.protocol, Protocol::Udp);
    assert_eq!((row.udp_windows, row.udp_silent_windows), (5, 3), "{row:?}");
    assert_eq!(row.udp_first_reply_ms, Some(3030));
    assert_eq!((row.up_packets, row.down_packets), (20, 2));
}

/// A TLS 1.2-style ClientHello carrying `name` in its server_name extension.
fn client_hello(name: &str) -> Vec<u8> {
    let n = name.as_bytes();
    let mut sni = vec![0, 0]; // extension type server_name
    let list = 3 + n.len();
    sni.extend_from_slice(&((list + 2) as u16).to_be_bytes());
    sni.extend_from_slice(&(list as u16).to_be_bytes());
    sni.push(0); // host_name
    sni.extend_from_slice(&(n.len() as u16).to_be_bytes());
    sni.extend_from_slice(n);
    let mut ext = vec![0x00, 0x17, 0, 0]; // extended_master_secret, empty, before SNI
    ext.extend_from_slice(&sni);
    let mut body = vec![3, 3];
    body.extend_from_slice(&[7; 32]); // random
    body.push(0); // session id
    body.extend_from_slice(&[0, 2, 0x13, 0x01]); // one cipher suite
    body.extend_from_slice(&[1, 0]); // null compression
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    let mut hs = vec![1, 0];
    hs.extend_from_slice(&(body.len() as u16).to_be_bytes());
    hs.extend_from_slice(&body);
    let mut rec = vec![0x16, 3, 1];
    rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    rec.extend_from_slice(&hs);
    rec
}

#[test]
fn sni_from_client_hello() {
    let now = Instant::now();
    let mut t = connected(0, now);
    t.outbound(
        &tcp(ME, SERVER, 101, 901, ACK, &client_hello("discord.com")),
        now,
    );
    assert_eq!(only(&t).sni.as_deref(), Some("discord.com"));
}

#[test]
fn a_client_hello_split_in_two_segments_still_gives_sni() {
    let now = Instant::now();
    let mut t = connected(0, now);
    let hello = client_hello("discord.com");
    let (a, b) = hello.split_at(hello.len() - 5);
    t.outbound(&tcp(ME, SERVER, 101, 901, ACK, a), now);
    t.outbound(&tcp(ME, SERVER, 101 + a.len() as u32, 901, ACK, b), now);
    assert_eq!(only(&t).sni.as_deref(), Some("discord.com"));
}

#[test]
fn loopback_proxy_port_marks_system_proxy() {
    let now = Instant::now();
    let proxy = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 2080);
    let me = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 50001);
    let mut t = Tally::new(Some(2080));
    t.outbound(&tcp(me, proxy, 100, 0, SYN, &[]), now);
    t.outbound(
        &tcp(
            me,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000),
            100,
            0,
            SYN,
            &[],
        ),
        now,
    );
    let row = only(&t);
    assert!(row.via_system_proxy, "{row:?}");
    assert_eq!(row.dst, "127.0.0.1:2080");
    assert_eq!(
        (row.syns, row.up_packets),
        (0, 0),
        "no counters through the proxy"
    );
    assert!(!only(&connected(0, now)).via_system_proxy);
}

#[test]
fn ipv6_flows_have_no_syn_counters() {
    let now = Instant::now();
    let mut p = vec![0x60, 0, 0, 0, 0, 20, 6, 64];
    p.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8].repeat(4)); // src
    p.extend_from_slice(&[0x26, 0x06, 0x47, 0x00].repeat(4)); // dst
    p.extend_from_slice(&50000u16.to_be_bytes());
    p.extend_from_slice(&443u16.to_be_bytes());
    p.extend_from_slice(&[0; 8]);
    p.extend_from_slice(&[0x50, SYN, 0xFF, 0xFF, 0, 0, 0, 0]);
    let mut t = Tally::new(None);
    t.outbound(&p, now);
    let row = only(&t);
    assert!(row.ipv6);
    assert_eq!(
        (row.protocol, row.local_port, row.up_packets, row.syns),
        (Protocol::Tcp, 50000, 1, 0)
    );
    assert_eq!(row.dst, "[2606:4700:2606:4700:2606:4700:2606:4700]:443");
}

#[test]
fn sniff_filter_compresses_to_budget() {
    let ports: Vec<u16> = (0..300).map(|i| 10000 + 7 * i).collect();
    let text = sniff_filter(&ports).unwrap();
    assert!(
        text.starts_with("(ip or ipv6) and (tcp or udp) and ("),
        "{text}"
    );
    assert!(
        text.matches("localPort").count() <= MAX_FILTER_RANGES,
        "{text}"
    );
    assert!(
        text.contains("localPort >= 10000") && text.contains("12093"),
        "{text}"
    );
}

#[test]
fn few_ports_stay_exact() {
    assert_eq!(
        sniff_filter(&[443, 80, 443]).unwrap(),
        "(ip or ipv6) and (tcp or udp) and (localPort == 80 or localPort == 443)"
    );
}

#[test]
fn empty_ports_no_filter() {
    assert_eq!(sniff_filter(&[]), None);
}

#[test]
fn a_udp_socket_does_not_claim_a_tcp_flow_of_its_port() {
    let now = Instant::now();
    let mut t = Tally::new(None);
    t.outbound(&tcp(ME, SERVER, 1, 0, SYN, b""), now);
    t.outbound(&build_udp(ME, SERVER, b"x", 1), now);
    let ports = [(Protocol::Udp, ME.port())].into_iter().collect();
    let rows = of_programs(t.rows(), &ports);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].protocol, Protocol::Udp);
}

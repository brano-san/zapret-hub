use router::divert::Address;
use router::packet::{build_udp, parse_socks_udp, parse_udp, socks_header};

#[test]
fn a_built_packet_has_the_scripts_bytes_and_parses_back() {
    let p = build_udp(
        "1.2.3.4:50001".parse().unwrap(),
        "192.168.1.5:61000".parse().unwrap(),
        b"hi",
        7,
    );
    assert_eq!(p.len(), 30);
    assert_eq!(p[..12], [0x45, 0, 0, 30, 0, 7, 0x40, 0, 64, 17, 0, 0]);
    assert_eq!(p[12..20], [1, 2, 3, 4, 192, 168, 1, 5]);
    assert_eq!(p[20..28], [0xC3, 0x51, 0xEE, 0x48, 0, 10, 0, 0]);
    let udp = parse_udp(&p).unwrap();
    assert_eq!(
        (udp.src.to_string(), udp.dst.to_string(), udp.payload),
        (
            "1.2.3.4:50001".into(),
            "192.168.1.5:61000".into(),
            &b"hi"[..]
        )
    );
}

#[test]
fn not_udp_fragments_and_short_packets_do_not_parse() {
    let p = build_udp(
        "1.2.3.4:1".parse().unwrap(),
        "5.6.7.8:2".parse().unwrap(),
        b"x",
        0,
    );
    let mut tcp = p.clone();
    tcp[9] = 6;
    let mut fragment = p.clone();
    fragment[6] = 0x20; // MF
    assert!(parse_udp(&tcp).is_none() && parse_udp(&fragment).is_none());
    assert!(parse_udp(&p[..27]).is_none() && parse_udp(&[0x60; 40]).is_none());
}

#[test]
fn the_socks_udp_header_names_the_destination() {
    assert_eq!(
        socks_header("35.217.1.2:50005".parse().unwrap()),
        [0, 0, 0, 1, 35, 217, 1, 2, 0xC3, 0x55]
    );
    let datagram = [0, 0, 0, 1, 35, 217, 1, 2, 0xC3, 0x55, b'o', b'k'];
    let (from, data) = parse_socks_udp(&datagram).unwrap();
    assert_eq!(
        (from.to_string(), data),
        ("35.217.1.2:50005".into(), &b"ok"[..])
    );
    let mut fragment = datagram;
    fragment[2] = 1;
    let mut domain = datagram;
    domain[3] = 3;
    assert!(parse_socks_udp(&fragment).is_none() && parse_socks_udp(&domain).is_none());
    assert!(parse_socks_udp(&datagram[..9]).is_none());
}

#[test]
fn an_outbound_address_turns_inbound_and_socket_fields_read() {
    let mut raw = [0u8; 80];
    raw[8] = 3;
    raw[9] = 4;
    raw[10] = 0b0000_1011;
    raw[16] = 0xAA;
    raw[32..36].copy_from_slice(&1234u32.to_le_bytes());
    raw[68..70].copy_from_slice(&50001u16.to_le_bytes());
    raw[72] = 17;
    let addr = Address(raw);
    assert!(addr.outbound());
    let inbound = addr.as_inbound();
    assert!(!inbound.outbound());
    assert_eq!(inbound.0[10], 0b0000_0001);
    assert_eq!(
        (&inbound.0[..10], &inbound.0[11..]),
        (&raw[..10], &raw[11..])
    );
    assert_eq!(
        (
            addr.event(),
            addr.socket_pid(),
            addr.socket_local_port(),
            addr.socket_protocol()
        ),
        (4, 1234, 50001, 17)
    );
}

#[test]
fn only_unicast_packets_are_routed() {
    use router::packet::unicast;
    let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
    assert!(unicast(ip("192.168.1.5"), ip("104.29.1.1")));
    assert!(!unicast(ip("192.168.1.5"), ip("224.0.0.251")), "mDNS");
    assert!(!unicast(ip("192.168.1.5"), ip("239.255.255.250")), "SSDP");
    assert!(!unicast(ip("192.168.1.5"), ip("255.255.255.255")));
    assert!(!unicast(ip("0.0.0.0"), ip("255.255.255.255")), "DHCP");
    assert!(!unicast(ip("0.0.0.0"), ip("104.29.1.1")));
}

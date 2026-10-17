use router::divert::Address;
use router::packet::{
    ACK, SYN, build_udp, parse_socks_udp, parse_socks_udp_from, parse_tcp, parse_udp, rewrite_tcp,
    socks_header, socks_header_to,
};

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

/// 192.168.1.5:50000 -> 162.159.135.232:443, SYN, data offset 6 (an MSS option), checksums 0x1234 / 0xABCD.
fn syn() -> Vec<u8> {
    let mut p = vec![
        0x45, 0, 0, 44, 0, 1, 0x40, 0, 128, 6, 0xAB, 0xCD, 192, 168, 1, 5, 162, 159, 135, 232,
    ];
    p.extend_from_slice(&[
        0xC3, 0x50, 0x01, 0xBB, 0, 0, 0, 1, 0, 0, 0, 0, 0x60, 0x02, 0xFA, 0xF0, 0x12, 0x34, 0, 0,
    ]);
    p.extend_from_slice(&[2, 4, 5, 0xB4]);
    p
}

#[test]
fn a_tcp_segment_parses_with_its_flags_and_options() {
    let p = syn();
    let t = parse_tcp(&p).unwrap();
    assert_eq!(
        (t.src.to_string(), t.dst.to_string()),
        ("192.168.1.5:50000".into(), "162.159.135.232:443".into())
    );
    assert_eq!(t.flags & (SYN | ACK), SYN);
    assert!(t.payload.is_empty(), "options are not payload");
    let mut data = p.clone();
    data.extend_from_slice(b"GET");
    data[3] = 47;
    assert_eq!(parse_tcp(&data).unwrap().payload, b"GET");
}

#[test]
fn bad_tcp_does_not_parse() {
    let p = syn();
    let (mut udp, mut fragment, mut short, mut long) = (p.clone(), p.clone(), p.clone(), p.clone());
    udp[9] = 17;
    fragment[7] = 1;
    short[32] = 0x40; // data offset 4
    long[32] = 0xF0; // 60 bytes, past the packet
    for bad in [udp, fragment, short, long, p[..39].to_vec()] {
        assert!(parse_tcp(&bad).is_none());
    }
}

#[test]
fn a_reflected_segment_has_new_addresses_and_zero_checksums() {
    let mut p = syn();
    rewrite_tcp(
        &mut p,
        "162.159.135.232:50000".parse().unwrap(),
        "192.168.1.5:61000".parse().unwrap(),
    );
    assert_eq!(p[12..20], [162, 159, 135, 232, 192, 168, 1, 5]);
    assert_eq!(p[20..24], [0xC3, 0x50, 0xEE, 0x48]);
    assert_eq!(
        (&p[10..12], &p[36..38]),
        (&[0u8, 0][..], &[0u8, 0][..]),
        "checksums are left to WinDivert"
    );
    assert_eq!(
        (&p[24..36], &p[38..]),
        (&syn()[24..36], &syn()[38..]),
        "sequence, flags, window and options stay"
    );
}

#[test]
fn a_known_domain_goes_into_the_socks_udp_header() {
    let dst = "35.214.1.1:50005".parse().unwrap();
    assert_eq!(socks_header_to(dst, None), socks_header(dst).to_vec());
    let named = [&[0u8, 0, 0, 3, 15][..], b"a.discord.media", &[0xC3, 0x55]].concat();
    assert_eq!(socks_header_to(dst, Some("a.discord.media")), named);
    let reply = [named.as_slice(), b"ok"].concat();
    assert_eq!(parse_socks_udp_from(&reply, dst), Some((dst, &b"ok"[..])));
    assert_eq!(parse_socks_udp_from(&reply[..12], dst), None, "a cut name");
    let ipv4 = [0, 0, 0, 1, 35, 214, 1, 1, 0xC3, 0x55, b'x'];
    assert_eq!(
        parse_socks_udp_from(&ipv4, "1.1.1.1:1".parse().unwrap()),
        Some((dst, &b"x"[..]))
    );
}

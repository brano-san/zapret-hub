use router::divert::Address;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};

use router::packet::{
    ACK, SYN, build_udp, build_udp_any, build_udp6, ipv6_upper, parse_socks_udp,
    parse_socks_udp_from, parse_tcp, parse_udp, parse_udp_any, parse_udp6, rewrite_tcp,
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
    let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
    assert!(unicast(ip("192.168.1.5"), ip("104.29.1.1")));
    assert!(!unicast(ip("192.168.1.5"), ip("224.0.0.251")), "mDNS");
    assert!(!unicast(ip("192.168.1.5"), ip("239.255.255.250")), "SSDP");
    assert!(!unicast(ip("192.168.1.5"), ip("255.255.255.255")));
    assert!(!unicast(ip("0.0.0.0"), ip("255.255.255.255")), "DHCP");
    assert!(!unicast(ip("0.0.0.0"), ip("104.29.1.1")));
    assert!(unicast(ip("2a02:6b8::5"), ip("2606:4700::1")));
    assert!(!unicast(ip("fe80::1"), ip("ff02::fb")), "IPv6 mDNS");
    assert!(!unicast(ip("::"), ip("ff02::1:2")), "DHCPv6");
    assert!(!unicast(ip("::"), ip("2606:4700::1")));
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
    let v4: SocketAddrV4 = "35.214.1.1:50005".parse().unwrap();
    let dst = SocketAddr::V4(v4);
    assert_eq!(socks_header_to(dst, None), socks_header(v4).to_vec());
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

#[test]
fn an_ipv6_destination_is_address_type_4_both_ways() {
    let dst: SocketAddr = "[2001:db8::7]:443".parse().unwrap();
    let mut header = vec![0u8, 0, 0, 4, 0x20, 0x01, 0x0d, 0xb8];
    header.extend([0; 11]);
    header.extend([7, 0x01, 0xBB]);
    assert_eq!(socks_header_to(dst, None), header);
    let datagram = [header.as_slice(), b"ok"].concat();
    assert_eq!(parse_socks_udp(&datagram), Some((dst, &b"ok"[..])));
    assert_eq!(
        parse_socks_udp_from(&datagram, "1.1.1.1:1".parse().unwrap()),
        Some((dst, &b"ok"[..]))
    );
    assert_eq!(parse_socks_udp(&datagram[..21]), None, "a cut address");
    let named = socks_header_to(dst, Some("a.discord.media"));
    assert_eq!(
        &named[..5],
        [0, 0, 0, 3, 15],
        "a known name still goes by name"
    );
}

#[test]
fn an_ipv6_udp_packet_builds_and_parses_back() {
    let src: SocketAddrV6 = "[fd00::1]:40000".parse().unwrap();
    let dst: SocketAddrV6 = "[2001:db8::53]:53".parse().unwrap();
    let p = build_udp6(src, dst, b"zapret-hub");
    assert_eq!(p.len(), 40 + 8 + 10);
    assert_eq!(&p[..8], [0x60, 0, 0, 0, 0, 18, 17, 64]);
    let u = parse_udp6(&p).unwrap();
    assert_eq!((u.src, u.dst, u.payload), (src, dst, &b"zapret-hub"[..]));
    assert_eq!(
        parse_udp_any(&p),
        Some((src.into(), dst.into(), &b"zapret-hub"[..]))
    );
    assert!(parse_udp(&p).is_none(), "not IPv4");
    assert!(parse_udp6(&p[..p.len() - 1]).is_none(), "truncated");
    let mut tcp = p.clone();
    tcp[6] = 6;
    assert!(parse_udp6(&tcp).is_none() && parse_udp_any(&tcp).is_none());
    let mut hop_by_hop = p.clone();
    hop_by_hop[6] = 0;
    assert!(
        parse_udp6(&hop_by_hop).is_none(),
        "extension headers are not read"
    );
    let v4 = build_udp(
        "10.0.0.1:1".parse().unwrap(),
        "10.0.0.2:53".parse().unwrap(),
        b"x",
        1,
    );
    assert_eq!(
        parse_udp_any(&v4).map(|(_, d, x)| (d.to_string(), x)),
        Some(("10.0.0.2:53".into(), &b"x"[..]))
    );
}

/// [2a02:6b8::5]:50000 -> [2606:4700::1]:443, SYN, data offset 6 (an MSS option), checksum 0x1234.
fn syn6() -> Vec<u8> {
    let mut p = vec![0x60, 0, 0, 0, 0, 24, 6, 64];
    p.extend_from_slice(
        &"2a02:6b8::5"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    p.extend_from_slice(
        &"2606:4700::1"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    p.extend_from_slice(&[
        0xC3, 0x50, 0x01, 0xBB, 0, 0, 0, 1, 0, 0, 0, 0, 0x60, 0x02, 0xFA, 0xF0, 0x12, 0x34, 0, 0,
    ]);
    p.extend_from_slice(&[2, 4, 5, 0xA0]);
    p
}

#[test]
fn an_ipv6_tcp_segment_parses_and_rewrites() {
    let p = syn6();
    let t = parse_tcp(&p).unwrap();
    assert_eq!(
        (t.src.to_string(), t.dst.to_string()),
        ("[2a02:6b8::5]:50000".into(), "[2606:4700::1]:443".into())
    );
    assert_eq!(t.flags & (SYN | ACK), SYN);
    assert!(t.payload.is_empty(), "options are not payload");
    let mut data = p.clone();
    data.extend_from_slice(b"GET");
    data[5] = 27;
    assert_eq!(parse_tcp(&data).unwrap().payload, b"GET");
    let (mut udp, mut short, mut long, mut hop) = (p.clone(), p.clone(), p.clone(), p.clone());
    udp[6] = 17;
    short[52] = 0x40;
    long[52] = 0xF0;
    hop[6] = 0;
    for bad in [udp, short, long, hop, p[..59].to_vec()] {
        assert!(parse_tcp(&bad).is_none());
    }
    let mut r = p.clone();
    rewrite_tcp(
        &mut r,
        "[2606:4700::1]:50000".parse().unwrap(),
        "[2a02:6b8::5]:61000".parse().unwrap(),
    );
    assert_eq!(
        (&r[8..24], &r[24..40]),
        (&p[24..40], &p[8..24]),
        "addresses swap"
    );
    assert_eq!(r[40..44], [0xC3, 0x50, 0xEE, 0x48]);
    assert_eq!(
        &r[56..58],
        &[0u8, 0][..],
        "the checksum is left to WinDivert"
    );
    assert_eq!(
        (&r[..8], &r[44..56], &r[58..]),
        (&p[..8], &p[44..56], &p[58..]),
        "header, sequence, flags, window and options stay"
    );
}

#[test]
fn a_udp_reply_is_built_in_the_flows_family() {
    let v6 = build_udp_any(
        "[2001:db8::53]:53".parse().unwrap(),
        "[fd00::1]:40000".parse().unwrap(),
        b"x",
        9,
    )
    .unwrap();
    assert_eq!(v6[0] >> 4, 6);
    let v4 = build_udp_any(
        "1.2.3.4:53".parse().unwrap(),
        "10.0.0.1:40000".parse().unwrap(),
        b"x",
        9,
    )
    .unwrap();
    assert_eq!(&v4[4..6], &[0, 9][..], "the IPv4 id");
    assert!(
        build_udp_any(
            "1.2.3.4:53".parse().unwrap(),
            "[fd00::1]:40000".parse().unwrap(),
            b"x",
            9
        )
        .is_none()
    );
}

/// `p` (IPv6) with an extension header of `kind` (`hdr`, its next header filled in) after
/// the fixed header.
fn with_ext(p: &[u8], kind: u8, hdr: &[u8]) -> Vec<u8> {
    let mut hdr = hdr.to_vec();
    hdr[0] = p[6];
    let mut q = p[..40].to_vec();
    q[6] = kind;
    let len = u16::from_be_bytes([p[4], p[5]]) + hdr.len() as u16;
    q[4..6].copy_from_slice(&len.to_be_bytes());
    q.extend_from_slice(&hdr);
    q.extend_from_slice(&p[40..]);
    q
}

#[test]
fn ipv6_extension_headers_are_skipped() {
    let pad = [0u8, 0, 1, 4, 0, 0, 0, 0];
    let routing = [0u8, 0, 4, 0, 0, 0, 0, 0];
    let atomic = [0u8, 0, 0, 0, 0, 0, 0, 7];
    let udp = build_udp6(
        "[2001:db8::53]:53".parse().unwrap(),
        "[fd00::1]:40000".parse().unwrap(),
        b"dns",
    );
    let hop = with_ext(&udp, 0, &pad);
    let chained = with_ext(&with_ext(&with_ext(&udp, 60, &pad), 43, &routing), 0, &pad);
    for p in [&hop, &chained, &with_ext(&udp, 44, &atomic)] {
        let u = parse_udp6(p).unwrap();
        assert_eq!((u.dst.port(), u.payload), (40000, &b"dns"[..]), "{p:02x?}");
    }
    assert_eq!(ipv6_upper(&chained), Some((17, 64, false)));
    let mut first = atomic;
    first[3] = 1; // more fragments
    let mut later = atomic;
    later[2] = 0x08; // offset 1
    assert_eq!(
        ipv6_upper(&with_ext(&udp, 44, &first)),
        Some((17, 48, true))
    );
    assert!(
        parse_udp6(&with_ext(&udp, 44, &first)).is_none(),
        "an incomplete datagram"
    );
    assert_eq!(
        ipv6_upper(&with_ext(&udp, 44, &later)),
        None,
        "a later fragment"
    );
    assert!(ipv6_upper(&hop[..44]).is_none(), "a cut header");
    let mut unknown = hop.clone();
    unknown[6] = 135; // mobility: not read
    assert!(parse_udp6(&unknown).is_none());

    let tcp = with_ext(&syn6(), 60, &pad);
    let t = parse_tcp(&tcp).unwrap();
    assert_eq!((t.dst.port(), t.flags & (SYN | ACK)), (443, SYN));
    let mut r = tcp.clone();
    rewrite_tcp(
        &mut r,
        "[2606:4700::1]:50000".parse().unwrap(),
        "[2a02:6b8::5]:61000".parse().unwrap(),
    );
    assert_eq!(&r[40..48], &tcp[40..48], "the extension header stays");
    assert_eq!(r[48..52], [0xC3, 0x50, 0xEE, 0x48], "ports past it");
    assert_eq!(&r[64..66], &[0u8, 0][..], "its checksum");
    assert!(
        parse_tcp(&with_ext(&syn6(), 44, &first)).is_none(),
        "a first fragment"
    );
}

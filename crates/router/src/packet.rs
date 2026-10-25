//! IPv4 and IPv6 UDP packets and the SOCKS5 UDP request header (RFC 1928, section 7).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

pub struct Udp<'a> {
    pub src: SocketAddrV4,
    pub dst: SocketAddrV4,
    pub payload: &'a [u8],
}

/// Multicast, broadcast and DHCP (from 0.0.0.0) packets are never relayed: they go out unchanged.
pub fn unicast(src: Ipv4Addr, dst: Ipv4Addr) -> bool {
    !(dst.is_multicast() || dst.is_broadcast() || src.is_unspecified())
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

/// Unfragmented IPv4/UDP only; lengths must agree with the buffer.
pub fn parse_udp(packet: &[u8]) -> Option<Udp<'_>> {
    if packet.len() < 28 || packet[0] >> 4 != 4 || packet[9] != 17 {
        return None;
    }
    let ihl = usize::from(packet[0] & 0x0F) * 4;
    let total = usize::from(be16(packet, 2));
    if ihl < 20 || total > packet.len() || total < ihl + 8 || be16(packet, 6) & 0x3FFF != 0 {
        return None;
    }
    let udp_len = usize::from(be16(packet, ihl + 4));
    if udp_len < 8 || ihl + udp_len > total {
        return None;
    }
    let addr = |at: usize, port_at: usize| {
        SocketAddrV4::new(
            Ipv4Addr::new(packet[at], packet[at + 1], packet[at + 2], packet[at + 3]),
            be16(packet, port_at),
        )
    };
    Some(Udp {
        src: addr(12, ihl),
        dst: addr(16, ihl + 2),
        payload: &packet[ihl + 8..ihl + udp_len],
    })
}

/// Checksums stay zero: `WinDivertHelperCalcChecksums` fills them in.
pub fn build_udp(src: SocketAddrV4, dst: SocketAddrV4, payload: &[u8], id: u16) -> Vec<u8> {
    let total = u16::try_from(28 + payload.len()).expect("UDP payload fits an IPv4 packet");
    let mut p = Vec::with_capacity(usize::from(total));
    p.extend_from_slice(&[0x45, 0]);
    p.extend_from_slice(&total.to_be_bytes());
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&[0x40, 0, 64, 17, 0, 0]);
    p.extend_from_slice(&src.ip().octets());
    p.extend_from_slice(&dst.ip().octets());
    p.extend_from_slice(&src.port().to_be_bytes());
    p.extend_from_slice(&dst.port().to_be_bytes());
    p.extend_from_slice(&(total - 20).to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(payload);
    p
}

pub struct Udp6<'a> {
    pub src: SocketAddrV6,
    pub dst: SocketAddrV6,
    pub payload: &'a [u8],
}

/// IPv6/UDP right after the fixed header (no extension headers); lengths must agree with the
/// buffer.
pub fn parse_udp6(packet: &[u8]) -> Option<Udp6<'_>> {
    if packet.len() < 48 || packet[0] >> 4 != 6 || packet[6] != 17 {
        return None;
    }
    let total = 40 + usize::from(be16(packet, 4));
    let udp_len = usize::from(be16(packet, 44));
    if total > packet.len() || udp_len < 8 || 40 + udp_len > total {
        return None;
    }
    let addr = |at: usize, port_at: usize| {
        let ip: [u8; 16] = packet[at..at + 16].try_into().unwrap();
        SocketAddrV6::new(Ipv6Addr::from(ip), be16(packet, port_at), 0, 0)
    };
    Some(Udp6 {
        src: addr(8, 40),
        dst: addr(24, 42),
        payload: &packet[48..40 + udp_len],
    })
}

/// The checksum stays zero (mandatory in IPv6: the sender fills it in).
pub fn build_udp6(src: SocketAddrV6, dst: SocketAddrV6, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(8 + payload.len()).expect("UDP payload fits an IPv6 packet");
    let mut p = Vec::with_capacity(40 + usize::from(len));
    p.extend_from_slice(&[0x60, 0, 0, 0]);
    p.extend_from_slice(&len.to_be_bytes());
    p.extend_from_slice(&[17, 64]);
    p.extend_from_slice(&src.ip().octets());
    p.extend_from_slice(&dst.ip().octets());
    p.extend_from_slice(&src.port().to_be_bytes());
    p.extend_from_slice(&dst.port().to_be_bytes());
    p.extend_from_slice(&len.to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(payload);
    p
}

/// Source, destination and payload of an IPv4 or IPv6 UDP packet (`parse_udp`, `parse_udp6`).
pub fn parse_udp_any(packet: &[u8]) -> Option<(SocketAddr, SocketAddr, &[u8])> {
    match packet.first()? >> 4 {
        4 => parse_udp(packet).map(|u| (u.src.into(), u.dst.into(), u.payload)),
        6 => parse_udp6(packet).map(|u| (u.src.into(), u.dst.into(), u.payload)),
        _ => None,
    }
}

pub fn socks_header(dst: SocketAddrV4) -> [u8; 10] {
    let o = dst.ip().octets();
    let p = dst.port().to_be_bytes();
    [0, 0, 0, 1, o[0], o[1], o[2], o[3], p[0], p[1]]
}

/// Address type 1 (IPv4) or 4 (IPv6), no fragmentation; anything else is dropped.
pub fn parse_socks_udp(datagram: &[u8]) -> Option<(SocketAddr, &[u8])> {
    match datagram {
        [0, 0, 0, 1, a, b, c, d, p0, p1, data @ ..] => Some((
            SocketAddrV4::new(
                Ipv4Addr::new(*a, *b, *c, *d),
                u16::from_be_bytes([*p0, *p1]),
            )
            .into(),
            data,
        )),
        [0, 0, 0, 4, rest @ ..] if rest.len() >= 18 => {
            let ip: [u8; 16] = rest[..16].try_into().unwrap();
            let port = u16::from_be_bytes([rest[16], rest[17]]);
            Some((
                SocketAddr::new(Ipv6Addr::from(ip).into(), port),
                &rest[18..],
            ))
        }
        _ => None,
    }
}

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const ACK: u8 = 0x10;

pub struct Tcp<'a> {
    pub src: SocketAddrV4,
    pub dst: SocketAddrV4,
    pub flags: u8,
    pub payload: &'a [u8],
}

/// Unfragmented IPv4/TCP only; header and option lengths must fit the packet.
pub fn parse_tcp(packet: &[u8]) -> Option<Tcp<'_>> {
    if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
        return None;
    }
    let ihl = usize::from(packet[0] & 0x0F) * 4;
    let total = usize::from(be16(packet, 2));
    if ihl < 20 || total > packet.len() || total < ihl + 20 || be16(packet, 6) & 0x3FFF != 0 {
        return None;
    }
    let doff = usize::from(packet[ihl + 12] >> 4) * 4;
    if doff < 20 || ihl + doff > total {
        return None;
    }
    let addr = |at: usize, port_at: usize| {
        SocketAddrV4::new(
            Ipv4Addr::new(packet[at], packet[at + 1], packet[at + 2], packet[at + 3]),
            be16(packet, port_at),
        )
    };
    Some(Tcp {
        src: addr(12, ihl),
        dst: addr(16, ihl + 2),
        flags: packet[ihl + 13],
        payload: &packet[ihl + doff..total],
    })
}

/// For a packet that passed `parse_tcp`. Checksums are zeroed: `WinDivertHelperCalcChecksums` fills them in.
pub fn rewrite_tcp(packet: &mut [u8], src: SocketAddrV4, dst: SocketAddrV4) {
    let ihl = usize::from(packet[0] & 0x0F) * 4;
    packet[12..16].copy_from_slice(&src.ip().octets());
    packet[16..20].copy_from_slice(&dst.ip().octets());
    packet[ihl..ihl + 2].copy_from_slice(&src.port().to_be_bytes());
    packet[ihl + 2..ihl + 4].copy_from_slice(&dst.port().to_be_bytes());
    packet[10..12].fill(0);
    packet[ihl + 16..ihl + 18].fill(0);
}

/// `None` is the plain address header (type 1 or 4); a domain goes in as address type 3.
pub fn socks_header_to(dst: SocketAddr, domain: Option<&str>) -> Vec<u8> {
    let Some(name) = domain else {
        return match dst {
            SocketAddr::V4(dst) => socks_header(dst).to_vec(),
            SocketAddr::V6(dst) => {
                let mut h = vec![0, 0, 0, 4];
                h.extend_from_slice(&dst.ip().octets());
                h.extend_from_slice(&dst.port().to_be_bytes());
                h
            }
        };
    };
    let len = u8::try_from(name.len()).expect("a domain is at most 253 bytes");
    let mut h = vec![0, 0, 0, 3, len];
    h.extend_from_slice(name.as_bytes());
    h.extend_from_slice(&dst.port().to_be_bytes());
    h
}

/// Like `parse_socks_udp`, plus address type 3: the client expects the reply from the address it sent to.
pub fn parse_socks_udp_from(datagram: &[u8], flow_dst: SocketAddr) -> Option<(SocketAddr, &[u8])> {
    match datagram {
        [0, 0, 0, 3, len, rest @ ..] => {
            let skip = usize::from(*len) + 2;
            (rest.len() >= skip).then(|| (flow_dst, &rest[skip..]))
        }
        _ => parse_socks_udp(datagram),
    }
}

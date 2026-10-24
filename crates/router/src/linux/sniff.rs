//! DNS answers read off every interface, `lo` included (systemd-resolved's stub answers programs
//! there), by an `AF_PACKET` socket (`CAP_NET_RAW`) for the domain rules. Datagram packet sockets
//! hand over the IP packet without its link header; a classic BPF filter keeps the kernel from
//! copying anything but IPv4 UDP from port 53.

use crate::dns::{self, Answer};
use crate::packet::parse_udp;

/// The DNS answer an IPv4 packet carries, if it is one (UDP from port 53).
pub fn answer(packet: &[u8]) -> Option<Answer> {
    parse_udp(packet)
        .filter(|u| u.src.port() == 53)
        .and_then(|u| dns::parse(u.payload))
}

#[cfg(target_os = "linux")]
mod sys {
    use std::io::{self, Read as _};
    use std::time::Duration;

    use socket2::{Domain, Protocol, SockFilter, Socket, Type};

    use crate::dns::Answer;

    /// `ip[9] == 17 && !(ip[6:2] & 0x1fff) && udp[0:2] == 53`, offsets from the IP header.
    const FILTER: [SockFilter; 9] = [
        SockFilter::new(0x30, 0, 0, 9),      // ldb [9]: protocol
        SockFilter::new(0x15, 0, 6, 17),     // jeq UDP, else drop
        SockFilter::new(0x28, 0, 0, 6),      // ldh [6]: fragment offset
        SockFilter::new(0x45, 4, 0, 0x1fff), // jset: a later fragment, drop
        SockFilter::new(0xb1, 0, 0, 0),      // ldxb 4*([0]&0xf): header length
        SockFilter::new(0x48, 0, 0, 0),      // ldh [x+0]: source port
        SockFilter::new(0x15, 0, 1, 53),     // jeq 53, else drop
        SockFilter::new(0x06, 0, 0, 0xffff), // accept
        SockFilter::new(0x06, 0, 0, 0),      // drop
    ];

    pub struct Sniffer(Socket);

    impl Sniffer {
        /// `next` gives up after `timeout`, so the reading thread can see it was stopped.
        pub fn open(timeout: Duration) -> io::Result<Sniffer> {
            let ip = (libc::ETH_P_IP as u16).to_be();
            let socket = Socket::new(
                Domain::PACKET,
                Type::DGRAM,
                Some(Protocol::from(i32::from(ip))),
            )?;
            socket.attach_filter(&FILTER)?;
            socket.set_read_timeout(Some(timeout))?;
            Ok(Sniffer(socket))
        }

        /// The next packet as an answer (`None`: not one, e.g. let in before the filter was
        /// attached). A timeout is `WouldBlock`/`TimedOut`. A loopback answer comes twice (out
        /// and in): `Names::record` takes it once.
        pub fn next(&self, buf: &mut [u8]) -> io::Result<Option<Answer>> {
            let n = (&self.0).read(buf)?;
            Ok(super::answer(&buf[..n]))
        }
    }
}

#[cfg(target_os = "linux")]
pub use sys::Sniffer;

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use super::*;
    use crate::packet::build_udp;

    /// An answer for `name` with one A record.
    fn response(name: &str, ip: [u8; 4]) -> Vec<u8> {
        let mut m = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        for label in name.split('.') {
            m.push(label.len() as u8);
            m.extend(label.as_bytes());
        }
        m.extend([0, 0, 1, 0, 1]);
        m.extend([0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        m.extend(ip);
        m
    }

    fn addr(a: [u8; 4], port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::from(a), port)
    }

    #[test]
    fn an_answer_from_port_53_is_read_and_anything_else_is_not() {
        let dns = response("discord.media", [1, 2, 3, 4]);
        let packet = build_udp(
            addr([127, 0, 0, 53], 53),
            addr([127, 0, 0, 1], 40000),
            &dns,
            1,
        );
        let answer = answer(&packet).unwrap();
        assert_eq!(answer.name, "discord.media");
        assert_eq!(answer.ips, [(Ipv4Addr::new(1, 2, 3, 4), 60)]);
        let query = build_udp(
            addr([127, 0, 0, 1], 40000),
            addr([127, 0, 0, 53], 53),
            &dns,
            1,
        );
        assert_eq!(super::answer(&query), None, "to port 53, not from it");
        assert_eq!(
            super::answer(&packet[..packet.len() - 1]),
            None,
            "truncated"
        );
        let mut tcp = packet.clone();
        tcp[9] = 6;
        assert_eq!(super::answer(&tcp), None, "not UDP");
    }

    /// The kernel side: socket, filter, loopback. Needs `CAP_NET_RAW` and an up `lo`, e.g.
    /// `unshare -rn sh -c 'ip link set lo up && <test binary> --ignored sniff'`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs CAP_NET_RAW (a user namespace will do)"]
    fn sniffer_reads_a_loopback_answer() {
        use std::net::UdpSocket;
        use std::time::Duration;
        let sniffer = Sniffer::open(Duration::from_millis(200)).unwrap();
        let server = UdpSocket::bind("127.0.0.1:53").unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(b"not an answer", "127.0.0.1:9").unwrap();
        server
            .send_to(
                &response("t.me", [5, 6, 7, 8]),
                client.local_addr().unwrap(),
            )
            .unwrap();
        let mut buf = vec![0; 65536];
        // The filter dropped the datagram to port 9: the answer comes first.
        let answer = sniffer.next(&mut buf).unwrap().unwrap();
        assert_eq!(answer.name, "t.me");
        assert_eq!(answer.ips, [(Ipv4Addr::new(5, 6, 7, 8), 60)]);
    }
}

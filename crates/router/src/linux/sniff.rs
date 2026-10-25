//! DNS answers read off every interface, `lo` included (systemd-resolved's stub answers programs
//! there), by an `AF_PACKET` socket (`CAP_NET_RAW`) for the domain rules. Datagram packet sockets
//! hand over the IP packet without its link header; a classic BPF filter keeps the kernel from
//! copying anything but IPv4 or IPv6 UDP to or from port 53. An answer counts only for a query
//! seen going out (`Asked`): a spoofed answer must guess the query's port and id, so another host
//! of the network cannot route chosen addresses through the proxy by sending fake answers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::dns::{self, Answer};
use crate::packet::parse_udp_any;

/// One query and its answer: who asked whom, with which id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Exchange {
    pub client: SocketAddr,
    pub server: SocketAddr,
    pub id: u16,
}

#[derive(Debug, PartialEq)]
pub enum Seen {
    Query(Exchange),
    Answer(Exchange, Answer),
}

/// The DNS query or answer an IPv4 or IPv6 packet carries (UDP to or from port 53).
pub fn seen(packet: &[u8]) -> Option<Seen> {
    let (src, dst, payload) = parse_udp_any(packet)?;
    let id = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    let query = *payload.get(2)? & 0x80 == 0;
    if dst.port() == 53 && query && payload.len() >= 12 {
        return Some(Seen::Query(Exchange {
            client: src,
            server: dst,
            id,
        }));
    }
    if src.port() != 53 {
        return None;
    }
    let exchange = Exchange {
        client: dst,
        server: src,
        id,
    };
    dns::parse(payload).map(|a| Seen::Answer(exchange, a))
}

/// How long an answer to a query is waited for.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);
/// Queries kept at most; older ones go first.
const MAX_ASKED: usize = 4096;

/// The queries seen lately, to accept only their answers.
#[derive(Default)]
pub struct Asked(HashMap<Exchange, Instant>);

impl Asked {
    pub fn ask(&mut self, e: Exchange, now: Instant) {
        if self.0.len() >= MAX_ASKED {
            self.0
                .retain(|_, at| now.duration_since(*at) < ANSWER_WITHIN);
            // ponytail: a flood of fresh queries clears the lot; their answers are then missed
            // until the flood stops.
            if self.0.len() >= MAX_ASKED {
                self.0.clear();
            }
        }
        self.0.insert(e, now);
    }

    /// `e` answers a query seen within `ANSWER_WITHIN`. Kept until it expires: a loopback answer
    /// comes twice (out and in).
    pub fn answers(&self, e: &Exchange, now: Instant) -> bool {
        self.0
            .get(e)
            .is_some_and(|at| now.duration_since(*at) < ANSWER_WITHIN)
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use std::io::{self, Read as _};
    use std::time::Duration;

    use socket2::{Domain, Protocol, SockFilter, Socket, Type};

    use super::Seen;

    /// IPv4: `ip[9] == 17 && !(ip[6:2] & 0x1fff)`, IPv6: `ip6[6] == 17` (UDP right after the
    /// fixed header); then `udp[0:2] == 53 || udp[2:2] == 53`. An IPv6 packet with a
    /// hop-by-hop, routing, destination options or fragment header passes whole: `seen` reads
    /// past them and checks the ports. Offsets from the IP header; the family by the packet's link
    /// protocol (the socket takes every protocol).
    const FILTER: [SockFilter; 24] = [
        SockFilter::new(0x28, 0, 0, 0xffff_f000), // 0 ldh proto (SKF_AD_PROTOCOL)
        SockFilter::new(0x15, 7, 0, 0x86dd),      // 1 jeq IPv6: to 9
        SockFilter::new(0x15, 0, 14, 0x0800),     // 2 jeq IPv4, else drop
        SockFilter::new(0x30, 0, 0, 9),           // 3 ldb [9]: protocol
        SockFilter::new(0x15, 0, 12, 17),         // 4 jeq UDP, else drop
        SockFilter::new(0x28, 0, 0, 6),           // 5 ldh [6]: fragment offset
        SockFilter::new(0x45, 10, 0, 0x1fff),     // 6 jset: a later fragment, drop
        SockFilter::new(0xb1, 0, 0, 0),           // 7 ldxb 4*([0]&0xf): header length
        SockFilter::new(0x05, 0, 0, 3),           // 8 ja 12: the ports
        SockFilter::new(0x30, 0, 0, 6),           // 9 ldb [6]: next header
        SockFilter::new(0x15, 0, 7, 17),          // 10 jeq UDP, else to 18
        SockFilter::new(0x01, 0, 0, 40),          // 11 ldx #40: header length
        SockFilter::new(0x48, 0, 0, 0),           // 12 ldh [x+0]: source port
        SockFilter::new(0x15, 2, 0, 53),          // 13 jeq 53: accept
        SockFilter::new(0x48, 0, 0, 2),           // 14 ldh [x+2]: destination port
        SockFilter::new(0x15, 0, 1, 53),          // 15 jeq 53, else drop
        SockFilter::new(0x06, 0, 0, 0xffff),      // 16 accept
        SockFilter::new(0x06, 0, 0, 0),           // 17 drop
        SockFilter::new(0x15, 3, 0, 0),           // 18 jeq hop-by-hop: to 22
        SockFilter::new(0x15, 2, 0, 43),          // 19 jeq routing: to 22
        SockFilter::new(0x15, 1, 0, 60),          // 20 jeq destination options: to 22
        SockFilter::new(0x15, 0, 1, 44),          // 21 jeq fragment, else drop
        SockFilter::new(0x06, 0, 0, 0xffff),      // 22 accept
        SockFilter::new(0x06, 0, 0, 0),           // 23 drop
    ];

    pub struct Sniffer(Socket);

    impl Sniffer {
        /// `next` gives up after `timeout`, so the reading thread can see it was stopped.
        pub fn open(timeout: Duration) -> io::Result<Sniffer> {
            // Every protocol: only such a socket sees outgoing packets too (the queries).
            let ip = (libc::ETH_P_ALL as u16).to_be();
            let socket = Socket::new(
                Domain::PACKET,
                Type::DGRAM,
                Some(Protocol::from(i32::from(ip))),
            )?;
            socket.attach_filter(&FILTER)?;
            socket.set_read_timeout(Some(timeout))?;
            Ok(Sniffer(socket))
        }

        /// The next packet as a query or an answer (`None`: neither, e.g. let in before the
        /// filter was attached). A timeout is `WouldBlock`/`TimedOut`. A loopback packet comes
        /// twice (out and in): `Asked` and `Names::record` take it once.
        pub fn next(&self, buf: &mut [u8]) -> io::Result<Option<Seen>> {
            let n = (&self.0).read(buf)?;
            Ok(super::seen(&buf[..n]))
        }
    }
}

#[cfg(target_os = "linux")]
pub use sys::Sniffer;

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, SocketAddrV6};

    use super::*;
    use crate::packet::{build_udp, build_udp6};

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

    /// The query `response` answers.
    fn query(name: &str) -> Vec<u8> {
        let mut m = response(name, [0; 4]);
        m[2] = 0x01;
        m[7] = 0;
        m.truncate(m.len() - 16);
        m
    }

    const CLIENT: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 40000);
    const SERVER: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 53), 53);
    const EXCHANGE: Exchange = Exchange {
        client: SocketAddr::V4(CLIENT),
        server: SocketAddr::V4(SERVER),
        id: 0x1234,
    };

    #[test]
    fn queries_to_and_answers_from_port_53_are_read_and_anything_else_is_not() {
        let packet = build_udp(SERVER, CLIENT, &response("discord.media", [1, 2, 3, 4]), 1);
        let Some(Seen::Answer(exchange, answer)) = seen(&packet) else {
            panic!("an answer");
        };
        assert_eq!(exchange, EXCHANGE);
        assert_eq!(answer.name, "discord.media");
        assert_eq!(answer.ips, [(IpAddr::from([1, 2, 3, 4]), 60)]);
        let asked = build_udp(CLIENT, SERVER, &query("discord.media"), 1);
        assert_eq!(seen(&asked), Some(Seen::Query(EXCHANGE)));
        let other = build_udp(CLIENT, addr([1, 1, 1, 1], 443), &query("x.y"), 1);
        assert_eq!(seen(&other), None, "not DNS");
        assert_eq!(seen(&packet[..packet.len() - 1]), None, "truncated");
        let mut tcp = packet.clone();
        tcp[9] = 6;
        assert_eq!(seen(&tcp), None, "not UDP");
    }

    #[test]
    fn an_ipv6_query_and_its_answer_are_read_too() {
        let client: SocketAddrV6 = "[fd00::1]:40000".parse().unwrap();
        let server: SocketAddrV6 = "[2001:4860:4860::8888]:53".parse().unwrap();
        let exchange = Exchange {
            client: client.into(),
            server: server.into(),
            id: 0x1234,
        };
        let asked = build_udp6(client, server, &query("t.me"));
        assert_eq!(seen(&asked), Some(Seen::Query(exchange)));
        let answer = build_udp6(server, client, &response("t.me", [5, 6, 7, 8]));
        let Some(Seen::Answer(e, a)) = seen(&answer) else {
            panic!("an answer");
        };
        assert_eq!((e, a.name.as_str()), (exchange, "t.me"));
        let other = build_udp6(client, "[fd00::2]:443".parse().unwrap(), &query("t.me"));
        assert_eq!(seen(&other), None, "not DNS");
    }

    #[test]
    fn only_the_answer_to_a_query_seen_lately_counts() {
        let now = Instant::now();
        let mut asked = Asked::default();
        assert!(!asked.answers(&EXCHANGE, now), "nobody asked");
        asked.ask(EXCHANGE, now);
        assert!(asked.answers(&EXCHANGE, now + Duration::from_secs(1)));
        assert!(
            asked.answers(&EXCHANGE, now + Duration::from_secs(2)),
            "a loopback answer comes twice"
        );
        assert!(!asked.answers(&EXCHANGE, now + ANSWER_WITHIN), "too late");
        for wrong in [
            Exchange {
                id: 0x1235,
                ..EXCHANGE
            },
            Exchange {
                client: addr([127, 0, 0, 1], 40001).into(),
                ..EXCHANGE
            },
            Exchange {
                server: addr([10, 0, 0, 1], 53).into(),
                ..EXCHANGE
            },
        ] {
            assert!(!asked.answers(&wrong, now), "{wrong:?}");
        }
        for port in 0..MAX_ASKED as u16 + 10 {
            asked.ask(
                Exchange {
                    client: addr([10, 0, 0, 2], port).into(),
                    ..EXCHANGE
                },
                now + ANSWER_WITHIN,
            );
        }
        assert!(asked.0.len() <= MAX_ASKED, "{}", asked.0.len());
    }

    /// The kernel side: socket, filter, loopback. Needs `CAP_NET_RAW` and an up `lo`, e.g.
    /// `unshare -rn sh -c 'ip link set lo up && <test binary> --ignored sniff'`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs CAP_NET_RAW (a user namespace will do)"]
    fn sniffer_reads_a_loopback_query_and_answer() {
        use std::net::UdpSocket;
        let _live = crate::linux::LIVE_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sniffer = Sniffer::open(Duration::from_millis(200)).unwrap();
        for host in ["127.0.0.1", "[::1]"] {
            let server = UdpSocket::bind(format!("{host}:53")).unwrap();
            if host == "[::1]" {
                // The answer carries a destination options header (PadN): the filter passes it.
                use std::os::fd::AsRawFd as _;
                let opts = [0u8, 0, 1, 4, 0, 0, 0, 0];
                // SAFETY: a live socket and a buffer of the length given.
                let rc = unsafe {
                    libc::setsockopt(
                        server.as_raw_fd(),
                        libc::IPPROTO_IPV6,
                        libc::IPV6_DSTOPTS,
                        opts.as_ptr().cast(),
                        opts.len() as libc::socklen_t,
                    )
                };
                assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
            }
            let client = UdpSocket::bind(format!("{host}:0")).unwrap();
            client.send_to(b"not dns", format!("{host}:9")).unwrap();
            client
                .send_to(&query("t.me"), format!("{host}:53"))
                .unwrap();
            server
                .send_to(
                    &response("t.me", [5, 6, 7, 8]),
                    client.local_addr().unwrap(),
                )
                .unwrap();
            let mut buf = vec![0; 65536];
            // The filter dropped the datagram to port 9: the query comes first, twice on `lo`.
            let mut got = vec![];
            while let Ok(Some(s)) = sniffer.next(&mut buf) {
                got.push(s);
            }
            let Some(Seen::Query(asked)) = got.first() else {
                panic!("{host}: {got:?}");
            };
            let Some(Seen::Answer(answered, answer)) = got.last() else {
                panic!("{host}: {got:?}");
            };
            assert_eq!(asked, answered);
            assert_eq!(asked.client, client.local_addr().unwrap());
            assert_eq!(answer.name, "t.me");
            assert_eq!(answer.ips, [(IpAddr::from([5, 6, 7, 8]), 60)]);
        }
    }
}

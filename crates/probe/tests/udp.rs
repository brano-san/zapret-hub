use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

fn server(reply: impl Fn(&[u8]) -> Option<Vec<u8>> + Send + 'static) -> SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            if let Some(r) = reply(&buf[..n]) {
                sock.send_to(&r, from).unwrap();
            }
        }
    });
    addr
}

const T: Duration = Duration::from_secs(2);

#[test]
fn ip_discovery_answered() {
    let addr = server(|req| {
        assert_eq!(req.len(), 74);
        assert_eq!(req[..4], [0, 1, 0, 70]);
        let mut r = req.to_vec();
        r[1] = 2;
        Some(r)
    });
    assert!(probe::udp::ip_discovery(addr, 0xDEAD_BEEF, T).is_some());
}

#[test]
fn ip_discovery_wrong_ssrc_is_none() {
    let addr = server(|req| {
        let mut r = req.to_vec();
        r[1] = 2;
        r[4] ^= 0xFF;
        Some(r)
    });
    assert!(probe::udp::ip_discovery(addr, 7, Duration::from_millis(300)).is_none());
}

#[test]
fn stun_answered() {
    let addr = server(|req| {
        assert_eq!(req.len(), 20);
        assert_eq!(req[..4], [0, 1, 0, 0]);
        assert_eq!(req[4..8], [0x21, 0x12, 0xA4, 0x42]);
        let mut r = req.to_vec();
        r[0] = 0x01;
        r[1] = 0x01;
        Some(r)
    });
    assert!(probe::udp::stun(addr, T).is_some());
}

#[test]
fn stun_silence_is_none() {
    let addr = server(|_| None);
    assert!(probe::udp::stun(addr, Duration::from_millis(200)).is_none());
}

#[test]
fn dns_udp_parses_a_and_nxdomain() {
    let n = std::sync::atomic::AtomicU32::new(0);
    let addr = server(move |req| {
        let mut r = req.to_vec();
        r[2] = 0x81;
        if n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            r[3] = 0x80;
            r[6..8].copy_from_slice(&[0, 2]);
            // CNAME (pointer name, RDATA is another pointer), then A 1.2.3.4
            r.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 12]);
            r.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);
        } else {
            r[3] = 0x83;
        }
        Some(r)
    });
    assert_eq!(
        probe::udp::dns_udp("discord.com", addr, T),
        Some(vec![Ipv4Addr::new(1, 2, 3, 4)])
    );
    assert_eq!(probe::udp::dns_udp("nope.invalid", addr, T), Some(vec![]));
}

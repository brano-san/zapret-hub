use router::socks::{self, SocksError};
use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener};
use std::time::Duration;

fn v4(a: SocketAddr) -> SocketAddrV4 {
    match a {
        SocketAddr::V4(a) => a,
        _ => unreachable!(),
    }
}

/// A one-connection proxy that answers the greeting with `hello` and the request with `reply`.
fn proxy(
    hello: &'static [u8],
    reply: Option<&'static [u8]>,
) -> (SocketAddrV4, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = v4(listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut got = vec![0u8; 3];
        conn.read_exact(&mut got).unwrap();
        conn.write_all(hello).unwrap();
        if let Some(reply) = reply {
            let mut req = [0u8; 10];
            if conn.read_exact(&mut req).is_ok() {
                got.extend_from_slice(&req);
                let _ = conn.write_all(reply);
            }
        }
        got
    });
    (addr, handle)
}

const DST: &str = "35.217.1.2:50005";
const SHORT: Duration = Duration::from_millis(300);

#[test]
fn udp_associate_names_the_destination_and_returns_the_relay() {
    let (addr, server) = proxy(&[5, 0], Some(&[5, 0, 0, 1, 0, 0, 0, 0, 0x1F, 0x90]));
    let assoc = socks::udp_associate(addr, DST.parse().unwrap(), Duration::from_secs(2)).unwrap();
    assert_eq!(assoc.relay, SocketAddrV4::new(*addr.ip(), 8080));
    assert_eq!(
        server.join().unwrap(),
        [5, 1, 0, 5, 3, 0, 1, 35, 217, 1, 2, 0xC3, 0x55]
    );
}

#[test]
fn a_proxy_that_wants_a_password_is_named() {
    for hello in [&[5u8, 2][..], &[5, 0xFF]] {
        let (addr, _server) = proxy(hello, None);
        assert!(matches!(
            socks::udp_associate(addr, DST.parse().unwrap(), SHORT),
            Err(SocksError::AuthRequired)
        ));
    }
}

#[test]
fn a_refused_associate_carries_its_code() {
    let (addr, _server) = proxy(&[5, 0], Some(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]));
    let err = socks::udp_associate(addr, DST.parse().unwrap(), SHORT).unwrap_err();
    assert!(matches!(err, SocksError::Refused(7)));
    assert!(err.to_string().contains("does not support UDP"));
}

#[test]
fn a_silent_proxy_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = v4(listener.local_addr().unwrap());
    let _hold = std::thread::spawn(move || {
        let _conn = listener.accept();
        std::thread::sleep(Duration::from_secs(2));
    });
    let started = std::time::Instant::now();
    assert!(matches!(
        socks::udp_associate(addr, DST.parse().unwrap(), SHORT),
        Err(SocksError::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
    let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr2 = v4(listener2.local_addr().unwrap());
    let _hold2 = std::thread::spawn(move || {
        let _conn = listener2.accept();
        std::thread::sleep(Duration::from_secs(2));
    });
    assert!(matches!(
        socks::connect(addr2, DST.parse().unwrap(), None, SHORT),
        Err(SocksError::Timeout)
    ));
}

#[test]
fn a_dropped_connection_and_a_closed_port_are_io_errors() {
    let (addr, _server) = proxy(&[5, 0], Some(&[]));
    assert!(matches!(
        socks::udp_associate(addr, DST.parse().unwrap(), SHORT),
        Err(SocksError::Io(_))
    ));
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        v4(l.local_addr().unwrap())
    };
    // Windows refuses a loopback connect only after ~2 s, so the timeout must exceed that.
    assert!(matches!(
        socks::check(closed, Duration::from_secs(5)),
        Err(SocksError::Io(_))
    ));
}

#[test]
fn check_greets_and_measures() {
    let (addr, _server) = proxy(&[5, 0], None);
    assert!(socks::check(addr, Duration::from_secs(2)).unwrap() < Duration::from_secs(2));
}

#[test]
fn connect_names_the_domain_and_then_carries_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = v4(listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut got = vec![0u8; 3 + 5 + 11 + 2];
        s.read_exact(&mut got[..3]).unwrap();
        s.write_all(&[5, 0]).unwrap();
        s.read_exact(&mut got[3..]).unwrap();
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        let mut ping = [0u8; 4];
        s.read_exact(&mut ping).unwrap();
        s.write_all(b"pong").unwrap();
        (got, ping)
    });
    let mut stream = socks::connect(
        addr,
        "162.159.135.232:443".parse().unwrap(),
        Some("discord.com"),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        stream.read_timeout().unwrap(),
        None,
        "a relayed stream has no timeout"
    );
    stream.write_all(b"ping").unwrap();
    let mut pong = [0u8; 4];
    stream.read_exact(&mut pong).unwrap();
    let (got, ping) = server.join().unwrap();
    assert_eq!(
        got,
        [&[5u8, 1, 0, 5, 1, 0, 3, 11][..], b"discord.com", &[1, 187]].concat()
    );
    assert_eq!((&ping, &pong), (b"ping", b"pong"));
}

#[test]
fn connect_by_ip_and_a_refusal_with_its_code() {
    let (addr, server) = proxy(&[5, 0], Some(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]));
    let err = socks::connect(addr, "1.2.3.4:80".parse().unwrap(), None, SHORT).unwrap_err();
    assert!(matches!(err, SocksError::Refused(5)));
    assert_eq!(err.to_string(), "the proxy refused the request: code 5");
    assert_eq!(
        server.join().unwrap(),
        [5, 1, 0, 5, 1, 0, 1, 1, 2, 3, 4, 0, 80]
    );
}

#[test]
fn connect_rejects_a_name_over_255_bytes() {
    let (addr, _server) = proxy(&[5, 0], None);
    let name = "a".repeat(256);
    let r = socks::connect(addr, DST.parse().unwrap(), Some(&name), SHORT);
    assert!(matches!(r, Err(SocksError::Io(_))));
}

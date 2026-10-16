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

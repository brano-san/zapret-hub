use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn handshake_talks_socks5() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hello = [0u8; 3];
        stream.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [5, 1, 0]);
        stream.write_all(&[5, 0]).unwrap();
    });
    assert!(probe::socks::handshake(&addr, Duration::from_secs(2)));
    server.join().unwrap();
}

#[test]
fn a_closed_port_is_not_a_proxy() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert!(!probe::socks::handshake(
        &format!("127.0.0.1:{port}"),
        Duration::from_millis(300)
    ));
    assert!(!probe::socks::handshake(
        "not an address",
        Duration::from_millis(300)
    ));
}

#[test]
fn reads_the_nekobox_port() {
    let dir = tempfile::tempdir().unwrap();
    let groups = dir.path().join("config").join("groups");
    std::fs::create_dir_all(&groups).unwrap();
    std::fs::write(
        groups.join("nekobox.json"),
        r#"{"inbound_socks_port": 18082, "spmode2": ["system_proxy"]}"#,
    )
    .unwrap();
    assert_eq!(probe::socks::nekobox_port(dir.path()), Some(18082));
    assert_eq!(
        probe::socks::nekobox_port(&dir.path().join("missing")),
        None
    );
}

/// A SOCKS5 proxy that answers `UDP ASSOCIATE` with `code`; its relay answers DNS when `answer`.
fn udp_proxy(code: u8, answer: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let relay = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = relay.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hello = [0u8; 3];
        stream.read_exact(&mut hello).unwrap();
        stream.write_all(&[5, 0]).unwrap();
        let mut request = [0u8; 10];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(request[1], 3, "UDP ASSOCIATE");
        let [hi, lo] = port.to_be_bytes();
        // Bound to 0.0.0.0: the client uses the proxy's own address.
        stream
            .write_all(&[5, code, 0, 1, 0, 0, 0, 0, hi, lo])
            .unwrap();
        let mut buf = [0u8; 1500];
        relay
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        if let Ok((n, from)) = relay.recv_from(&mut buf)
            && answer
        {
            assert_eq!(buf[..10], [0, 0, 0, 1, 1, 1, 1, 1, 0, 53]);
            buf[12] |= 0x80;
            relay.send_to(&buf[..n], from).unwrap();
        }
        // Keep the association until the client is done.
        let _ = stream.read(&mut [0u8; 1]);
    });
    addr
}

#[test]
fn a_proxy_that_relays_udp() {
    let t = Duration::from_millis(500);
    assert_eq!(probe::socks::relays_udp(&udp_proxy(0, true), t), Some(true));
}

#[test]
fn a_proxy_without_udp() {
    let t = Duration::from_millis(300);
    // Refused: 07, command not supported.
    assert_eq!(
        probe::socks::relays_udp(&udp_proxy(7, false), t),
        Some(false)
    );
    // Accepted, but nothing comes back through the relay.
    assert_eq!(
        probe::socks::relays_udp(&udp_proxy(0, false), t),
        Some(false)
    );
    assert_eq!(probe::socks::relays_udp("127.0.0.1:1", t), None);
}

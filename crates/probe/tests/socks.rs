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

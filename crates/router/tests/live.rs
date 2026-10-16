//! Opens WinDivert: every test needs an elevated terminal and `WINDIVERT_DLL=<path to WinDivert.dll>`.
use router::divert::{Dll, FLAG_RECV_ONLY, FLAG_SNIFF, Handle, Layer, MAX_PACKET, SOCKET_FILTER};
use router::packet::parse_udp;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn dll() -> Dll {
    let path = std::env::var("WINDIVERT_DLL").expect("set WINDIVERT_DLL");
    Dll::load(&PathBuf::from(path)).unwrap()
}

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn a_network_handle_captures_and_reinjects_a_packet() {
    let h = Arc::new(
        Handle::open(
            &dll(),
            "outbound and udp and ip.DstAddr == 192.0.2.1 and udp.DstPort == 9",
            Layer::Network,
            0,
        )
        .unwrap(),
    );
    let reader = {
        let h = h.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; MAX_PACKET];
            let (n, addr) = h.recv(&mut buf).unwrap();
            (buf[..n].to_vec(), addr)
        })
    };
    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
    s.send_to(b"zapret-hub", "192.0.2.1:9").unwrap();
    let (packet, addr) = reader.join().unwrap();
    assert_eq!(parse_udp(&packet).unwrap().payload, b"zapret-hub");
    assert!(addr.outbound());
    h.send(&packet, &addr).unwrap();
}

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn the_socket_handle_names_this_process() {
    let h = Arc::new(
        Handle::open(
            &dll(),
            SOCKET_FILTER,
            Layer::Socket,
            FLAG_SNIFF | FLAG_RECV_ONLY,
        )
        .unwrap(),
    );
    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
    let port = s.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let h = h.clone();
        std::thread::spawn(move || {
            while let Ok((_, addr)) = h.recv(&mut []) {
                if addr.socket_pid() == std::process::id() && addr.socket_local_port() == port {
                    let _ = tx.send(());
                    return;
                }
            }
        });
    }
    s.send_to(b"x", "192.0.2.1:9").unwrap();
    rx.recv_timeout(Duration::from_secs(2))
        .expect("no socket event for this process within 2 s");
    h.shutdown();
}

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn shutdown_ends_a_waiting_recv() {
    let h = Arc::new(Handle::open(&dll(), "false", Layer::Network, 0).unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let h = h.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; MAX_PACKET];
            let _ = tx.send(h.recv(&mut buf).is_err());
        });
    }
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    h.shutdown();
    assert!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "recv must return Err"
    );
    assert!(t.elapsed() < Duration::from_secs(1));
}

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
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let h = h.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; MAX_PACKET];
            if let Ok((n, addr)) = h.recv(&mut buf) {
                let _ = tx.send((buf[..n].to_vec(), addr));
            }
        });
    }
    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
    s.send_to(b"zapret-hub", "192.0.2.1:9").unwrap();
    let Ok((packet, addr)) = rx.recv_timeout(Duration::from_secs(2)) else {
        h.shutdown();
        panic!("no packet captured within 2 s");
    };
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

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL and ROUTER_SOCKS5 (a working proxy) set"]
fn a_running_router_relays_a_flow_through_the_proxy() {
    use router::flows::Via;
    use router::rules::{self, Protocol, Rule};
    use router::{Router, RouterState, Setup, parse_proxy};

    let dir = tempfile::tempdir().unwrap();
    let r = Router::new(dir.path().join("routes.toml"));
    let rule = Rule {
        name: "dns".into(),
        enabled: true,
        protocol: Protocol::Udp,
        ports: vec!["53".into()],
        ips: vec!["1.1.1.1/32".into()],
        domains: vec![],
    };
    r.edit(|list| rules::add(list, rule)).unwrap();
    let proxy = parse_proxy(std::env::var("ROUTER_SOCKS5").ok().as_deref()).unwrap();
    let dll = PathBuf::from(std::env::var("WINDIVERT_DLL").expect("set WINDIVERT_DLL"));
    r.start(Setup { dll, proxy }).unwrap();
    assert_eq!(r.status().state, RouterState::Running);

    // A DNS query for example.com, type A.
    let mut query = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in ["example", "com"] {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.extend_from_slice(&[0, 0, 1, 0, 1]);
    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.send_to(&query, "1.1.1.1:53").unwrap();
    let mut buf = [0u8; 1500];
    let (n, from) = s
        .recv_from(&mut buf)
        .expect("no DNS answer through the proxy");
    assert_eq!(from.to_string(), "1.1.1.1:53");
    assert!(n > 12 && buf[..2] == [0x12, 0x34], "not our answer");

    let port = s.local_addr().unwrap().port();
    let rows = r.flows(false);
    let row = rows
        .iter()
        .find(|f| f.local_port == port && f.destination == "1.1.1.1:53")
        .unwrap_or_else(|| panic!("no flow row: {rows:?}"));
    assert_eq!(row.via, Via::Proxy, "{row:?}");
    assert!(row.down > 0.0, "{row:?}");
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
}

#[test]
#[ignore = "loads WinDivert.dll (no elevation needed): run with WINDIVERT_DLL set"]
fn the_largest_capture_filters_compile() {
    use router::divert::{capture_filter, filter};
    use router::rules::{self, Preset, Rule, Table};
    use std::net::Ipv4Addr;
    let names: Vec<(Ipv4Addr, String)> = (0..400u32)
        .map(|i| {
            let name = if i % 2 == 0 {
                "discord.com"
            } else {
                "x.discord.media"
            };
            (Ipv4Addr::from(0x0A00_0000 + i * 1000), name.to_string())
        })
        .collect();
    let table = Table::new(&[
        rules::preset(Preset::Discord),
        rules::preset(Preset::DiscordVoice),
    ])
    .with_names(&names);
    let held: Vec<Ipv4Addr> = (0..100u32)
        .map(|i| Ipv4Addr::from(0xC000_0000 + i * 7))
        .collect();
    let ranges = Table::new(&[Rule {
        domains: vec![],
        ips: (0..65).map(|i| format!("10.{i}.0.0/24")).collect(),
        ..rules::preset(Preset::DiscordVoice)
    }]);
    let ports = Table::new(&[Rule {
        protocol: rules::Protocol::Both,
        ports: (0..150).map(|i| (1000 + 2 * i).to_string()).collect(),
        ..rules::preset(Preset::Discord)
    }]);
    let dll = dll();
    for text in [
        capture_filter(&table, Some(61000), &held).unwrap(),
        filter(&ranges).unwrap(),
        capture_filter(&ports, Some(61000), &[]).unwrap(),
    ] {
        dll.compile(&text, Layer::Network)
            .unwrap_or_else(|e| panic!("{e}: {text}"));
    }
}

//! Observes real flows with conntrack on Linux and classifies them. It needs `ip`, `nft` and the
//! routing capabilities, and refuses to run outside a fresh network namespace: the host's network
//! is never touched. As a user (a user namespace grants the capabilities there):
//! `unshare -rn <target>/debug/deps/linux_observe-<hash> --ignored` (and again with `ZH_ACCT=1`).
//!
//! A dummy link holds `10.88.0.1` (a TCP server of this process: a flow with a reply) and leads
//! to `10.88.0.2`, where nothing answers (a UDP flow and a TCP connect without a reply).
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use diag::culprits::{Fault, fault};
use router::observe::Observer;
use router::rules::Protocol;

fn ip(args: &str) {
    let out = Command::new("ip").args(args.split(' ')).output().unwrap();
    assert!(out.status.success(), "ip {args}: {out:?}");
}

#[test]
#[ignore = "needs ip, nft and a fresh network namespace: unshare -rn <this binary> --ignored"]
fn conntrack_tells_an_answered_tcp_flow_from_unanswered_udp_and_syns() {
    let links: Vec<String> = std::fs::read_to_string("/proc/net/dev")
        .unwrap()
        .lines()
        .skip(2)
        .filter_map(|l| Some(l.split_once(':')?.0.trim().to_owned()))
        .collect();
    assert_eq!(
        links,
        ["lo"],
        "run in a fresh network namespace: unshare -rn"
    );
    ip("link set lo up");
    ip("link add d0 type dummy");
    ip("addr add 10.88.0.1/24 dev d0");
    ip("link set d0 up");
    // `ZH_ACCT=1`: with per-direction counters (the namespace's own setting).
    if std::env::var_os("ZH_ACCT").is_some() {
        std::fs::write("/proc/sys/net/netfilter/nf_conntrack_acct", "1").unwrap();
    }
    let server = TcpListener::bind("10.88.0.1:7000").unwrap();
    std::thread::spawn(move || {
        for mut conn in server.incoming().flatten() {
            let mut buf = [0u8; 64];
            if let Ok(n) = conn.read(&mut buf) {
                let _ = conn.write_all(&buf[..n]);
            }
        }
    });

    let exe = std::env::current_exe().unwrap();
    let me = exe.file_name().unwrap().to_string_lossy().into_owned();
    let observer = Observer::start(Path::new(""), vec![me], None).expect("observe");
    assert!(observer.has_dns(), "the DNS sniffer opens with CAP_NET_RAW");

    let silent = std::thread::spawn(|| {
        TcpStream::connect_timeout(&"10.88.0.2:443".parse().unwrap(), Duration::from_secs(6))
    });
    let mut conn = TcpStream::connect("10.88.0.1:7000").unwrap();
    conn.write_all(b"ping").unwrap();
    let mut buf = [0u8; 4];
    conn.read_exact(&mut buf).unwrap();
    let udp = UdpSocket::bind("0.0.0.0:0").unwrap();
    for _ in 0..7 {
        udp.send_to(b"hello", "10.88.0.2:5000").unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
    assert!(
        silent.join().unwrap().is_err(),
        "nothing answers at 10.88.0.2"
    );
    let flows = observer.stop();
    let tables = Command::new("nft")
        .args(["list", "tables"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&tables.stdout).contains("zapret_hub_observe"),
        "the conntrack table goes with the observation"
    );

    let row = |protocol, dst: &str| {
        flows
            .iter()
            .find(|f| f.protocol == protocol && f.dst == dst)
            .unwrap_or_else(|| panic!("{protocol:?} {dst} in {flows:#?}"))
    };
    let answered = row(Protocol::Tcp, "10.88.0.1:7000");
    assert!(answered.syn_answered, "{answered:?}");
    assert_eq!(fault(answered), None);
    let syns = row(Protocol::Tcp, "10.88.0.2:443");
    assert!(
        matches!(fault(syns), Some(Fault::SynUnanswered { syns }) if syns >= 3),
        "{syns:?}"
    );
    let udp = row(Protocol::Udp, "10.88.0.2:5000");
    assert!(
        matches!(fault(udp), Some(Fault::UdpNoReply { up, down: 0 }) if up >= 4),
        "{udp:?}"
    );
    // Only this program's flows: the server's side is the same conntrack entry.
    assert_eq!(flows.len(), 3, "{flows:#?}");
}

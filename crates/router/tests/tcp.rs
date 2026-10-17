use router::packet::{ACK, FIN, RST, SYN, Tcp, parse_tcp, rewrite_tcp};
use router::tcp::{self, ACCEPT_WAIT, DIRECT_FOR, IDLE, LINGER, Nat, Verdict};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering::SeqCst},
};
use std::time::{Duration, Instant};

const AP: u16 = 61000;
const CLIENT: &str = "192.168.1.5:50000";
const SERVER: &str = "162.159.135.232:443";

fn sa(s: &str) -> SocketAddrV4 {
    s.parse().unwrap()
}
fn seg(src: &str, dst: &str, flags: u8) -> Tcp<'static> {
    Tcp {
        src: src.parse().unwrap(),
        dst: dst.parse().unwrap(),
        flags,
        payload: &[],
    }
}
fn reply(port: u16, flags: u8) -> Tcp<'static> {
    seg(
        "192.168.1.5:61000",
        &format!("162.159.135.232:{port}"),
        flags,
    )
}

#[test]
fn a_syn_is_reflected_to_the_acceptor_and_its_replies_back() {
    let t0 = Instant::now();
    let mut nat = Nat::default();
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        Verdict::Syn
    );
    let to_acceptor = Verdict::Rewrite {
        src: sa("162.159.135.232:50000"),
        dst: sa("192.168.1.5:61000"),
    };
    assert_eq!(nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0), to_acceptor);
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        to_acceptor,
        "a retransmitted SYN"
    );
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0),
        to_acceptor
    );
    assert_eq!(
        nat.original(50000, "162.159.135.232".parse().unwrap()),
        Some(sa(SERVER))
    );
    assert_eq!(
        nat.original(50000, "10.0.0.7".parse().unwrap()),
        None,
        "a LAN host on the acceptor port"
    );
    assert_eq!(
        nat.from_acceptor(&reply(50000, SYN | ACK), t0),
        Verdict::Rewrite {
            src: sa(SERVER),
            dst: sa(CLIENT)
        }
    );
    assert_eq!(nat.destinations(), [sa(SERVER)]);
}

/// IPv4/TCP, 20 + 20 bytes, checksums 0xAAAA / 0xBBBB.
fn packet(src: SocketAddrV4, dst: SocketAddrV4, flags: u8) -> Vec<u8> {
    let mut p = vec![0x45, 0, 0, 40, 0, 1, 0x40, 0, 128, 6, 0xAA, 0xAA];
    p.extend_from_slice(&src.ip().octets());
    p.extend_from_slice(&dst.ip().octets());
    p.extend_from_slice(&src.port().to_be_bytes());
    p.extend_from_slice(&dst.port().to_be_bytes());
    p.extend_from_slice(&[
        0, 0, 0, 1, 0, 0, 0, 0, 0x50, flags, 0xFA, 0xF0, 0xBB, 0xBB, 0, 0,
    ]);
    p
}

fn apply(p: &mut [u8], v: Verdict) {
    let Verdict::Rewrite { src, dst } = v else {
        panic!("{v:?}")
    };
    rewrite_tcp(p, src, dst);
}

#[test]
fn reflection_rewrites_the_bytes_both_ways() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    // Client SYN to the server: src = server ip : client port, dst = client ip : acceptor port.
    let mut syn = packet(sa(CLIENT), sa(SERVER), SYN);
    let v = nat.admit(&parse_tcp(&syn).unwrap(), AP, t0);
    apply(&mut syn, v);
    assert_eq!(syn[12..20], [162, 159, 135, 232, 192, 168, 1, 5]);
    assert_eq!(syn[20..24], [0xC3, 0x50, 0xEE, 0x48], "50000 -> 61000");
    // The acceptor's SYN-ACK (outbound, client ip : acceptor -> server ip : client port) goes
    // back as the server's: src = server ip:port, dst = client ip:port.
    let mut synack = packet(
        sa("192.168.1.5:61000"),
        sa("162.159.135.232:50000"),
        SYN | ACK,
    );
    let v = nat.from_acceptor(&parse_tcp(&synack).unwrap(), t0);
    apply(&mut synack, v);
    assert_eq!(synack[12..20], [162, 159, 135, 232, 192, 168, 1, 5]);
    assert_eq!(synack[20..24], [0x01, 0xBB, 0xC3, 0x50], "443 -> 50000");
    for p in [&syn, &synack] {
        assert_eq!((&p[10..12], &p[36..38]), (&[0u8, 0][..], &[0u8, 0][..]));
        assert_eq!(p[33], parse_tcp(p).unwrap().flags, "flags stay");
    }
}

#[test]
fn a_connection_older_than_the_rule_passes_unchanged() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0),
        Verdict::Pass
    );
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN | ACK), AP, t0),
        Verdict::Pass,
        "only a bare SYN opens"
    );
    assert_eq!(
        nat.from_acceptor(&reply(4444, ACK), t0),
        Verdict::Drop,
        "no entry, no reply"
    );
}

#[test]
fn entries_end_after_fin_both_ways_or_a_reset_and_idle_ones_after_2_minutes() {
    let t0 = Instant::now();
    let mut nat = Nat::default();
    for port in 50001..=50004 {
        nat.admit(&seg(&format!("192.168.1.5:{port}"), SERVER, SYN), AP, t0);
    }
    nat.from_client(&seg("192.168.1.5:50001", SERVER, FIN | ACK), AP, t0);
    nat.from_acceptor(&reply(50001, FIN | ACK), t0);
    nat.from_acceptor(&reply(50002, RST), t0);
    nat.from_client(&seg("192.168.1.5:50003", SERVER, FIN | ACK), AP, t0);
    assert!(
        nat.expire(t0 + LINGER - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        nat.destinations().len(),
        4,
        "a closed entry lingers for the last ACK"
    );
    let mut gone = nat.expire(t0 + LINGER);
    gone.sort();
    assert_eq!(
        gone,
        [(50001, sa(SERVER)), (50002, sa(SERVER))],
        "removals are reported"
    );
    let peer = "162.159.135.232".parse().unwrap();
    let alive: Vec<bool> = (50001..=50004)
        .map(|p| nat.original(p, peer).is_some())
        .collect();
    assert_eq!(alive, [false, false, true, true], "one FIN keeps it");
    nat.from_client(
        &seg("192.168.1.5:50004", SERVER, ACK),
        AP,
        t0 + Duration::from_secs(60),
    );
    assert_eq!(
        nat.expire(t0 + IDLE),
        [(50003, sa(SERVER))],
        "an idle entry is reported"
    );
    assert_eq!(nat.destinations().len(), 2, "and lingers");
    assert!(
        matches!(
            nat.from_acceptor(&reply(50003, FIN | ACK), t0 + IDLE),
            Verdict::Rewrite { .. }
        ),
        "so the acceptor's FIN still reaches the client"
    );
    let mut gone = nat.expire(t0 + Duration::from_secs(60) + IDLE);
    gone.sort();
    assert_eq!(
        gone,
        [(50003, sa(SERVER)), (50004, sa(SERVER))],
        "the lingering one goes, the other one idles"
    );
    assert_eq!(nat.destinations().len(), 1);
    assert_eq!(
        nat.expire(t0 + Duration::from_secs(60) + IDLE + LINGER),
        [(50004, sa(SERVER))]
    );
    assert!(nat.destinations().is_empty());
}

#[test]
fn a_closed_entry_replaced_by_a_new_syn_is_reported() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    nat.from_acceptor(&reply(50000, RST), t0);
    nat.admit(&seg(CLIENT, "1.1.1.1:443", SYN), AP, t0);
    assert_eq!(nat.expire(t0), [(50000, sa(SERVER))]);
    assert_eq!(nat.destinations(), [sa("1.1.1.1:443")]);
}

#[test]
fn a_new_direct_connection_on_a_lingering_port_is_not_caught() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    nat.from_client(&seg(CLIENT, SERVER, FIN | ACK), AP, t0);
    nat.from_acceptor(&reply(50000, FIN | ACK), t0);
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0),
        Verdict::Rewrite {
            src: sa("162.159.135.232:50000"),
            dst: sa("192.168.1.5:61000")
        },
        "the last ACK still reaches the acceptor"
    );
    nat.mark_direct(sa(SERVER), t0);
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        Verdict::Marked
    );
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0),
        Verdict::Pass
    );
    assert_eq!(
        nat.expire(t0),
        [(50000, sa(SERVER))],
        "the removal is reported"
    );
    assert!(nat.destinations().is_empty());
}

#[test]
fn a_closed_entry_takes_a_new_syn() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    nat.from_acceptor(&reply(50000, RST), t0);
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        Verdict::Syn
    );
}

#[test]
fn a_failed_address_goes_direct_for_60_s() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.mark_direct(sa(SERVER), t0);
    assert_eq!(
        nat.from_client(
            &seg(CLIENT, SERVER, SYN),
            AP,
            t0 + DIRECT_FOR - Duration::from_millis(1)
        ),
        Verdict::Marked
    );
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0 + DIRECT_FOR),
        Verdict::Syn
    );
    nat.mark_direct(sa(SERVER), t0);
    nat.clear_marks();
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        Verdict::Syn,
        "a proxy that came back clears the marks"
    );
}

#[test]
fn a_syn_the_acceptor_never_saw_goes_direct_after_2_s() {
    assert_eq!(
        ACCEPT_WAIT,
        Duration::from_secs(2),
        "before the 3 s SYN retransmit of Windows"
    );
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    nat.admit(&seg("192.168.1.5:50009", SERVER, SYN), AP, t0);
    nat.accepted(50009);
    assert!(
        nat.unaccepted(t0 + ACCEPT_WAIT - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(nat.unaccepted(t0 + ACCEPT_WAIT), [(50000, sa(SERVER))]);
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0 + ACCEPT_WAIT),
        Verdict::Marked,
        "its retry goes direct"
    );
}

fn pair() -> (TcpStream, TcpStream) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    (c, l.accept().unwrap().0)
}

#[test]
fn pipe_copies_until_eof_and_counts() {
    let ((mut app, hub_in), (hub_out, mut far)) = (pair(), pair());
    let worker = std::thread::spawn(move || {
        let mut n = 0;
        tcp::pipe(&hub_in, &hub_out, &|| false, &mut |b| n += b).unwrap();
        n
    });
    app.write_all(b"hello").unwrap();
    app.shutdown(Shutdown::Write).unwrap();
    let mut got = Vec::new();
    far.read_to_end(&mut got).unwrap();
    assert_eq!((got.as_slice(), worker.join().unwrap()), (&b"hello"[..], 5));
}

#[test]
fn pipe_ends_when_the_session_stops() {
    let ((_app, hub_in), (hub_out, _far)) = (pair(), pair());
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let worker =
        std::thread::spawn(move || tcp::pipe(&hub_in, &hub_out, &|| s.load(SeqCst), &mut |_| {}));
    stop.store(true, SeqCst);
    let started = Instant::now();
    worker.join().unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn reset_sends_an_rst() {
    let (mut app, hub) = pair();
    tcp::reset(hub);
    assert_eq!(
        app.read(&mut [0u8; 1]).unwrap_err().kind(),
        ErrorKind::ConnectionReset
    );
}

#[test]
fn a_forgotten_entry_leaves_no_mark() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    assert_eq!(nat.forget(50000), Some(sa(SERVER)));
    assert!(nat.destinations().is_empty());
    assert_eq!(
        nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0),
        Verdict::Syn,
        "not marked direct"
    );
}

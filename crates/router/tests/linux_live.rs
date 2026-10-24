//! Routes real flows through a running router on Linux. It needs `ip`, `nft`, `bash` and the
//! routing capabilities, and refuses to run outside a fresh network namespace: the host's network
//! is never touched. As a user (a user namespace grants the capabilities there):
//! `unshare -rn <target>/debug/deps/linux_live-<hash> --ignored --test-threads 1`.
//!
//! This process's namespace holds the router, the SOCKS5 proxy and the programs (`bash`); a second
//! namespace behind a veth pair holds the "internet": TCP and UDP echo and a DNS server.
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::spawn;
use std::time::{Duration, Instant};

use router::flows::{DirectReason, FlowRow, ProxyState, Via};
use router::linux::{nft, sockopt};
use router::packet::socks_header;
use router::rules::{Protocol, Rule};
use router::{Router, RouterState, Setup};

/// What the proxy was asked to reach: `tcp 10.77.0.2:7000`, `udp …`, `tcp echo.test:7000`.
type Log = Arc<Mutex<Vec<String>>>;

const ECHO: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 2);
/// The address `echo.test` resolves to.
const NAMED: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 3);
const WAIT: Duration = Duration::from_secs(5);

fn cmd(program: &str, args: &str) -> String {
    let out = Command::new(program)
        .args(args.split(' '))
        .output()
        .unwrap();
    assert!(out.status.success(), "{program} {args}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ip(args: &str) -> String {
    cmd("ip", args)
}

fn echo_tcp(mut conn: TcpStream) {
    let mut buf = [0u8; 4096];
    while let Ok(n) = conn.read(&mut buf) {
        if n == 0 || conn.write_all(&buf[..n]).is_err() {
            return;
        }
    }
}

/// Answers every query with `NAMED` (TTL 60).
fn dns_answer(query: &[u8]) -> Vec<u8> {
    let mut m = query[..2].to_vec();
    m.extend([0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
    m.extend(&query[12..]);
    m.extend([0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
    m.extend(NAMED.octets());
    m
}

/// The second namespace, made by a thread of this process: its sockets stay there.
fn internet() {
    let main = std::process::id();
    let (tx, rx) = mpsc::channel();
    spawn(move || {
        // SAFETY: a plain syscall; it moves only this thread.
        let rc = unsafe { libc::unshare(libc::CLONE_NEWNET) };
        assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
        ip(&format!("link add vb type veth peer name va netns {main}"));
        ip(&format!("addr add {ECHO}/24 dev vb"));
        ip(&format!("addr add {NAMED}/24 dev vb"));
        ip("link set vb up");
        ip("link set lo up");
        for port in [7000, 7100] {
            let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).unwrap();
            spawn(move || {
                for conn in listener.incoming().flatten() {
                    spawn(move || echo_tcp(conn));
                }
            });
        }
        let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 7001)).unwrap();
        spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = udp.recv_from(&mut buf) {
                let _ = udp.send_to(&buf[..n], from);
            }
        });
        let dns = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 53)).unwrap();
        spawn(move || {
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = dns.recv_from(&mut buf) {
                let _ = dns.send_to(&dns_answer(&buf[..n]), from);
            }
        });
        tx.send(()).unwrap();
    });
    rx.recv_timeout(WAIT).expect("the second namespace is up");
}

/// A SOCKS5 address (type 1 or 3) at `at`: its text and where it goes; `echo.test` is `NAMED`.
fn target(at: &[u8]) -> Option<(String, SocketAddrV4, usize)> {
    let (ip, name, end) = match at.first()? {
        1 => {
            let ip = Ipv4Addr::new(*at.get(1)?, *at.get(2)?, *at.get(3)?, *at.get(4)?);
            (ip, ip.to_string(), 5)
        }
        3 => {
            let len = usize::from(*at.get(1)?);
            let name = String::from_utf8(at.get(2..2 + len)?.to_vec()).ok()?;
            assert_eq!(name, "echo.test");
            (NAMED, name, 2 + len)
        }
        _ => return None,
    };
    let port = u16::from_be_bytes([*at.get(end)?, *at.get(end + 1)?]);
    Some((
        format!("{name}:{port}"),
        SocketAddrV4::new(ip, port),
        end + 2,
    ))
}

fn marked_udp() -> UdpSocket {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    socket2::SockRef::from(&socket)
        .set_mark(nft::MARK_OWN)
        .unwrap();
    socket
}

/// A no-auth SOCKS5 proxy (CONNECT, UDP ASSOCIATE). Its own sockets are marked like the hub's,
/// as a real proxy's traffic is skipped (`zapret-bypass`, or rules that do not match it).
fn serve(mut conn: TcpStream, log: &Log) {
    let mut greeting = [0u8; 3];
    if conn.read_exact(&mut greeting).is_err() || conn.write_all(&[5, 0]).is_err() {
        return;
    }
    let mut request = [0u8; 300];
    let Ok(n) = conn.read(&mut request) else {
        return; // the router's health check ends here
    };
    let Some((name, dst, _)) = request.get(3..n).and_then(target) else {
        return;
    };
    match request[1] {
        1 => {
            log.lock().unwrap().push(format!("tcp {name}"));
            let up = sockopt::connect_marked(dst, nft::MARK_OWN, WAIT).unwrap();
            conn.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            let (mut a, mut b) = (conn.try_clone().unwrap(), up.try_clone().unwrap());
            spawn(move || std::io::copy(&mut a, &mut b));
            let (mut up, mut conn) = (up, conn);
            let _ = std::io::copy(&mut up, &mut conn);
        }
        3 => {
            let relay = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap());
            let mut reply = vec![5, 0, 0];
            reply.extend(
                socks_header(SocketAddrV4::new(
                    Ipv4Addr::LOCALHOST,
                    relay.local_addr().unwrap().port(),
                ))[3..]
                    .iter(),
            );
            conn.write_all(&reply).unwrap();
            let out = Arc::new(marked_udp());
            let client: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
            {
                let (relay, out, client) = (relay.clone(), out.clone(), client.clone());
                spawn(move || {
                    let mut buf = [0u8; 2048];
                    while let Ok((n, SocketAddr::V4(from))) = out.recv_from(&mut buf) {
                        let mut datagram = socks_header(from).to_vec();
                        datagram.extend(&buf[..n]);
                        if let Some(to) = *client.lock().unwrap() {
                            let _ = relay.send_to(&datagram, to);
                        }
                    }
                });
            }
            let log = log.clone();
            spawn(move || {
                let mut buf = [0u8; 2048];
                while let Ok((n, from)) = relay.recv_from(&mut buf) {
                    let Some((name, dst, end)) = buf.get(3..n).and_then(target) else {
                        continue;
                    };
                    log.lock().unwrap().push(format!("udp {name}"));
                    *client.lock().unwrap() = Some(from);
                    let _ = out.send_to(&buf[3 + end..n], dst);
                }
            });
            // The association lives as long as its control connection.
            let _ = conn.read(&mut [0u8; 1]);
        }
        _ => {}
    }
}

fn proxy(log: &Log) -> SocketAddrV4 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let SocketAddr::V4(addr) = listener.local_addr().unwrap() else {
        unreachable!()
    };
    let log = log.clone();
    spawn(move || {
        for conn in listener.incoming().flatten() {
            let log = log.clone();
            spawn(move || serve(conn, &log));
        }
    });
    addr
}

/// What `bash` (a program apart from this process, which the router takes for itself) prints.
fn bash(script: &str) -> String {
    let out = Command::new("bash").args(["-c", script]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Sends `text` over `proto` (`tcp`/`udp`) from `bash` and returns the echo.
fn echo(proto: &str, dst: &str, text: &str) -> String {
    let (host, port) = dst.split_once(':').unwrap();
    bash(&format!(
        "exec 3<>/dev/{proto}/{host}/{port} && printf {text} >&3 && read -t 5 -N {} r <&3; \
         printf %s \"$r\"",
        text.len()
    ))
}

/// The first row `pick` takes, waiting for its counters to settle.
fn row(r: &Router, pick: impl Fn(&FlowRow) -> bool) -> FlowRow {
    let end = Instant::now() + WAIT;
    loop {
        let rows = r.flows(true);
        if let Some(row) = rows.iter().find(|f| pick(f)) {
            return row.clone();
        }
        assert!(Instant::now() < end, "no such row: {rows:#?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn rule(name: &str, protocol: Protocol, ports: &[&str]) -> Rule {
    Rule {
        name: name.into(),
        enabled: true,
        protocol,
        ports: ports.iter().map(|p| (*p).to_owned()).collect(),
        ips: vec![],
        domains: vec![],
        apps: vec![],
    }
}

fn fwmark_rules() -> usize {
    ip("-4 rule")
        .matches("fwmark 0x10000000/0x10000000")
        .count()
}

#[test]
#[ignore = "needs ip, nft, bash and a fresh network namespace: unshare -rn <this binary> --ignored"]
fn the_router_carries_tcp_udp_programs_and_domains_through_the_proxy() {
    // This namespace's links (sysfs may still show the host's).
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
    internet();
    ip("addr add 10.77.0.1/24 dev va");
    ip("link set va up");
    assert_eq!(
        echo("tcp", "10.77.0.2:7000", "plain"),
        "plain",
        "the veth works"
    );

    let log: Log = Arc::default();
    let proxy = proxy(&log);
    let proxied = |entry: &str| log.lock().unwrap().iter().any(|l| l == entry);

    // What a crashed session leaves: the table and two policy rules.
    cmd("nft", "add table inet zapret_hub_route");
    for _ in 0..2 {
        ip("-4 rule add fwmark 0x10000000/0x10000000 lookup 100");
    }

    let dir = tempfile::tempdir().unwrap();
    let r = Router::new(dir.path().join("routes.toml"));
    let mut ips = rule("ips", Protocol::Both, &["7000", "7001"]);
    ips.ips = vec![format!("{ECHO}/32")];
    let mut prog = rule("prog", Protocol::Tcp, &["7100"]);
    prog.apps = vec!["bash".into()];
    let mut dom = rule("dom", Protocol::Tcp, &["7000"]);
    dom.domains = vec!["echo.test".into()];
    r.edit(|list| {
        list.extend([ips, prog, dom]);
        Ok(())
    })
    .unwrap();
    r.start(Setup {
        dll: Default::default(),
        proxy,
    })
    .unwrap();
    assert_eq!(r.status().state, RouterState::Running);
    assert_eq!(fwmark_rules(), 1, "the stale rules went, one is in force");
    until("the proxy check passes", || {
        matches!(r.status().proxy, ProxyState::Ok { .. })
    });
    let is_bash = |f: &FlowRow| f.program.as_deref().is_some_and(|p| p.ends_with("/bash"));

    // TCP by address.
    assert_eq!(echo("tcp", "10.77.0.2:7000", "hello"), "hello");
    assert!(proxied("tcp 10.77.0.2:7000"), "{log:?}");
    let tcp = row(&r, |f| {
        f.protocol == "tcp" && f.destination == "10.77.0.2:7000" && f.down >= 5.0
    });
    assert_eq!((tcp.rule.as_deref(), &tcp.via), (Some("ips"), &Via::Proxy));
    assert_eq!(tcp.up, 5.0);
    assert!(is_bash(&tcp), "{tcp:?}");

    // UDP by address, through UDP ASSOCIATE; the answer comes from where it was sent.
    assert_eq!(echo("udp", "10.77.0.2:7001", "ping"), "ping");
    assert!(proxied("udp 10.77.0.2:7001"), "{log:?}");
    let udp = row(&r, |f| {
        f.protocol == "udp" && f.destination == "10.77.0.2:7001" && f.down >= 4.0
    });
    assert_eq!((udp.rule.as_deref(), &udp.via), (Some("ips"), &Via::Proxy));

    // A program rule: bash on its port.
    assert_eq!(echo("tcp", "10.77.0.2:7100", "prog"), "prog");
    assert!(proxied("tcp 10.77.0.2:7100"), "{log:?}");
    let by_program = row(&r, |f| f.destination == "10.77.0.2:7100");
    assert_eq!(
        (by_program.rule.as_deref(), &by_program.via),
        (Some("prog"), &Via::Proxy)
    );

    // The hub's own process is never sent through the proxy.
    let mut own = TcpStream::connect_timeout(&"10.77.0.2:7000".parse().unwrap(), WAIT).unwrap();
    own.write_all(b"self").unwrap();
    let mut back = [0u8; 4];
    own.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"self");
    let port = own.local_addr().unwrap().port();
    let looped = row(&r, |f| f.local_port == port);
    assert_eq!(
        looped.via,
        Via::Direct {
            reason: DirectReason::Loop
        }
    );
    drop(own);

    // New rules apply while a connection lives; it goes on. Another program on the program
    // rule's port then goes direct.
    let live = Command::new("bash")
        .args([
            "-c",
            "exec 3<>/dev/tcp/10.77.0.2/7000 && printf hello >&3 && read -t 5 -N 5 a <&3; \
             sleep 2; printf world >&3; read -t 5 -N 5 b <&3; printf %s%s \"$a\" \"$b\"",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_secs(1));
    r.edit(|list| {
        list[1].apps = vec!["nothing-here".into()];
        let mut extra = rule("extra", Protocol::Tcp, &[]);
        extra.ips = vec!["10.77.0.9/32".into()];
        list.push(extra);
        Ok(())
    })
    .unwrap();
    let chain = cmd("nft", "list chain inet zapret_hub_route tcp_match");
    assert!(chain.contains("10.77.0.9"), "{chain}");
    let out = live.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "helloworld");
    let before = log.lock().unwrap().len();
    assert_eq!(echo("tcp", "10.77.0.2:7100", "other"), "other");
    let other = row(&r, |f| {
        f.destination == "10.77.0.2:7100" && f.rule.is_none() && f.down >= 5.0
    });
    assert_eq!(
        other.via,
        Via::Direct {
            reason: DirectReason::NoRule
        }
    );
    assert!(is_bash(&other), "{other:?}");
    assert_eq!(log.lock().unwrap().len(), before, "not through the proxy");

    // A domain rule: the sniffed answer puts the name's address in force.
    let resolver = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    resolver.set_read_timeout(Some(WAIT)).unwrap();
    let mut query = vec![0x43, 0x21, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    query.extend(b"\x04echo\x04test\x00\x00\x01\x00\x01");
    resolver.send_to(&query, (ECHO, 53)).unwrap();
    let mut buf = [0u8; 512];
    resolver.recv_from(&mut buf).expect("a DNS answer");
    until("the name's address is routed", || {
        cmd("nft", "list chain inet zapret_hub_route tcp_match").contains("10.77.0.3")
    });
    assert_eq!(echo("tcp", "10.77.0.3:7000", "named"), "named");
    assert!(proxied("tcp echo.test:7000"), "{log:?}");
    let named = row(&r, |f| f.destination == "10.77.0.3:7000");
    assert_eq!(
        (named.rule.as_deref(), named.domain.as_deref(), &named.via),
        (Some("dom"), Some("echo.test"), &Via::Proxy)
    );

    // Stopped: nothing left behind, traffic goes directly.
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
    assert_eq!(fwmark_rules(), 0);
    assert!(!cmd("nft", "list tables").contains("zapret_hub_route"));
    assert_eq!(ip("-4 route show table 100"), "");
    let count = log.lock().unwrap().len();
    assert_eq!(echo("tcp", "10.77.0.2:7000", "after"), "after");
    assert_eq!(echo("udp", "10.77.0.2:7001", "after"), "after");
    assert_eq!(log.lock().unwrap().len(), count, "not through the proxy");
}

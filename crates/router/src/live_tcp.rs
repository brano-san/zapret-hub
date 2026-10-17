//! The TCP and DNS side of a running session: reflected connections, the local acceptor and the
//! names learned from sniffed DNS answers.

use std::io;
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::divert::{Address, Handle, MAX_PACKET};
use crate::flows::{self, DirectReason, FlowKey, QUEUE, Via};
use crate::owner::{self, PID_WAIT};
use crate::packet::{self, ACK, SYN, Tcp, parse_tcp, parse_udp, rewrite_tcp};
use crate::rules::Protocol;
use crate::socks::{self, OPEN_TIMEOUT, SocksError};
use crate::tcp::{self, Verdict};
use crate::{Live, ProxyState, Table, dns, spawn};

impl Live {
    /// Through the handle that captured the packet (it still sends while draining after a stop),
    /// else through the capture in force.
    fn out(&self, handle: Option<&Handle>, packet: &[u8], addr: &Address) {
        match handle {
            Some(h) => {
                let _ = h.send(packet, addr);
            }
            None => self.inject(packet, addr),
        }
    }

    fn reflect(
        &self,
        handle: Option<&Handle>,
        packet: &[u8],
        addr: &Address,
        src: SocketAddrV4,
        dst: SocketAddrV4,
    ) {
        let mut p = packet.to_vec();
        rewrite_tcp(&mut p, src, dst);
        let mut a = addr.as_inbound();
        self.dll.checksums(&mut p, &mut a);
        self.out(handle, &p, &a);
    }

    pub(crate) fn domain_of(&self, dst: SocketAddrV4, now: Instant) -> Option<String> {
        self.names
            .lock()
            .unwrap()
            .name_of(*dst.ip(), now)
            .map(str::to_owned)
    }

    /// `rule` is empty for a connection caught for a program rule that matches none.
    fn open_row(&self, key: FlowKey, rule: &str, exe: Option<&str>, via: Via, now: Instant) {
        let domain = self.domain_of(key.dst, now);
        let mut stats = self.stats.lock().unwrap();
        stats.open(key, rule, via, now);
        if let Some(d) = domain {
            stats.set_domain(key, d);
        }
        if let Some(exe) = exe {
            stats.set_program(key, exe.to_owned());
        }
    }

    /// A connection left alone: its row opens and closes at once (no NAT entry ends it later).
    fn direct_row(
        &self,
        key: FlowKey,
        rule: &str,
        exe: Option<&str>,
        reason: DirectReason,
        now: Instant,
    ) {
        self.open_row(key, rule, exe, Via::Direct { reason }, now);
        self.stats.lock().unwrap().close(key, now);
    }

    pub(crate) fn on_tcp(
        self: &Arc<Self>,
        handle: Option<&Handle>,
        packet: &[u8],
        seg: &Tcp,
        addr: Address,
    ) {
        let now = Instant::now();
        // Only the acceptor sends from its port; a reply without an entry is dropped.
        if seg.src.port() == self.acceptor_port {
            let verdict = self.nat.lock().unwrap().from_acceptor(seg, now);
            if let Verdict::Rewrite { src, dst } = verdict {
                self.reflect(handle, packet, &addr, src, dst);
            }
            return;
        }
        if self.is_stopped() || !packet::unicast(*seg.src.ip(), *seg.dst.ip()) {
            return self.out(handle, packet, &addr);
        }
        let verdict = self
            .nat
            .lock()
            .unwrap()
            .from_client(seg, self.acceptor_port, now);
        let key = FlowKey {
            protocol: Protocol::Tcp,
            local_port: seg.src.port(),
            dst: seg.dst,
        };
        match verdict {
            Verdict::Rewrite { src, dst } => {
                // A retransmitted SYN of a held connection: the held one goes out once a filter
                // covers its destination.
                if seg.flags & (SYN | ACK) == SYN && !self.waiting.lock().unwrap().covers(seg.dst) {
                    return;
                }
                self.reflect(handle, packet, &addr, src, dst)
            }
            Verdict::Pass => self.out(handle, packet, &addr),
            Verdict::Drop => {}
            Verdict::Marked => {
                let table = self.table.read().unwrap().clone();
                if let Some(rule) = table.matches(Protocol::Tcp, seg.dst) {
                    self.direct_row(key, rule, None, DirectReason::Marked, now);
                }
                self.out(handle, packet, &addr);
            }
            Verdict::Syn => self.on_syn(handle, packet, seg, addr, key, now),
        }
    }

    fn on_syn(
        self: &Arc<Self>,
        handle: Option<&Handle>,
        packet: &[u8],
        seg: &Tcp,
        addr: Address,
        key: FlowKey,
        now: Instant,
    ) {
        let table = self.table.read().unwrap().clone();
        if table.matches(Protocol::Tcp, seg.dst).is_none()
            && !table.wants_owner(Protocol::Tcp, seg.dst.port())
        {
            return self.out(handle, packet, &addr);
        }
        // ponytail: like a new UDP flow, an owner missing from events and tables blocks capture up
        // to 50 ms per SYN (once per port).
        let pid = self.tcp_owners.pid(key.local_port, PID_WAIT, |p| {
            owner::table_pid(Protocol::Tcp, p)
        });
        let exe = pid.and_then(|p| self.images.get(p));
        let exe = exe.as_deref();
        let Some(rule) = table.matches_flow(Protocol::Tcp, seg.dst, exe) else {
            self.direct_row(key, "", exe, DirectReason::NoRule, now);
            return self.out(handle, packet, &addr);
        };
        let state = self.proxy_state.lock().unwrap().clone();
        let proxy_pid = *self.proxy_pid.lock().unwrap();
        if let Some(reason) = flows::decide(&state, pid, std::process::id(), proxy_pid) {
            self.direct_row(key, rule, exe, reason, now);
            return self.out(handle, packet, &addr);
        }
        // ponytail: a program rule's connection to a new address waits for a filter reopen (its
        // address joins the held part); a browser in `apps` reopens it per address. Upgrade: a
        // `tcp.SrcPort == p` part for the ports of matched sockets.
        let verdict = {
            // Held under one lock with the admit, so `release` cannot miss this SYN.
            let mut waiting = self.waiting.lock().unwrap();
            let covered = waiting.covers(seg.dst);
            if !covered && waiting.syns.len() >= QUEUE {
                drop(waiting);
                return self.out(handle, packet, &addr);
            }
            let verdict = self.nat.lock().unwrap().admit(seg, self.acceptor_port, now);
            self.open_row(key, rule, exe, Via::Opening, now);
            if !covered {
                waiting.syns.push((packet.to_vec(), addr));
                drop(waiting);
                self.recompose_soon();
                return;
            }
            verdict
        };
        if let Verdict::Rewrite { src, dst } = verdict {
            self.reflect(handle, packet, &addr, src, dst);
        }
    }

    /// Hands the held SYNs (admitted already) the filter just put in force covers (`active` and
    /// its held destinations) to it, keeping the rest; when it could not open, forgets their
    /// entries and sends them unchanged.
    pub(crate) fn release(self: &Arc<Self>, active: Option<(Arc<Table>, Vec<Ipv4Addr>)>) {
        let opened = active.is_some();
        let syns = {
            let mut waiting = self.waiting.lock().unwrap();
            match active {
                Some((table, held)) => waiting.activate(table, held),
                None => std::mem::take(&mut waiting.syns),
            }
        };
        let now = Instant::now();
        for (packet, addr) in syns {
            let Some(seg) = parse_tcp(&packet) else {
                self.inject(&packet, &addr);
                continue;
            };
            if opened {
                self.on_tcp(None, &packet, &seg, addr);
                continue;
            }
            self.nat.lock().unwrap().forget(seg.src.port());
            let key = FlowKey {
                protocol: Protocol::Tcp,
                local_port: seg.src.port(),
                dst: seg.dst,
            };
            let reason = DirectReason::Failed {
                error: "the capture filter did not open".into(),
            };
            let mut stats = self.stats.lock().unwrap();
            stats.set_via(key, Via::Direct { reason });
            stats.close(key, now);
            drop(stats);
            self.inject(&packet, &addr);
        }
    }

    /// Re-applies the rules with the names known now; a refused filter shows as the rules error.
    pub(crate) fn recompose(self: &Arc<Self>) {
        if self.is_stopped() {
            return;
        }
        let error = self.apply(None).err();
        if let Some(inner) = self.inner.upgrade() {
            inner.rules.lock().unwrap().filter_error = error;
        }
    }

    /// Spawns one `recompose` for any number of requests made before it starts.
    pub(crate) fn recompose_soon(self: &Arc<Self>) {
        if !self.recompose_pending.swap(true, Ordering::SeqCst) {
            let l = self.clone();
            spawn(move || l.recompose());
        }
    }

    /// Records the addresses of names some rule wants; ends when the DNS handle shuts down.
    pub(crate) fn dns_loop(self: &Arc<Self>, handle: &Handle) {
        let mut buf = vec![0u8; MAX_PACKET];
        while let Ok((n, _)) = handle.recv(&mut buf) {
            let Some(answer) = parse_udp(&buf[..n]).and_then(|u| dns::parse(u.payload)) else {
                continue;
            };
            if !self.base.lock().unwrap().wants(&answer.name) {
                continue;
            }
            if self.names.lock().unwrap().record(&answer, Instant::now()) {
                self.recompose();
            }
        }
    }

    /// Ends on the first connection after the session stopped (`shut` makes one).
    pub(crate) fn accept_loop(self: &Arc<Self>) {
        for conn in self.acceptor.incoming() {
            if self.is_stopped() {
                return;
            }
            match conn {
                Ok(stream) => {
                    let l = self.clone();
                    spawn(move || l.relay_tcp(stream));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    // ponytail: two threads per connection; a pool or IOCP if connections reach the hundreds.
    fn relay_tcp(self: Arc<Self>, client: TcpStream) {
        let Ok(SocketAddr::V4(peer)) = client.peer_addr() else {
            return tcp::reset(client);
        };
        let port = peer.port();
        let orig = {
            let mut nat = self.nat.lock().unwrap();
            let orig = nat.original(port, *peer.ip());
            if orig.is_some() {
                nat.accepted(port);
            }
            orig
        };
        // Not a reflected connection: nobody from the LAN rides the proxy.
        let Some(orig) = orig else {
            return tcp::reset(client);
        };
        let key = FlowKey {
            protocol: Protocol::Tcp,
            local_port: port,
            dst: orig,
        };
        let domain = self.domain_of(orig, Instant::now());
        let up = match socks::connect(self.proxy, orig, domain.as_deref(), OPEN_TIMEOUT) {
            Ok(up) => up,
            Err(e) => {
                let now = Instant::now();
                self.nat.lock().unwrap().mark_direct(orig, now);
                // Ruling P6: a refused CONNECT is a failure, not "no UDP".
                let reason = match e {
                    SocksError::Refused(_) => DirectReason::Failed {
                        error: e.to_string(),
                    },
                    _ => DirectReason::from(&e),
                };
                let mut stats = self.stats.lock().unwrap();
                stats.set_via(key, Via::Direct { reason });
                stats.close(key, now);
                drop(stats);
                if matches!(e, SocksError::AuthRequired) {
                    self.set_proxy(ProxyState::AuthRequired);
                }
                return tcp::reset(client);
            }
        };
        self.stats.lock().unwrap().set_via(key, Via::Proxy);
        let _ = self.pipe_both(key, &client, &up);
        self.end_conn(key);
        self.stats.lock().unwrap().close(key, Instant::now());
    }

    /// Client to proxy on a new thread, proxy to client here. An error, a stop or a gone NAT entry
    /// shuts both sockets down, which also ends the other direction; after an EOF the other
    /// direction goes on (a half-close).
    fn pipe_both(
        self: &Arc<Self>,
        key: FlowKey,
        client: &TcpStream,
        up: &TcpStream,
    ) -> io::Result<()> {
        self.conns
            .lock()
            .unwrap()
            .insert(key, (client.try_clone()?, up.try_clone()?));
        if self.is_stopped() {
            return Ok(()); // `shut` may have run before the insert
        }
        let l = self.clone();
        let stop = move || {
            l.is_stopped()
                || l.nat
                    .lock()
                    .unwrap()
                    .original(key.local_port, *key.dst.ip())
                    .is_none()
        };
        let (from, to, l, stop2) = (
            client.try_clone()?,
            up.try_clone()?,
            self.clone(),
            stop.clone(),
        );
        let upload = std::thread::spawn(move || {
            let r = tcp::pipe(&from, &to, &stop2, &mut |n| {
                l.stats.lock().unwrap().add(key, n, 0, Instant::now());
            });
            if r.is_err() || stop2() {
                l.end_conn(key);
            }
        });
        let r = tcp::pipe(up, client, &stop, &mut |n| {
            self.stats.lock().unwrap().add(key, 0, n, Instant::now());
        });
        if r.is_err() || stop() {
            self.end_conn(key);
        }
        let _ = upload.join();
        Ok(())
    }

    /// Shuts down both sockets of a proxied connection, which unblocks its pipes.
    fn end_conn(&self, key: FlowKey) {
        if let Some((a, b)) = self.conns.lock().unwrap().remove(&key) {
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        }
    }

    /// Expires names (not those of live flows) and NAT entries (their rows close, their relays
    /// end), sends unaccepted SYNs' destinations direct, then rebuilds the filter (no reopen when
    /// its text is the same).
    pub(crate) fn tcp_tick(self: &Arc<Self>, now: Instant) {
        // A live flow to an address known only from DNS keeps it captured (proxied UDP mid-call).
        let mut live: Vec<_> = self
            .flows
            .lock()
            .unwrap()
            .keys()
            .map(|k| *k.dst.ip())
            .collect();
        live.extend(
            self.nat
                .lock()
                .unwrap()
                .destinations()
                .iter()
                .map(|d| *d.ip()),
        );
        self.names.lock().unwrap().expire(now, &live);
        let (gone, unaccepted) = {
            let mut nat = self.nat.lock().unwrap();
            (nat.expire(now), nat.unaccepted(now))
        };
        let key = |local_port, dst| FlowKey {
            protocol: Protocol::Tcp,
            local_port,
            dst,
        };
        for (port, dst) in gone {
            self.end_conn(key(port, dst));
            self.stats.lock().unwrap().close(key(port, dst), now);
        }
        self.waiting.lock().unwrap().forget(&unaccepted);
        for (port, dst) in unaccepted {
            log::warn!(
                "the local acceptor never saw the connection to {dst}: Windows Firewall may block \
                 Zapret Hub; the address goes direct for 60 s"
            );
            let mut stats = self.stats.lock().unwrap();
            stats.set_via(
                key(port, dst),
                Via::Direct {
                    reason: DirectReason::NotAccepted,
                },
            );
            stats.close(key(port, dst), now);
        }
        self.recompose();
    }
}

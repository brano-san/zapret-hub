//! TPROXY'd UDP: each datagram's original destination comes with it; a flow (local port,
//! destination) relays through a SOCKS5 `UDP ASSOCIATE` or a marked direct socket, and answers go
//! back from a socket bound (`IP_TRANSPARENT`) to the destination the program sent to.

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use socket2::SockRef;

use super::{Live, spawn};
use crate::flows::{self, DirectReason, FlowKey, ProxyState, QUEUE, Via};
use crate::linux::{nft, sockopt};
use crate::owner;
use crate::packet::{parse_socks_udp_from, socks_header_to};
use crate::rules::Protocol;
use crate::socks::{self, OPEN_TIMEOUT, SocksError};

const MAX_DATAGRAM: usize = 65536;
/// How long a relay read waits before it looks whether its flow is still current.
const RELAY_POLL: Duration = Duration::from_secs(1);

pub(super) struct Relay {
    _control: TcpStream,
    socket: UdpSocket,
    relay: SocketAddrV4,
    /// Named in each datagram's header instead of the address, when known.
    domain: Option<String>,
}

impl Relay {
    fn send(&self, dst: SocketAddrV4, payload: &[u8]) {
        let mut datagram = socks_header_to(dst, self.domain.as_deref());
        datagram.extend_from_slice(payload);
        let _ = self.socket.send_to(&datagram, self.relay);
    }
}

pub(super) enum Up {
    /// The datagrams waiting for the proxy; the oldest go when more than `QUEUE` wait.
    Opening(VecDeque<Vec<u8>>),
    Proxy(Arc<Relay>),
    Direct(Arc<UdpSocket>, DirectReason),
}

impl Up {
    /// The socket answers come in on.
    fn socket(&self) -> Option<&UdpSocket> {
        match self {
            Up::Opening(_) => None,
            Up::Proxy(r) => Some(&r.socket),
            Up::Direct(s, _) => Some(s),
        }
    }
}

/// A UDP flow: its row's key and the program's address (two addresses may send from one port).
// ponytail: such two flows share one row (`Stats` is keyed by `FlowKey`, as on Windows).
pub(super) type UdpKey = (FlowKey, Ipv4Addr);

pub(super) struct Flow {
    client: SocketAddrV4,
    /// Bound to the flow's destination: answers reach the program from where it sent.
    reply: Arc<UdpSocket>,
    up: Up,
}

/// A socket to `dst` whose packets skip the routing (`MARK_OWN`).
fn direct_socket(dst: SocketAddrV4) -> std::io::Result<Arc<UdpSocket>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    SockRef::from(&socket).set_mark(nft::MARK_OWN)?;
    socket.connect(dst)?;
    socket.set_read_timeout(Some(RELAY_POLL))?;
    Ok(Arc::new(socket))
}

impl Live {
    /// Ends with the session (the socket's read timeout lets it look).
    pub(super) fn udp_loop(self: &Arc<Self>) {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        while !self.is_stopped() {
            match sockopt::recv_orig(&self.tproxy, &mut buf) {
                Ok((n, client, Some(dst))) => self.on_datagram(client, dst, &buf[..n]),
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    fn on_datagram(self: &Arc<Self>, client: SocketAddrV4, dst: SocketAddrV4, payload: &[u8]) {
        let key = FlowKey {
            protocol: Protocol::Udp,
            local_port: client.port(),
            dst,
        };
        let id = (key, *client.ip());
        let now = Instant::now();
        let up = payload.len() as u64;
        let mut flows = self.flows.lock().unwrap();
        if let Some(flow) = flows.get_mut(&id) {
            match &mut flow.up {
                Up::Opening(pending) => {
                    if pending.len() == QUEUE {
                        pending.pop_front();
                    }
                    pending.push_back(payload.to_vec());
                    return; // counted when it goes
                }
                Up::Proxy(relay) => {
                    let relay = relay.clone();
                    drop(flows);
                    relay.send(dst, payload);
                }
                Up::Direct(socket, _) => {
                    let socket = socket.clone();
                    drop(flows);
                    let _ = socket.send(payload);
                }
            }
            self.stats.lock().unwrap().add(key, up, 0, now);
            return;
        }
        drop(flows);
        // Only this thread adds flows: the key stays vacant until the insert below.
        let pid = owner::table_pid_other(Protocol::Udp, client.port());
        let exe = pid.and_then(|p| self.images.get(p));
        let table = self.table.read().unwrap().clone();
        let (rule, direct) =
            flows::route(&table, Protocol::Udp, dst, exe.as_deref(), self.decide(pid));
        let domain = self.domain_of(dst, now);
        let opened = sockopt::transparent_udp(dst, nft::MARK_OWN).and_then(|reply| {
            let up = match &direct {
                Some(reason) => Up::Direct(direct_socket(dst)?, reason.clone()),
                None => Up::Opening(VecDeque::from([payload.to_vec()])),
            };
            Ok((Arc::new(reply), up))
        });
        let (reply, up_side) = match opened {
            Ok(x) => x,
            Err(e) => {
                // Dropped: the program's retry tries again.
                log::warn!("cannot relay UDP to {dst}: {e}");
                return;
            }
        };
        let via = direct
            .clone()
            .map_or(Via::Opening, |reason| Via::Direct { reason });
        self.open_row(key, rule, exe.as_deref(), domain.clone(), via, now);
        let direct_socket = match &up_side {
            Up::Direct(socket, _) => Some(socket.clone()),
            _ => None,
        };
        // In the table before its reader starts, which stops once the flow is gone.
        self.flows.lock().unwrap().insert(
            id,
            Flow {
                client,
                reply: reply.clone(),
                up: up_side,
            },
        );
        let l = self.clone();
        match direct_socket {
            Some(socket) => {
                let _ = socket.send(payload);
                self.stats.lock().unwrap().add(key, up, 0, now);
                spawn(move || l.relay_rx(id, client, &reply, &socket, None));
            }
            None => spawn(move || l.open_flow(id, domain)),
        }
    }

    // ponytail: a control TCP that drops after the flow opened is not watched; proxied flows close
    // when two 10 s checks in a row see the proxy fail (as on Windows).
    fn open_flow(self: Arc<Self>, id: UdpKey, domain: Option<String>) {
        let key = id.0;
        let started = Instant::now();
        let opened = socks::udp_associate(self.proxy, key.dst, OPEN_TIMEOUT).and_then(|assoc| {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            socket.set_read_timeout(Some(RELAY_POLL))?;
            Ok(Arc::new(Relay {
                _control: assoc.control,
                socket,
                relay: assoc.relay,
                domain,
            }))
        });
        let latency_ms = started.elapsed().as_millis() as u32;
        let mut flows = self.flows.lock().unwrap();
        // Closed (idle or stopped) while opening: nothing to do.
        let Some(flow) = flows.get_mut(&id) else {
            return;
        };
        let Up::Opening(pending) = &mut flow.up else {
            return;
        };
        let pending = std::mem::take(pending);
        let up: u64 = pending.iter().map(|p| p.len() as u64).sum();
        let (client, reply) = (flow.client, flow.reply.clone());
        // Pending datagrams go out under the lock, so later ones cannot overtake them.
        let via = match opened {
            Ok(relay) => {
                for p in &pending {
                    relay.send(key.dst, p);
                }
                flow.up = Up::Proxy(relay.clone());
                drop(flows);
                let l = self.clone();
                spawn(move || l.relay_rx(id, client, &reply, &relay.socket, Some(relay.relay)));
                self.update_proxy(|current| {
                    (*current == ProxyState::NoUdp).then_some(ProxyState::Ok { latency_ms })
                });
                Via::Proxy
            }
            Err(e) => {
                let mut reason = DirectReason::from(&e);
                match direct_socket(key.dst) {
                    Ok(socket) => {
                        for p in &pending {
                            let _ = socket.send(p);
                        }
                        flow.up = Up::Direct(socket.clone(), reason.clone());
                        drop(flows);
                        let l = self.clone();
                        spawn(move || l.relay_rx(id, client, &reply, &socket, None));
                    }
                    Err(err) => {
                        flows.remove(&id);
                        drop(flows);
                        reason = DirectReason::Failed {
                            error: err.to_string(),
                        };
                    }
                }
                match e {
                    SocksError::AuthRequired => self.set_proxy(ProxyState::AuthRequired),
                    SocksError::Refused(7) => self.set_proxy(ProxyState::NoUdp),
                    _ => {}
                }
                Via::Direct { reason }
            }
        };
        let mut stats = self.stats.lock().unwrap();
        stats.set_via(key, via);
        stats.add(key, up, 0, Instant::now());
    }

    /// Answers from `socket` (the proxy's relay when `relay` is set, else the destination) go to
    /// the program through `reply`, until the flow ends or changes.
    // ponytail: a proxied answer from another address than the flow's still comes from the
    // flow's destination (one reply socket per flow).
    fn relay_rx(
        &self,
        id: UdpKey,
        client: SocketAddrV4,
        reply: &UdpSocket,
        socket: &UdpSocket,
        relay: Option<SocketAddrV4>,
    ) {
        let key = id.0;
        let mut buf = vec![0u8; MAX_DATAGRAM];
        loop {
            let current = self
                .flows
                .lock()
                .unwrap()
                .get(&id)
                .and_then(|f| f.up.socket())
                .is_some_and(|s| std::ptr::eq(s, socket));
            if !current || self.is_stopped() {
                return;
            }
            let (n, from) = match socket.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                Err(_) => {
                    // An ICMP unreachable: skip it without spinning.
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            let data = match relay {
                Some(r) if from != SocketAddr::V4(r) => continue,
                Some(_) => match parse_socks_udp_from(&buf[..n], key.dst) {
                    Some((_, data)) => data,
                    None => continue,
                },
                None => &buf[..n],
            };
            let _ = reply.send_to(data, client);
            self.stats
                .lock()
                .unwrap()
                .add(key, 0, data.len() as u64, Instant::now());
        }
    }

    /// The flows a proxy change ends: proxied ones while it fails, those sent direct by a
    /// failure once it is back.
    pub(super) fn udp_closing(&self, failing: bool, recovered: bool) -> Vec<FlowKey> {
        self.flows
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, f)| match &f.up {
                Up::Proxy(_) => failing,
                Up::Direct(_, reason) => recovered && reason.proxy_failure(),
                Up::Opening(_) => false,
            })
            .map(|(k, _)| k.0)
            .collect()
    }

    /// Drops the row's flows (their threads end within `RELAY_POLL`) and closes the row.
    pub(super) fn close_flow(&self, key: FlowKey, now: Instant) {
        self.flows.lock().unwrap().retain(|k, _| k.0 != key);
        self.stats.lock().unwrap().close(key, now);
    }
}

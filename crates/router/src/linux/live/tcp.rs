//! Redirected TCP: the acceptor learns each connection's destination (`SO_ORIGINAL_DST`) and
//! owner, then relays it through the proxy or, marked so it is not redirected again, directly.

use std::io;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Live, spawn};
use crate::flows::{self, DirectReason, FlowKey, ProxyState, Via};
use crate::linux::{nft, sockopt};
use crate::owner;
use crate::rules::Protocol;
use crate::socks::{self, OPEN_TIMEOUT, SocksError};
use crate::tcp;

impl Live {
    /// Ends on the first connection after the session stopped (`shut` makes one).
    pub(super) fn accept_loop(self: &Arc<Self>) {
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

    // ponytail: two threads per connection and a /proc walk for its owner; a pool and a cached
    // socket map if connections reach the hundreds.
    fn relay_tcp(self: Arc<Self>, client: TcpStream) {
        let (Ok(SocketAddr::V4(peer)), Ok(orig)) =
            (client.peer_addr(), sockopt::original_dst(&client))
        else {
            return tcp::reset(client);
        };
        // Loopback is never redirected: a connection made to the acceptor itself, whose own
        // address is then the "original" one.
        if orig.ip().is_loopback() {
            return tcp::reset(client);
        }
        let key = FlowKey {
            protocol: Protocol::Tcp,
            local_port: peer.port(),
            dst: orig,
        };
        let pid = owner::table_pid(Protocol::Tcp, peer.port());
        let exe = pid.and_then(|p| self.images.get(p));
        let table = self.table.read().unwrap().clone();
        let (rule, direct) = flows::route(
            &table,
            Protocol::Tcp,
            orig,
            exe.as_deref(),
            self.decide(pid),
        );
        let now = Instant::now();
        let domain = self.domain_of(orig, now);
        let via = direct
            .clone()
            .map_or(Via::Opening, |reason| Via::Direct { reason });
        self.open_row(key, rule, exe.as_deref(), domain.clone(), via, now);
        let proxied = match direct {
            Some(_) => None,
            None => match socks::connect(self.proxy, orig, domain.as_deref(), OPEN_TIMEOUT) {
                Ok(up) => {
                    self.stats.lock().unwrap().set_via(key, Via::Proxy);
                    Some(up)
                }
                Err(e) => {
                    // A refused CONNECT is a failure, not "no UDP" (as on Windows); the
                    // connection still goes, directly.
                    let reason = match e {
                        SocksError::Refused(_) => DirectReason::Failed {
                            error: e.to_string(),
                        },
                        _ => DirectReason::from(&e),
                    };
                    self.stats
                        .lock()
                        .unwrap()
                        .set_via(key, Via::Direct { reason });
                    if matches!(e, SocksError::AuthRequired) {
                        self.set_proxy(ProxyState::AuthRequired);
                    }
                    None
                }
            },
        };
        let up = match proxied {
            Some(up) => up,
            None => match sockopt::connect_marked(orig, nft::MARK_OWN, OPEN_TIMEOUT) {
                Ok(up) => up,
                Err(e) => {
                    let mut stats = self.stats.lock().unwrap();
                    let reason = DirectReason::Failed {
                        error: e.to_string(),
                    };
                    stats.set_via(key, Via::Direct { reason });
                    stats.close(key, Instant::now());
                    drop(stats);
                    return tcp::reset(client);
                }
            },
        };
        let _ = self.pipe_both(key, &client, &up);
        self.end_conn(key);
        self.stats.lock().unwrap().close(key, Instant::now());
    }

    /// Client to upstream on a new thread, upstream to client here. An error or a stop shuts both
    /// sockets down, which also ends the other direction; after an EOF the other direction goes
    /// on (a half-close).
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
        let (from, to, l) = (client.try_clone()?, up.try_clone()?, self.clone());
        let upload = std::thread::spawn(move || {
            let stop = || l.is_stopped();
            let r = tcp::pipe(&from, &to, &stop, &mut |n| {
                l.stats.lock().unwrap().add(key, n, 0, Instant::now());
            });
            if r.is_err() || stop() {
                l.end_conn(key);
            }
        });
        let stop = || self.is_stopped();
        let r = tcp::pipe(up, client, &stop, &mut |n| {
            self.stats.lock().unwrap().add(key, 0, n, Instant::now());
        });
        if r.is_err() || stop() {
            self.end_conn(key);
        }
        let _ = upload.join();
        Ok(())
    }

    /// Shuts down both sockets of a relayed connection, which unblocks its pipes.
    fn end_conn(&self, key: FlowKey) {
        if let Some((a, b)) = self.conns.lock().unwrap().remove(&key) {
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        }
    }
}

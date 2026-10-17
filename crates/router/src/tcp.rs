//! Reflection of chosen TCP connections into the local acceptor, streamdump-style.
//!
//! A client segment to the original destination goes back in as inbound from
//! `original ip : client port` to `client ip : acceptor port`; the acceptor's replies leave as
//! outbound non-loopback segments and go back in as inbound from the original destination.
//! Checksums are recomputed at send.

use crate::packet::{ACK, FIN, RST, SYN, Tcp};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpStream};
use std::os::windows::io::AsRawSocket;
use std::time::{Duration, Instant};
use windows::Win32::Networking::WinSock::{
    LINGER as SockLinger, SO_LINGER, SOCKET, SOL_SOCKET, setsockopt,
};

pub const IDLE: Duration = Duration::from_secs(120);
/// After FIN both ways or an RST, for the last ACK.
pub const LINGER: Duration = Duration::from_secs(10);
pub const DIRECT_FOR: Duration = Duration::from_secs(60);
/// Below the 3 s SYN retransmit of Windows, so the retry already goes direct.
pub const ACCEPT_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Pass,
    Marked,
    Syn,
    /// Rewrite the addresses and send the segment as inbound.
    Rewrite {
        src: SocketAddrV4,
        dst: SocketAddrV4,
    },
    Drop,
}

struct Entry {
    orig: SocketAddrV4,
    client_fin: bool,
    acceptor_fin: bool,
    closed: Option<Instant>,
    syn_at: Instant,
    accepted: bool,
    last: Instant,
}

impl Entry {
    fn see(&mut self, flags: u8, from_client: bool, now: Instant) {
        self.last = now;
        if flags & FIN != 0 {
            *(if from_client {
                &mut self.client_fin
            } else {
                &mut self.acceptor_fin
            }) = true;
        }
        if flags & RST != 0 || self.client_fin && self.acceptor_fin {
            self.closed.get_or_insert(now);
        }
    }
}

/// Entries by client port. Every removal is reported as `(client port, original destination)`
/// by `expire` or `unaccepted`, so the caller can close its flow row. An idle entry is reported
/// once more when it starts lingering, so the caller ends its connection while the acceptor's
/// FIN can still be reflected to the client.
// ponytail: keyed by client port alone; the same local port to another destination replaces a
// live entry. Upgrade: key by (client port, original destination) if that shows up.
#[derive(Default)]
pub struct Nat {
    entries: HashMap<u16, Entry>,
    marks: HashMap<SocketAddrV4, Instant>,
    /// Closed entries a new SYN replaced, reported by the next `expire`.
    replaced: Vec<(u16, SocketAddrV4)>,
}

fn bare_syn(seg: &Tcp) -> bool {
    seg.flags & (SYN | ACK) == SYN
}

impl Nat {
    pub fn from_client(&mut self, seg: &Tcp, acceptor: u16, now: Instant) -> Verdict {
        let port = seg.src.port();
        if bare_syn(seg)
            && let Some(e) = self.entries.get(&port)
            && e.closed.is_some()
        {
            // A new connection on this port: the lingering entry must not catch its segments.
            self.replaced.push((port, e.orig));
            self.entries.remove(&port);
        }
        if let Some(e) = self.entries.get_mut(&port)
            && e.orig == seg.dst
        {
            e.see(seg.flags, true, now);
            return to_acceptor(seg, acceptor);
        }
        if !bare_syn(seg) {
            Verdict::Pass
        } else if self.marked(seg.dst, now) {
            Verdict::Marked
        } else {
            Verdict::Syn
        }
    }

    pub fn admit(&mut self, seg: &Tcp, acceptor: u16, now: Instant) -> Verdict {
        let entry = Entry {
            orig: seg.dst,
            client_fin: false,
            acceptor_fin: false,
            closed: None,
            syn_at: now,
            accepted: false,
            last: now,
        };
        if let Some(old) = self.entries.insert(seg.src.port(), entry) {
            self.replaced.push((seg.src.port(), old.orig));
        }
        to_acceptor(seg, acceptor)
    }

    pub fn from_acceptor(&mut self, seg: &Tcp, now: Instant) -> Verdict {
        let port = seg.dst.port();
        match self.entries.get_mut(&port) {
            Some(e) if e.orig.ip() == seg.dst.ip() => {
                e.see(seg.flags, false, now);
                Verdict::Rewrite {
                    src: e.orig,
                    dst: SocketAddrV4::new(*seg.src.ip(), port),
                }
            }
            _ => Verdict::Drop,
        }
    }

    /// `peer` is the accepted socket's peer ip, which reflection set to the original one.
    pub fn original(&self, client_port: u16, peer: Ipv4Addr) -> Option<SocketAddrV4> {
        self.entries
            .get(&client_port)
            .map(|e| e.orig)
            .filter(|o| *o.ip() == peer)
    }

    pub fn accepted(&mut self, client_port: u16) {
        if let Some(e) = self.entries.get_mut(&client_port) {
            e.accepted = true;
        }
    }

    /// Removes entries the acceptor has not seen for `ACCEPT_WAIT`, marks their destinations.
    pub fn unaccepted(&mut self, now: Instant) -> Vec<(u16, SocketAddrV4)> {
        let gone: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| !e.accepted && now.duration_since(e.syn_at) >= ACCEPT_WAIT)
            .map(|(p, e)| (*p, e.orig))
            .collect();
        for (port, orig) in &gone {
            self.entries.remove(port);
            self.marks.insert(*orig, now);
        }
        gone
    }

    /// Removes the entry without marking its destination (its SYN goes out unchanged).
    pub fn forget(&mut self, client_port: u16) -> Option<SocketAddrV4> {
        self.entries.remove(&client_port).map(|e| e.orig)
    }

    pub fn mark_direct(&mut self, dst: SocketAddrV4, now: Instant) {
        self.marks.insert(dst, now);
    }

    pub fn clear_marks(&mut self) {
        self.marks.clear();
    }

    /// Closes idle entries (they linger too), removes closed entries past `LINGER` and old marks;
    /// returns the newly idle and the removed entries.
    pub fn expire(&mut self, now: Instant) -> Vec<(u16, SocketAddrV4)> {
        let mut gone = std::mem::take(&mut self.replaced);
        self.entries.retain(|port, e| {
            if e.closed.is_none() && now.duration_since(e.last) >= IDLE {
                e.closed = Some(now);
                gone.push((*port, e.orig));
            }
            let ended = e.closed.is_some_and(|c| now.duration_since(c) >= LINGER);
            if ended {
                gone.push((*port, e.orig));
            }
            !ended
        });
        self.marks
            .retain(|_, t| now.duration_since(*t) < DIRECT_FOR);
        gone
    }

    pub fn destinations(&self) -> Vec<SocketAddrV4> {
        self.entries.values().map(|e| e.orig).collect()
    }

    fn marked(&self, dst: SocketAddrV4, now: Instant) -> bool {
        self.marks
            .get(&dst)
            .is_some_and(|t| now.duration_since(*t) < DIRECT_FOR)
    }
}

fn to_acceptor(seg: &Tcp, acceptor: u16) -> Verdict {
    Verdict::Rewrite {
        src: SocketAddrV4::new(*seg.dst.ip(), seg.src.port()),
        dst: SocketAddrV4::new(*seg.src.ip(), acceptor),
    }
}

/// Copies `from` into `to` until EOF, then half-closes `to`. Checks `stop` every second while
/// reading; the caller's `stop` should also turn true once the NAT entry is gone. A write to a
/// peer that does not read blocks: the caller unblocks it with `shutdown(Both)` on both sockets
/// (a write timeout on Windows leaves the socket indeterminate, so none is set).
pub fn pipe(
    from: &TcpStream,
    to: &TcpStream,
    stop: &dyn Fn() -> bool,
    count: &mut dyn FnMut(u64),
) -> std::io::Result<()> {
    from.set_read_timeout(Some(Duration::from_secs(1)))?;
    let (mut from, mut to) = (from, to);
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) => return to.shutdown(Shutdown::Write),
            Ok(n) => {
                to.write_all(&buf[..n])?;
                count(n as u64);
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if stop() {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Closes `stream` with a zero linger, so the peer gets an RST.
pub fn reset(stream: TcpStream) {
    let linger = SockLinger {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: `stream` owns a live socket for the call; the slice covers exactly the bytes of
    // `linger`, a plain struct that outlives it, as `setsockopt(SO_LINGER)` expects. A failure
    // only leaves the normal close.
    unsafe {
        let bytes =
            std::slice::from_raw_parts((&raw const linger).cast::<u8>(), size_of::<SockLinger>());
        setsockopt(
            SOCKET(stream.as_raw_socket() as usize),
            SOL_SOCKET,
            SO_LINGER,
            Some(bytes),
        );
    }
    drop(stream);
}

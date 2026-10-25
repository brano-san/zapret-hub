//! Which process owns a local port (socket events, then the system's socket tables) and its image
//! path.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(target_os = "linux")]
mod procfs;
#[cfg(windows)]
mod win32;

#[cfg(target_os = "linux")]
pub use procfs::{
    image_path, listener_pid, port_pids, process_start, table_pid, table_pid_other, udp_pids,
};
#[cfg(windows)]
pub use win32::{image_path, listener_pid, process_start, table_pid, udp_pids};

pub const PID_WAIT: Duration = Duration::from_millis(50);

/// Local port and family (IPv6) -> PID, fed by socket events; `wait` covers an event that
/// trails its packet. `None` means "looked up, no owner found" and is not waited for again. An
/// IPv4 and an IPv6 socket of two programs may hold one port number.
#[derive(Default)]
pub struct Owners {
    map: Mutex<HashMap<(u16, bool), Option<u32>>>,
    changed: Condvar,
}

impl Owners {
    pub fn record(&self, local_port: u16, ipv6: bool, pid: u32) {
        self.map
            .lock()
            .unwrap()
            .insert((local_port, ipv6), Some(pid));
        self.changed.notify_all();
    }

    pub fn forget(&self, local_port: u16, ipv6: bool) {
        self.map.lock().unwrap().remove(&(local_port, ipv6));
    }

    pub fn wait(&self, local_port: u16, ipv6: bool, timeout: Duration) -> Option<u32> {
        let key = (local_port, ipv6);
        let guard = self.map.lock().unwrap();
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |m| !m.contains_key(&key))
            .unwrap();
        guard.get(&key).copied().flatten()
    }

    /// Known entry, else `lookup` (the tables), else wait for an event; a miss is remembered.
    pub fn pid(
        &self,
        local_port: u16,
        ipv6: bool,
        timeout: Duration,
        lookup: impl FnOnce(u16) -> Option<u32>,
    ) -> Option<u32> {
        let key = (local_port, ipv6);
        if let Some(known) = self.map.lock().unwrap().get(&key) {
            return *known;
        }
        if let Some(pid) = lookup(local_port) {
            self.record(local_port, ipv6, pid);
            return Some(pid);
        }
        let found = self.wait(local_port, ipv6, timeout);
        if found.is_none() {
            self.map.lock().unwrap().entry(key).or_insert(None);
        }
        found
    }
}

/// Port of a `MIB_TCPROW_OWNER_PID` (low 16 bits, network byte order).
pub fn port_of(dw_port: u32) -> u16 {
    u16::from_be(dw_port as u16)
}

/// Owner of `port` for a table row: TIME_WAIT and similar rows carry PID 0 and never count.
pub fn row_owner(dw_port: u32, pid: u32, port: u16) -> Option<u32> {
    (pid != 0 && port_of(dw_port) == port).then_some(pid)
}

/// PID -> (start time, image path); the start time tells a reused PID from the old process.
/// A failed image lookup is cached too (`None`), so System or unopenable processes cost one query.
#[derive(Default)]
pub struct Images(Mutex<HashMap<u32, Cached>>);

type Cached = (u64, Option<Arc<str>>);

impl Images {
    pub fn lookup(
        &self,
        pid: u32,
        start: u64,
        image: impl FnOnce() -> Option<String>,
    ) -> Option<Arc<str>> {
        if let Some((s, path)) = self.0.lock().unwrap().get(&pid)
            && *s == start
        {
            return path.clone();
        }
        let path: Option<Arc<str>> = image().map(Into::into); // outside the lock
        self.0.lock().unwrap().insert(pid, (start, path.clone()));
        path
    }

    pub fn get(&self, pid: u32) -> Option<Arc<str>> {
        let start = process_start(pid)?;
        self.lookup(pid, start, || image_path(pid))
    }

    /// Drops entries whose process is gone or whose PID now has another start time.
    pub fn prune(&self, alive: impl Fn(u32) -> Option<u64>) {
        let seen: Vec<(u32, u64)> = self
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(pid, (start, _))| (*pid, *start))
            .collect();
        // The liveness checks run outside the lock: the capture thread reads the cache.
        let dead: Vec<(u32, u64)> = seen
            .into_iter()
            .filter(|(pid, start)| alive(*pid) != Some(*start))
            .collect();
        self.0
            .lock()
            .unwrap()
            .retain(|pid, (start, _)| !dead.contains(&(*pid, *start)));
    }

    pub fn paths(&self) -> Vec<Arc<str>> {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter_map(|(_, p)| p.clone())
            .collect()
    }
}

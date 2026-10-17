//! Which process owns a local port (socket events, then the Windows tables) and its image path.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
    MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID,
    MIB_UDPROW_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_CLASS, TCP_TABLE_OWNER_PID_ALL,
    TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

use crate::rules::Protocol;

pub const PID_WAIT: Duration = Duration::from_millis(50);

/// Local port -> PID, fed by socket events; `wait` covers an event that trails its packet.
/// `None` means "looked up, no owner found" and is not waited for again.
#[derive(Default)]
pub struct Owners {
    map: Mutex<HashMap<u16, Option<u32>>>,
    changed: Condvar,
}

impl Owners {
    pub fn record(&self, local_port: u16, pid: u32) {
        self.map.lock().unwrap().insert(local_port, Some(pid));
        self.changed.notify_all();
    }

    pub fn forget(&self, local_port: u16) {
        self.map.lock().unwrap().remove(&local_port);
    }

    pub fn wait(&self, local_port: u16, timeout: Duration) -> Option<u32> {
        let guard = self.map.lock().unwrap();
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |m| !m.contains_key(&local_port))
            .unwrap();
        guard.get(&local_port).copied().flatten()
    }

    /// Known entry, else `lookup` (the tables), else wait for an event; a miss is remembered.
    pub fn pid(
        &self,
        local_port: u16,
        timeout: Duration,
        lookup: impl FnOnce(u16) -> Option<u32>,
    ) -> Option<u32> {
        if let Some(known) = self.map.lock().unwrap().get(&local_port) {
            return *known;
        }
        if let Some(pid) = lookup(local_port) {
            self.record(local_port, pid);
            return Some(pid);
        }
        let found = self.wait(local_port, timeout);
        if found.is_none() {
            self.map.lock().unwrap().entry(local_port).or_insert(None);
        }
        found
    }
}

/// Port of a `MIB_TCPROW_OWNER_PID` (low 16 bits, network byte order).
pub fn port_of(dw_port: u32) -> u16 {
    u16::from_be(dw_port as u16)
}

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// Runs a Windows owner-PID table call (retrying while the table grows) and returns what `pick`
/// makes of its rows. `rows_at` is the offset of the row array in the table struct.
fn owner_rows<Row, T>(
    fetch: impl Fn(Option<*mut c_void>, &mut u32) -> u32,
    rows_at: usize,
    pick: impl Fn(&[Row]) -> Option<T>,
) -> Option<T> {
    let mut size = 0u32;
    // First call only reports the needed size.
    fetch(None, &mut size);
    // u64 words keep the buffer 8-aligned for the table struct; retry if the table grew.
    for _ in 0..3 {
        let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
        let rc = fetch(Some(buf.as_mut_ptr().cast()), &mut size);
        if rc == ERROR_INSUFFICIENT_BUFFER {
            continue; // `size` was updated
        }
        if rc != 0 {
            return None;
        }
        // SAFETY: on success the buffer starts with a `u32` row count followed (at `rows_at`)
        // by that many rows inside the `size` bytes the call filled; raw pointers avoid
        // reading the rows through the table struct's one-row array.
        let rows = unsafe {
            let base = buf.as_ptr().cast::<u8>();
            let count = base.cast::<u32>().read() as usize;
            std::slice::from_raw_parts(base.add(rows_at).cast::<Row>(), count)
        };
        // ponytail: port lookups take the first matching row; several rows can share a port,
        // fine for an owner guess
        return pick(rows);
    }
    None
}

/// Owner of `port` for a table row: TIME_WAIT and similar rows carry PID 0 and never count.
pub fn row_owner(dw_port: u32, pid: u32, port: u16) -> Option<u32> {
    (pid != 0 && port_of(dw_port) == port).then_some(pid)
}

fn tcp_pid(class: TCP_TABLE_CLASS, port: u16) -> Option<u32> {
    owner_rows(
        |table, size| {
            // SAFETY: `table` is None (size query) or a buffer of at least `*size` bytes.
            unsafe { GetExtendedTcpTable(table, size, false, AF_INET, class, 0) }
        },
        std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
        |rows: &[MIB_TCPROW_OWNER_PID]| {
            rows.iter()
                .find_map(|r| row_owner(r.dwLocalPort, r.dwOwningPid, port))
        },
    )
    .or_else(|| {
        // Dual-stack sockets (bound on [::]) show up only in the IPv6 table.
        owner_rows(
            |table, size| {
                // SAFETY: as above.
                unsafe { GetExtendedTcpTable(table, size, false, AF_INET6, class, 0) }
            },
            std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table),
            |rows: &[MIB_TCP6ROW_OWNER_PID]| {
                rows.iter()
                    .find_map(|r| row_owner(r.dwLocalPort, r.dwOwningPid, port))
            },
        )
    })
}

fn udp_pid(port: u16) -> Option<u32> {
    owner_rows(
        |table, size| {
            // SAFETY: `table` is None (size query) or a buffer of at least `*size` bytes.
            unsafe { GetExtendedUdpTable(table, size, false, AF_INET, UDP_TABLE_OWNER_PID, 0) }
        },
        std::mem::offset_of!(MIB_UDPTABLE_OWNER_PID, table),
        |rows: &[MIB_UDPROW_OWNER_PID]| {
            rows.iter()
                .find_map(|r| row_owner(r.dwLocalPort, r.dwOwningPid, port))
        },
    )
    .or_else(|| {
        owner_rows(
            |table, size| {
                // SAFETY: as above.
                unsafe { GetExtendedUdpTable(table, size, false, AF_INET6, UDP_TABLE_OWNER_PID, 0) }
            },
            std::mem::offset_of!(MIB_UDP6TABLE_OWNER_PID, table),
            |rows: &[MIB_UDP6ROW_OWNER_PID]| {
                rows.iter()
                    .find_map(|r| row_owner(r.dwLocalPort, r.dwOwningPid, port))
            },
        )
    })
}

/// PIDs owning UDP sockets now, IPv4 and IPv6 tables, sorted and without duplicates.
pub fn udp_pids() -> Vec<u32> {
    let v4 = owner_rows(
        |table, size| {
            // SAFETY: `table` is None (size query) or a buffer of at least `*size` bytes.
            unsafe { GetExtendedUdpTable(table, size, false, AF_INET, UDP_TABLE_OWNER_PID, 0) }
        },
        std::mem::offset_of!(MIB_UDPTABLE_OWNER_PID, table),
        |rows: &[MIB_UDPROW_OWNER_PID]| {
            Some(rows.iter().map(|r| r.dwOwningPid).collect::<Vec<u32>>())
        },
    );
    let v6 = owner_rows(
        |table, size| {
            // SAFETY: as above.
            unsafe { GetExtendedUdpTable(table, size, false, AF_INET6, UDP_TABLE_OWNER_PID, 0) }
        },
        std::mem::offset_of!(MIB_UDP6TABLE_OWNER_PID, table),
        |rows: &[MIB_UDP6ROW_OWNER_PID]| {
            Some(rows.iter().map(|r| r.dwOwningPid).collect::<Vec<u32>>())
        },
    );
    let mut pids: Vec<u32> = v4
        .into_iter()
        .chain(v6)
        .flatten()
        .filter(|&p| p != 0)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// PID of the process listening on the TCP `port`.
pub fn listener_pid(port: u16) -> Option<u32> {
    tcp_pid(TCP_TABLE_OWNER_PID_LISTENER, port)
}

/// PID owning the local `local_port` per the Windows socket tables, IPv4 then IPv6 (`Both` is TCP).
pub fn table_pid(protocol: Protocol, local_port: u16) -> Option<u32> {
    match protocol {
        Protocol::Udp => udp_pid(local_port),
        Protocol::Tcp | Protocol::Both => tcp_pid(TCP_TABLE_OWNER_PID_ALL, local_port),
    }
}

/// Opens `pid` for limited queries, runs `f`, closes the handle.
fn with_process<T>(pid: u32, f: impl FnOnce(HANDLE) -> Option<T>) -> Option<T> {
    // SAFETY: plain Win32 call; the returned handle is closed below on every path.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let out = f(handle);
    // SAFETY: `handle` was opened above and is not used afterwards.
    unsafe {
        let _ = CloseHandle(handle);
    }
    out
}

/// Creation time (FILETIME as u64) of a running process; `None` if it does not open or has exited.
pub fn process_start(pid: u32) -> Option<u64> {
    const STILL_ACTIVE: u32 = 259;
    with_process(pid, |h| {
        let mut code = 0u32;
        // SAFETY: `h` is a live handle; the out pointer is a local.
        unsafe { GetExitCodeProcess(h, &mut code) }.ok()?;
        if code != STILL_ACTIVE {
            return None;
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: `h` is a live handle; the out pointers are locals.
        unsafe { GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    })
}

/// Full Win32 path of the process image.
pub fn image_path(pid: u32) -> Option<String> {
    with_process(pid, |h| {
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` u16s; the call updates `len` to the written length.
        unsafe {
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
        }
        .ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    })
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

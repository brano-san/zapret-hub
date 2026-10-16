//! Which process owns a local UDP port (from socket events) and who listens on a TCP port.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};

pub const PID_WAIT: Duration = Duration::from_millis(50);

/// Local UDP port -> PID, fed by socket events; `wait` covers an event that trails its packet.
#[derive(Default)]
pub struct Owners {
    map: Mutex<HashMap<u16, u32>>,
    changed: Condvar,
}

impl Owners {
    pub fn record(&self, local_port: u16, pid: u32) {
        self.map.lock().unwrap().insert(local_port, pid);
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
        guard.get(&local_port).copied()
    }
}

/// Port of a `MIB_TCPROW_OWNER_PID` (low 16 bits, network byte order).
pub fn port_of(dw_port: u32) -> u16 {
    u16::from_be(dw_port as u16)
}

/// PID of the process listening on the IPv4 TCP `port`.
pub fn listener_pid(port: u16) -> Option<u32> {
    const AF_INET: u32 = 2;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    let mut size = 0u32;
    // First call only reports the needed size.
    // SAFETY: a null table with `size = 0` is the documented size query.
    unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    // u64 words keep the buffer 8-aligned for the table struct; retry if the table grew.
    for _ in 0..3 {
        let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
        // SAFETY: `buf` is at least `size` bytes, which is passed along.
        let rc = unsafe {
            GetExtendedTcpTable(
                Some(buf.as_mut_ptr().cast()),
                &mut size,
                false,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == ERROR_INSUFFICIENT_BUFFER {
            continue; // `size` was updated
        }
        if rc != 0 {
            return None;
        }
        // SAFETY: on success the buffer starts with MIB_TCPTABLE_OWNER_PID and holds
        // `dwNumEntries` rows inside the `size` bytes the call filled.
        let rows = unsafe {
            let table = &*buf.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
        };
        return rows
            .iter()
            .find(|r| port_of(r.dwLocalPort) == port)
            .map(|r| r.dwOwningPid);
    }
    None
}

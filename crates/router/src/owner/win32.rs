//! The owner-PID tables and process queries of Windows.

use std::ffi::c_void;

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

use super::row_owner;

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

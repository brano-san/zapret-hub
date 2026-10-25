//! The socket tables and process files of Linux `/proc`.

use std::collections::HashMap;

use crate::rules::Protocol;

/// The IPv4 and IPv6 tables of one protocol (`Both` is TCP, as on Windows).
fn net_files(protocol: Protocol) -> [&'static str; 2] {
    match protocol {
        Protocol::Udp => ["/proc/net/udp", "/proc/net/udp6"],
        Protocol::Tcp | Protocol::Both => ["/proc/net/tcp", "/proc/net/tcp6"],
    }
}

/// The tables a socket of the family may sit in: an IPv4 flow's in both (dual-stack sockets are
/// IPv6 ones), the IPv4 table first; an IPv6 flow's in the IPv6 table only.
pub fn family_files(protocol: Protocol, ipv6: bool) -> &'static [&'static str] {
    match (protocol, ipv6) {
        (Protocol::Udp, false) => &["/proc/net/udp", "/proc/net/udp6"],
        (Protocol::Udp, true) => &["/proc/net/udp6"],
        (_, false) => &["/proc/net/tcp", "/proc/net/tcp6"],
        (_, true) => &["/proc/net/tcp6"],
    }
}

/// TCP's LISTEN in the `st` column.
const LISTEN: &str = "0A";

/// Socket inodes of the rows of a `/proc/net/{tcp,udp}[6]` table whose local port is `port`
/// (`None`: any), only LISTEN rows when `listen_only`. Inode 0 (TIME_WAIT) is no socket.
pub fn table_inodes(text: &str, port: Option<u16>, listen_only: bool) -> Vec<u64> {
    table_rows(text, listen_only)
        .filter(|(local_port, _)| port.is_none_or(|port| port == *local_port))
        .map(|(_, inode)| inode)
        .collect()
}

/// (local port, socket inode) of a table's rows, as `table_inodes` takes them.
fn table_rows(text: &str, listen_only: bool) -> impl Iterator<Item = (u16, u64)> + '_ {
    text.lines().filter_map(move |line| {
        // `sl local_address rem_address st tx:rx tr:when retrnsmt uid timeout inode ...`
        let fields: Vec<&str> = line.split_whitespace().collect();
        let local_port = fields.get(1)?.rsplit_once(':')?.1;
        let local_port = u16::from_str_radix(local_port, 16).ok()?;
        if listen_only && *fields.get(3)? != LISTEN {
            return None;
        }
        let inode = fields.get(9)?.parse().ok().filter(|inode| *inode != 0)?;
        Some((local_port, inode))
    })
}

/// The inode of a `/proc/<pid>/fd/*` link that names a socket: `socket:[12345]`.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// `starttime`, field 22 of `/proc/<pid>/stat`; `None` for a zombie.
pub fn stat_start(stat: &str) -> Option<u64> {
    // The name (field 2) may hold anything, `) ` too: count from its last `)`.
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    if fields.next()? == "Z" {
        return None;
    }
    // Field 3 (the state) is taken: starttime is 19 further.
    fields.nth(18)?.parse().ok()
}

fn inodes(files: &[&str], port: Option<u16>, listen_only: bool) -> Vec<u64> {
    files
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .flat_map(|text| table_inodes(&text, port, listen_only))
        .collect()
}

/// Socket inode -> the lowest PID holding it. Other users' fds do not open without root: their
/// sockets have no owner here.
fn socket_pids(skip: Option<u32>) -> HashMap<u64, u32> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return map;
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect();
    pids.sort_unstable();
    for pid in pids.into_iter().filter(|p| Some(*p) != skip) {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Some(inode) = std::fs::read_link(fd.path())
                .ok()
                .and_then(|link| socket_inode(link.to_str()?))
            {
                map.entry(inode).or_insert(pid);
            }
        }
    }
    map
}

/// The first PID holding one of `inodes`.
// ponytail: walks every process's fds per lookup; cache the map if lookups get hot
fn owner_of(inodes: &[u64], skip: Option<u32>) -> Option<u32> {
    if inodes.is_empty() {
        return None;
    }
    let pids = socket_pids(skip);
    inodes.iter().find_map(|inode| pids.get(inode).copied())
}

/// PIDs owning UDP sockets now, IPv4 and IPv6 tables, sorted and without duplicates.
pub fn udp_pids() -> Vec<u32> {
    let pids = socket_pids(None);
    let mut out: Vec<u32> = inodes(&net_files(Protocol::Udp), None, false)
        .iter()
        .filter_map(|inode| pids.get(inode).copied())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// (protocol, local port) -> PID of every socket, IPv4 and IPv6, from one walk of the processes'
/// fds. Another process wins a port this one holds too (`table_pid_other`).
pub fn port_pids() -> HashMap<(Protocol, u16), u32> {
    let pids = socket_pids(None);
    let me = std::process::id();
    let mut out = HashMap::new();
    for protocol in [Protocol::Tcp, Protocol::Udp] {
        for file in net_files(protocol) {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            for (port, inode) in table_rows(&text, false) {
                if let Some(pid) = pids.get(&inode) {
                    out.entry((protocol, port))
                        .and_modify(|p| {
                            if *p == me {
                                *p = *pid;
                            }
                        })
                        .or_insert(*pid);
                }
            }
        }
    }
    out
}

/// PID of the process listening on the TCP `port`.
pub fn listener_pid(port: u16) -> Option<u32> {
    owner_of(&inodes(&net_files(Protocol::Tcp), Some(port), true), None)
}

/// PID owning the local `local_port` of a socket of the family (`ipv6`) per the socket tables
/// (`family_files`; `Both` is TCP).
pub fn table_pid(protocol: Protocol, local_port: u16, ipv6: bool) -> Option<u32> {
    owner_of(
        &inodes(family_files(protocol, ipv6), Some(local_port), false),
        None,
    )
}

/// `table_pid`, another process first: this one's relay sockets may hold the same port number
/// (a UDP answer socket is bound to the flow's destination port).
pub fn table_pid_other(protocol: Protocol, local_port: u16, ipv6: bool) -> Option<u32> {
    let inodes = inodes(family_files(protocol, ipv6), Some(local_port), false);
    owner_of(&inodes, Some(std::process::id())).or_else(|| owner_of(&inodes, None))
}

/// Start time (clock ticks since boot) of a running process; `None` if it is gone or a zombie.
pub fn process_start(pid: u32) -> Option<u64> {
    stat_start(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Full path of the process image.
pub fn image_path(pid: u32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0820 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41236 1 0000000000000000 100 0 0 10 0
   1: 0100007F:C350 0100007F:0820 01 00000000:00000000 00:00000000 00000000  1000        0 41240 1 0000000000000000 20 4 30 10 -1
   2: 0100007F:0821 0100007F:9C40 06 00000000:00000000 03:00000F5A 00000000     0        0 0 3 0000000000000000
";

    #[test]
    fn a_table_row_names_its_socket_by_local_port() {
        // 0x0820 = 2080, 0xC350 = 50000.
        assert_eq!(table_inodes(TCP, Some(2080), false), [41236]);
        assert_eq!(table_inodes(TCP, Some(50000), false), [41240]);
        assert_eq!(table_inodes(TCP, Some(50000), true), [] as [u64; 0]);
        assert_eq!(table_inodes(TCP, Some(2080), true), [41236]);
        assert_eq!(table_inodes(TCP, None, false), [41236, 41240]);
        // A TIME_WAIT row has no socket.
        assert_eq!(table_inodes(TCP, Some(2081), false), [] as [u64; 0]);
        assert_eq!(
            table_inodes("garbage\nmore garbage", None, false),
            [] as [u64; 0]
        );
    }

    #[test]
    fn an_ipv6_row_has_the_port_after_the_last_colon() {
        let tcp6 = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52001 1 0000000000000000 100 0 0 10 0
";
        assert_eq!(table_inodes(tcp6, Some(8080), true), [52001]);
    }

    #[test]
    fn only_socket_links_have_an_inode() {
        assert_eq!(socket_inode("socket:[41236]"), Some(41236));
        assert_eq!(socket_inode("pipe:[41236]"), None);
        assert_eq!(socket_inode("/dev/null"), None);
        assert_eq!(socket_inode("socket:[x]"), None);
    }

    #[test]
    fn the_start_time_survives_a_name_with_parentheses() {
        let stat =
            "4242 (a) b (c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";
        assert_eq!(stat_start(stat), Some(987654));
        let zombie = "4243 (gone) Z 1 4243 4243 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987655 0 0";
        assert_eq!(stat_start(zombie), None);
        assert_eq!(stat_start("4244 (short) S 1"), None);
    }
}

//! WinDivert: the capture filter, DLL choice and the address struct (pure part).

use crate::rules::Table;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub const MAX_FILTER_RANGES: usize = 64;
pub const SOCKET_FILTER: &str =
    "!loopback and udp and (event == BIND or event == CONNECT or event == CLOSE)";
pub const EVENT_SOCKET_CLOSE: u8 = 7;

fn port(&(a, b): &(u16, u16)) -> String {
    match (a, b) {
        _ if a == b => format!("udp.DstPort == {a}"),
        (_, 65535) => format!("udp.DstPort >= {a}"),
        _ => format!("(udp.DstPort >= {a} and udp.DstPort <= {b})"),
    }
}

fn range(&(lo, hi): &(u32, u32)) -> String {
    let ip = |x: u32| std::net::Ipv4Addr::from(x);
    if lo == hi {
        format!("ip.DstAddr == {}", ip(lo))
    } else {
        format!("(ip.DstAddr >= {} and ip.DstAddr <= {})", ip(lo), ip(hi))
    }
}

fn any(items: impl Iterator<Item = String>) -> String {
    format!("({})", items.collect::<Vec<_>>().join(" or "))
}

pub fn filter(table: &Table) -> Option<String> {
    if table.is_empty() {
        return None;
    }
    let total: usize = table.rules.iter().map(|r| r.ranges.len()).sum();
    let by_ip = total <= MAX_FILTER_RANGES;
    let parts: Vec<String> = table
        .rules
        .iter()
        .map(|r| {
            let mut s = String::from("udp");
            if !r.ports.is_empty() {
                s += &format!(" and {}", any(r.ports.iter().map(port)));
            }
            // ponytail: with no ports and too many ranges this is just `(udp)`: all outbound UDP goes
            // through the hub, which matches IPs itself; upgrade: require ports in such a rule or raise the cap.
            if by_ip && !r.ranges.is_empty() {
                s += &format!(" and {}", any(r.ranges.iter().map(range)));
            }
            format!("({s})")
        })
        .collect();
    Some(format!(
        "outbound and !loopback and ip and ({})",
        parts.join(" or ")
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Flowseal,
    Zapret2,
}

pub fn pick_dll(
    running: Option<Engine>,
    flowseal_root: Option<&Path>,
    winws2_dir: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf> {
    let flowseal = flowseal_root.map(|r| r.join("bin").join("WinDivert.dll"));
    let zapret2 = winws2_dir.map(|d| d.join("WinDivert.dll"));
    let running_dll = |dll: Option<PathBuf>| match dll {
        Some(p) if exists(&p) => Ok(p),
        Some(p) => bail!(
            "{} is missing: the running engine's driver needs its own WinDivert.dll",
            p.display()
        ),
        None => bail!("the running engine's folder is unknown: its WinDivert.dll cannot be found"),
    };
    match running {
        Some(Engine::Flowseal) => running_dll(flowseal),
        Some(Engine::Zapret2) => running_dll(zapret2),
        None => {
            let candidates: Vec<PathBuf> = flowseal.into_iter().chain(zapret2).collect();
            match candidates.iter().find(|p| exists(p)) {
                Some(p) => Ok(p.clone()),
                None => bail!(
                    "WinDivert.dll was not found in the zapret or the zapret2 folder (checked: {})",
                    candidates
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    }
}

/// `WINDIVERT_ADDRESS` of WinDivert 2.2.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct Address(pub [u8; 80]);

impl Address {
    pub fn outbound(&self) -> bool {
        self.0[10] & 0x02 != 0
    }

    /// Clears Outbound and Impostor; the rest stays.
    pub fn as_inbound(&self) -> Address {
        let mut a = *self;
        a.0[10] &= !0x0A;
        a
    }

    pub fn event(&self) -> u8 {
        self.0[9]
    }

    pub fn socket_pid(&self) -> u32 {
        u32::from_le_bytes(self.0[32..36].try_into().unwrap())
    }

    pub fn socket_local_port(&self) -> u16 {
        u16::from_le_bytes(self.0[68..70].try_into().unwrap())
    }

    pub fn socket_protocol(&self) -> u8 {
        self.0[72]
    }
}

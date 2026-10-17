//! WinDivert: the capture filter, DLL choice and the address struct (pure part).

use crate::rules::{Protocol, Table};
use anyhow::{Result, bail};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

pub const MAX_FILTER_RANGES: usize = 64;
pub const SOCKET_FILTER: &str =
    "!loopback and (udp or tcp) and (event == BIND or event == CONNECT or event == CLOSE)";
pub const DNS_FILTER: &str = "ip and udp and udp.SrcPort == 53";
pub const EVENT_SOCKET_CLOSE: u8 = 7;
pub const PROTO_TCP: u8 = 6;

fn port(proto: &str, &(a, b): &(u16, u16)) -> String {
    match (a, b) {
        _ if a == b => format!("{proto}.DstPort == {a}"),
        (_, 65535) => format!("{proto}.DstPort >= {a}"),
        _ => format!("({proto}.DstPort >= {a} and {proto}.DstPort <= {b})"),
    }
}

fn range(&(lo, hi): &(u32, u32)) -> String {
    let ip = |x: u32| Ipv4Addr::from(x);
    if lo == hi {
        format!("ip.DstAddr == {}", ip(lo))
    } else {
        format!("(ip.DstAddr >= {} and ip.DstAddr <= {})", ip(lo), ip(hi))
    }
}

fn any(items: impl Iterator<Item = String>) -> String {
    format!("({})", items.collect::<Vec<_>>().join(" or "))
}

/// `head` alone, or `head and (<ports in proto's field>)`.
fn on_ports(head: &str, proto: &str, ports: &[(u16, u16)]) -> String {
    if ports.is_empty() {
        head.to_string()
    } else {
        format!("{head} and {}", any(ports.iter().map(|p| port(proto, p))))
    }
}

/// WinDivert compiles a filter into at most 256 tests (`WINDIVERT_FILTER_MAXLEN`); this keeps
/// a margin, since `capture_filter` counts the tests itself.
pub const FILTER_BUDGET: usize = 200;
/// `outbound`, `!loopback`, `ip`.
const BASE_TESTS: usize = 3;
/// `tcp and tcp.SrcPort == N`.
const ACCEPTOR_TESTS: usize = 2;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Udp,
    Tcp,
    Both,
    /// The first SYN of a connection (domain TCP rules).
    Syn,
    /// Destinations of live NAT entries.
    Held,
}

/// One `or` alternative: protocol and ports, ANDed with its IP ranges when it has any.
struct Part {
    kind: Kind,
    /// Sorted.
    ports: Vec<(u16, u16)>,
    /// Sorted, merged.
    ranges: Vec<(u32, u32)>,
}

impl Part {
    fn tcp(&self) -> bool {
        self.kind != Kind::Udp
    }

    fn tests(&self) -> usize {
        let ports: usize = self
            .ports
            .iter()
            .map(|&(a, b)| if a == b || b == 65535 { 1 } else { 2 })
            .sum();
        let ranges: usize = self
            .ranges
            .iter()
            .map(|&(a, b)| if a == b { 1 } else { 2 })
            .sum();
        ranges
            + match self.kind {
                Kind::Udp | Kind::Tcp | Kind::Held => 1 + ports,
                Kind::Both => 2 + 2 * ports,
                Kind::Syn => 3 + ports,
            }
    }

    fn text(&self) -> String {
        let head = match self.kind {
            Kind::Udp => on_ports("udp", "udp", &self.ports),
            Kind::Tcp => on_ports("tcp", "tcp", &self.ports),
            Kind::Both if self.ports.is_empty() => "(udp or tcp)".into(),
            Kind::Both => format!(
                "(({}) or ({}))",
                on_ports("udp", "udp", &self.ports),
                on_ports("tcp", "tcp", &self.ports)
            ),
            Kind::Syn => on_ports("tcp and tcp.Syn and !tcp.Ack", "tcp", &self.ports),
            Kind::Held => "tcp".into(),
        };
        if self.ranges.is_empty() {
            format!("({head})")
        } else {
            format!("({head} and {})", any(self.ranges.iter().map(range)))
        }
    }
}

/// Inside one list, merges the two neighbouring items with the smallest gap (ties: the first).
/// False when no list has two items.
fn merge_closest<T: Copy + Ord + Into<u64>>(lists: Vec<&mut Vec<(T, T)>>) -> bool {
    let closest = lists
        .iter()
        .enumerate()
        .flat_map(|(l, list)| {
            list.windows(2)
                .enumerate()
                .map(move |(i, w)| (w[1].0.into().saturating_sub(w[0].1.into()), l, i))
        })
        .min_by_key(|&(gap, _, _)| gap);
    let Some((_, l, i)) = closest else {
        return false;
    };
    let list = lists.into_iter().nth(l).unwrap();
    list[i].1 = list[i].1.max(list[i + 1].1);
    list.remove(i + 1);
    true
}

/// Merges the closest IP ranges, then the closest ports, until at most `MAX_FILTER_RANGES`
/// ranges are left and the filter has at most `FILTER_BUDGET` tests, or nothing is left to merge.
// ponytail: O(n²) in the number of ranges and ports (hundreds at most); a heap of gaps if that
// ever grows. The filter becomes a superset; `Table` stays exact and the hub re-sends what it does
// not match.
fn fit(parts: &mut [Part], extra: usize) {
    loop {
        let ranges: usize = parts.iter().map(|p| p.ranges.len()).sum();
        let tests: usize = extra + parts.iter().map(Part::tests).sum::<usize>();
        if ranges <= MAX_FILTER_RANGES && tests <= FILTER_BUDGET {
            return;
        }
        let merged = merge_closest(parts.iter_mut().map(|p| &mut p.ranges).collect())
            || (ranges <= MAX_FILTER_RANGES
                && merge_closest(parts.iter_mut().map(|p| &mut p.ports).collect()));
        if !merged {
            return;
        }
    }
}

/// The router's reason for a capture filter WinDivert refused. The filter is generated, so an
/// invalid one (`ERROR_INVALID_PARAMETER`) means it did not fit WinDivert's limit.
pub fn refused(e: &std::io::Error) -> String {
    if e.raw_os_error() == Some(87) {
        format!("too many rules for the capture filter: disable or narrow some (WinDivert: {e})")
    } else {
        format!("WinDivert refused the filter: {e}")
    }
}

/// Only error 87 (the filter did not fit) is final for that filter text; memory or driver
/// errors may pass on the next rebuild.
pub fn refused_for_good(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(87)
}

pub fn filter(table: &Table) -> Option<String> {
    capture_filter(table, None, &[])
}

/// The capture handle's filter: each rule's protocol and ports over its ranges (subnets and
/// resolved IPs; none yet, no part), the first SYNs of domain and program TCP rules (so the hub
/// sees a SYN to an address learned since the last reopen, or decides a program's connection), the
/// UDP of program rules while their program runs (`Compiled.running`), destinations of live NAT entries (`held`) and the
/// local acceptor's replies.
pub fn capture_filter(table: &Table, acceptor: Option<u16>, held: &[Ipv4Addr]) -> Option<String> {
    let mut parts: Vec<Part> = Vec::new();
    for r in &table.rules {
        let mut ports = r.ports.clone();
        ports.sort_unstable();
        if !r.ranges.is_empty() {
            let kind = match r.protocol {
                Protocol::Udp => Kind::Udp,
                Protocol::Tcp => Kind::Tcp,
                Protocol::Both => Kind::Both,
            };
            parts.push(Part {
                kind,
                ports: ports.clone(),
                ranges: r.ranges.clone(),
            });
        }
        let syn = r.protocol != Protocol::Udp && !(r.domains.is_empty() && r.apps.is_empty());
        // `running` is set only for rules that take UDP and have programs.
        let udp = r.protocol != Protocol::Tcp && !r.apps.is_empty() && r.running;
        for (wanted, kind) in [(syn, Kind::Syn), (udp, Kind::Udp)] {
            let same = |p: &Part| p.kind == kind && p.ports == ports && p.ranges.is_empty();
            if wanted && !parts.iter().any(same) {
                parts.push(Part {
                    kind,
                    ports: ports.clone(),
                    ranges: vec![],
                });
            }
        }
    }
    if !held.is_empty() {
        parts.push(Part {
            kind: Kind::Held,
            ports: vec![],
            ranges: crate::rules::merge(held.iter().map(|&ip| (ip.into(), ip.into())).collect()),
        });
    }
    if parts.is_empty() {
        return None;
    }
    let acceptor = acceptor.filter(|_| parts.iter().any(Part::tcp));
    let extra = BASE_TESTS + acceptor.map_or(0, |_| ACCEPTOR_TESTS);
    fit(&mut parts, extra);
    let mut text: Vec<String> = parts.iter().map(Part::text).collect();
    if let Some(port) = acceptor {
        text.push(format!("(tcp and tcp.SrcPort == {port})"));
    }
    Some(format!(
        "outbound and !loopback and ip and ({})",
        text.join(" or ")
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

// ---- the DLL and handles (the live part) ----

use std::ffi::{c_char, c_void};
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{HSTRING, PCSTR};

type OpenFn = unsafe extern "C" fn(*const c_char, i32, i16, u64) -> *mut c_void;
type RecvFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut u32, *mut Address) -> i32;
type SendFn =
    unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut u32, *const Address) -> i32;
type ShutdownFn = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type CloseFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ChecksumsFn = unsafe extern "C" fn(*mut c_void, u32, *mut Address, u64) -> i32;
type CompileFn =
    unsafe extern "C" fn(*const c_char, i32, *mut c_char, u32, *mut *const c_char, *mut u32) -> i32;

#[derive(Clone, Debug)]
pub struct Dll {
    open: OpenFn,
    recv: RecvFn,
    send: SendFn,
    shutdown: ShutdownFn,
    close: CloseFn,
    checksums: ChecksumsFn,
    compile: CompileFn,
}

pub const PRIORITY: i16 = 1000;
pub const FLAG_SNIFF: u64 = 0x1;
pub const FLAG_RECV_ONLY: u64 = 0x4;
/// Receive only: queued packets still drain and the handle can still send until it is closed.
const SHUTDOWN_RECV: i32 = 1;
/// The largest packet WinDivert delivers; size receive buffers with it.
pub const MAX_PACKET: usize = 0xFFFF;

#[derive(Clone, Copy)]
pub enum Layer {
    Network = 0,
    Socket = 3,
}

static LOADED: OnceLock<Mutex<std::collections::HashMap<PathBuf, Dll>>> = OnceLock::new();

impl Dll {
    /// Loads once per path; the library is never freed (it lives until the process ends).
    pub fn load(path: &Path) -> Result<Dll> {
        let mut loaded = LOADED.get_or_init(Default::default).lock().unwrap();
        if let Some(dll) = loaded.get(path) {
            return Ok(dll.clone());
        }
        // SAFETY: a plain library load of a full path; WinDivert.dll has no unsafe load-time contract.
        let module = unsafe { LoadLibraryW(&HSTRING::from(path.as_os_str())) }
            .map_err(|e| anyhow::anyhow!("cannot load {}: {e}", path.display()))?;
        let sym = |name: &'static str| -> Result<*const c_void> {
            let c = format!("{name}\0");
            // SAFETY: `module` is a live module handle and `c` is NUL-terminated.
            match unsafe { GetProcAddress(module, PCSTR(c.as_ptr())) } {
                Some(f) => Ok(f as *const c_void),
                None => bail!("{} has no {name}: is it WinDivert 2.x?", path.display()),
            }
        };
        // SAFETY: the signatures are those of WinDivert 2.2's exported functions.
        let dll = unsafe {
            Dll {
                open: std::mem::transmute::<*const c_void, OpenFn>(sym("WinDivertOpen")?),
                recv: std::mem::transmute::<*const c_void, RecvFn>(sym("WinDivertRecv")?),
                send: std::mem::transmute::<*const c_void, SendFn>(sym("WinDivertSend")?),
                shutdown: std::mem::transmute::<*const c_void, ShutdownFn>(sym(
                    "WinDivertShutdown",
                )?),
                close: std::mem::transmute::<*const c_void, CloseFn>(sym("WinDivertClose")?),
                checksums: std::mem::transmute::<*const c_void, ChecksumsFn>(sym(
                    "WinDivertHelperCalcChecksums",
                )?),
                compile: std::mem::transmute::<*const c_void, CompileFn>(sym(
                    "WinDivertHelperCompileFilter",
                )?),
            }
        };
        loaded.insert(path.to_path_buf(), dll.clone());
        Ok(dll)
    }

    /// Compiles `filter` without opening a handle (no elevation needed): WinDivert's error and
    /// position on failure.
    pub fn compile(&self, filter: &str, layer: Layer) -> std::result::Result<(), String> {
        let f = std::ffi::CString::new(filter).map_err(|e| e.to_string())?;
        let mut error: *const c_char = std::ptr::null();
        let mut pos = 0u32;
        // SAFETY: `f` is NUL-terminated; a null object buffer only checks; `error`/`pos` are valid
        // out-pointers and `error` points to a static string when set.
        let ok = unsafe {
            (self.compile)(
                f.as_ptr(),
                layer as i32,
                std::ptr::null_mut(),
                0,
                &mut error,
                &mut pos,
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let error = match error.is_null() {
            true => "unknown error".into(),
            // SAFETY: WinDivert sets `error` to a NUL-terminated static string.
            false => unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy(),
        };
        Err(format!("{error} at {pos}"))
    }

    pub fn checksums(&self, packet: &mut [u8], addr: &mut Address) {
        // SAFETY: the pointer and length describe `packet`; `addr` is a valid WINDIVERT_ADDRESS.
        unsafe {
            (self.checksums)(packet.as_mut_ptr().cast(), packet.len() as u32, addr, 0);
        }
    }
}

pub struct Handle {
    dll: Dll,
    raw: *mut c_void,
}

// SAFETY: WinDivert handles may be used from several threads (one recv, one send, shutdown from another).
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    pub fn open(dll: &Dll, filter: &str, layer: Layer, flags: u64) -> std::io::Result<Handle> {
        let f = std::ffi::CString::new(filter)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        // SAFETY: `f` is NUL-terminated and outlives the call.
        let raw = unsafe { (dll.open)(f.as_ptr(), layer as i32, PRIORITY, flags) };
        if raw as isize == INVALID_HANDLE_VALUE.0 as isize {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Handle {
            dll: dll.clone(),
            raw,
        })
    }

    /// For the SOCKET layer pass an empty buffer.
    pub fn recv(&self, buf: &mut [u8]) -> std::io::Result<(usize, Address)> {
        let mut addr = Address([0; 80]);
        let mut len = 0u32;
        let data = if buf.is_empty() {
            std::ptr::null_mut()
        } else {
            buf.as_mut_ptr().cast()
        };
        // SAFETY: `data`/`buf.len()` describe the buffer (or null with 0); `len` and `addr` are valid out-pointers.
        let ok = unsafe { (self.dll.recv)(self.raw, data, buf.len() as u32, &mut len, &mut addr) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok((len as usize, addr))
    }

    pub fn send(&self, packet: &[u8], addr: &Address) -> std::io::Result<()> {
        // SAFETY: the pointer and length describe `packet`; `addr` is a valid WINDIVERT_ADDRESS.
        let ok = unsafe {
            (self.dll.send)(
                self.raw,
                packet.as_ptr().cast(),
                packet.len() as u32,
                std::ptr::null_mut(),
                addr,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Stops receiving: `recv` returns the queued packets, then an error; `send` keeps working.
    pub fn shutdown(&self) {
        // SAFETY: `raw` is a live handle until Drop.
        unsafe { (self.dll.shutdown)(self.raw, SHUTDOWN_RECV) };
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `raw` is closed exactly once, here.
        unsafe { (self.dll.close)(self.raw) };
    }
}

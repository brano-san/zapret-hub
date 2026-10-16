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

#[derive(Clone, Debug)]
pub struct Dll {
    open: OpenFn,
    recv: RecvFn,
    send: SendFn,
    shutdown: ShutdownFn,
    close: CloseFn,
    checksums: ChecksumsFn,
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
            }
        };
        loaded.insert(path.to_path_buf(), dll.clone());
        Ok(dll)
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

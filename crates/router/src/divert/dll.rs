//! WinDivert.dll loaded at run time, and its handles.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::Address;

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

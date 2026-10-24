//! Folders and links open the way the user's own shell opens them.
//!
//! Windows: its default verb, so a file manager registered for folders (FilePilot and the like)
//! opens them, not always Explorer. The app runs elevated, and a program started from it would be
//! elevated too: the desktop's Explorer runs the verb instead, unelevated, through
//! `IShellDispatch2::ShellExecute` (the desktop's shell view from `IShellWindows`). Without a
//! desktop shell (another shell, Explorer restarting) it falls back to handing the target to
//! `explorer.exe`.
//!
//! Linux: `xdg-open`, the desktop's default handler.

#[cfg(not(windows))]
use std::ffi::OsStr;

#[cfg(windows)]
pub use win::open;

/// `xdg-open` hands the target over and may stay until the handler exits: it is not waited for.
#[cfg(not(windows))]
pub fn open(target: &OsStr) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(target);
    let child = cmd
        .spawn()
        .map_err(|err| anyhow::anyhow!("cannot run xdg-open: {err}"))?;
    reap(child);
    log::info!("handed {} to xdg-open", target.display());
    Ok(())
}

/// A helper program started and left to run: `notify-send` and the like. Its exit is collected
/// on a thread of its own, so it leaves no zombie.
#[cfg(not(windows))]
pub fn launch(mut cmd: std::process::Command) {
    match cmd.spawn() {
        Ok(child) => reap(child),
        Err(err) => log::warn!("cannot run {}: {err}", cmd.get_program().display()),
    }
}

#[cfg(not(windows))]
fn reap(mut child: std::process::Child) {
    std::thread::spawn(move || child.wait());
}

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt as _;
    use std::process::Command;
    use std::time::Duration;

    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IServiceProvider,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Shell::{
        CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows,
        SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
    };
    use windows::core::{BSTR, Interface as _};

    /// `SW_SHOWNORMAL`.
    const SHOW_NORMAL: i32 = 1;

    pub fn open(target: &OsStr) -> anyhow::Result<()> {
        match desktop_open(target) {
            Ok(()) => {
                log::info!("opened {} through the desktop shell", target.display());
                Ok(())
            }
            Err(err) => {
                log::warn!(
                    "the desktop shell did not open {} ({err}), handing it to explorer.exe",
                    target.display()
                );
                let mut cmd = Command::new("explorer.exe");
                cmd.arg(target);
                // explorer exits with 1 even when it opened the target: the code means nothing.
                exec::run(&mut cmd, Duration::from_secs(10), None)?;
                Ok(())
            }
        }
    }

    fn int(value: i32) -> VARIANT {
        let mut variant = VARIANT::default();
        // SAFETY: a VT_I4 variant holds its value in `lVal`.
        unsafe {
            (*variant.Anonymous.Anonymous).vt = VT_I4;
            (*variant.Anonymous.Anonymous).Anonymous.lVal = value;
        }
        variant
    }

    fn desktop_open(target: &OsStr) -> windows::core::Result<()> {
        // SAFETY: COM is initialized for this thread around the calls and released after them; every
        // interface is dropped inside `execute`, before `CoUninitialize`.
        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = execute(target);
            // A thread already in another apartment (`RPC_E_CHANGED_MODE`) keeps it.
            if init.is_ok() {
                CoUninitialize();
            }
            result
        }
    }

    /// Raymond Chen's «How can I launch an unelevated process from my elevated process».
    unsafe fn execute(target: &OsStr) -> windows::core::Result<()> {
        unsafe {
            let windows: IShellWindows =
                CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
            let mut hwnd = 0;
            let desktop = windows.FindWindowSW(
                &int(CSIDL_DESKTOP as i32),
                &VARIANT::default(),
                SWC_DESKTOP,
                &mut hwnd,
                SWFO_NEEDDISPATCH,
            )?;
            let browser: IShellBrowser = desktop
                .cast::<IServiceProvider>()?
                .QueryService(&SID_STopLevelBrowser)?;
            let view: IShellFolderViewDual = browser
                .QueryActiveShellView()?
                .GetItemObject(SVGIO_BACKGROUND)?;
            let shell: IShellDispatch2 = view.Application()?.cast()?;
            let file = BSTR::from_wide(&target.encode_wide().collect::<Vec<u16>>());
            // Empty arguments, folder and verb: the default verb, as a double click.
            let empty = VARIANT::default();
            shell.ShellExecute(&file, &empty, &empty, &empty, &int(SHOW_NORMAL))
        }
    }
}

//! The named pipe between `zapret-hub-cli` and the app (CLI spec 4, 5): local only, one request
//! line and one answer line per connection, a thread per client.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PWSTR};

use crate::MAX_REQUEST;

/// One per machine, like the zapret service (spec 12, decision 12).
pub const NAME: &str = r"\\.\pipe\zapret-hub-cli";

/// SYSTEM and Administrators may do anything, `user` may read and write, nobody else may open it;
/// Medium integrity with no-write-up and no-read-up keeps sandboxed (Low) programs out (spec 5).
pub fn sddl(user: &str) -> String {
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{user})S:(ML;;NWNR;;;ME)")
}

/// `S-1-5-21-…` of the user this process runs as: from its token, so an elevated app still names
/// the user who started it.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: plain Win32 calls with buffers of the sizes they report; the token is closed and the
    // string freed before this returns.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let owned = OwnedHandle::from_raw_handle(token.0);
        let mut size = 0;
        // Fails with ERROR_INSUFFICIENT_BUFFER, having reported the size.
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        // u64 cells: TOKEN_USER starts with a pointer and must be aligned for it.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )?;
        drop(owned);
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let sid = text.to_string();
        LocalFree(Some(HLOCAL(text.0.cast())));
        sid.map_err(io::Error::other)
    }
}

/// A new instance of the pipe. With `first` it fails with ERROR_ACCESS_DENIED when the name exists
/// already, whoever made it (spec 5).
fn instance(name: &HSTRING, sddl: &HSTRING, first: bool) -> io::Result<OwnedHandle> {
    // SAFETY: the descriptor lives until CreateNamedPipeW, which copies it, has returned.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            ..Default::default()
        };
        let mut open = PIPE_ACCESS_DUPLEX;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let pipe = CreateNamedPipeW(
            name,
            open,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            Some(&attributes),
        );
        let created = if pipe.is_invalid() {
            Err(io::Error::last_os_error())
        } else {
            Ok(OwnedHandle::from_raw_handle(pipe.0))
        };
        LocalFree(Some(HLOCAL(descriptor.0)));
        created
    }
}

/// Waits for a client to open this instance.
fn connect(pipe: &OwnedHandle) -> io::Result<()> {
    // SAFETY: a valid pipe handle, used synchronously (no OVERLAPPED).
    match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
        // A client that opened the instance before the call is connected already.
        Err(err) if err.code() != ERROR_PIPE_CONNECTED.to_hresult() => Err(err.into()),
        _ => Ok(()),
    }
}

/// The app's end. The first instance is made by `bind`, so a taken name is known before serving.
pub struct Server {
    name: HSTRING,
    sddl: HSTRING,
    next: OwnedHandle,
}

impl Server {
    /// Fails with ERROR_ACCESS_DENIED (os error 5) when another program holds `name`.
    pub fn bind(name: &str) -> io::Result<Self> {
        let name = HSTRING::from(name);
        let sddl = HSTRING::from(sddl(&current_user_sid()?));
        let next = instance(&name, &sddl, true)?;
        Ok(Self { name, sddl, next })
    }

    /// Answers clients until making an instance fails, and returns that error. `handler` gets the
    /// request line without its `\n` and returns the answer line. Each client has its own thread:
    /// one that never sends its line holds only that thread (spec 11).
    pub fn serve(mut self, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) -> io::Error {
        let handler = Arc::new(handler);
        loop {
            let connected = connect(&self.next);
            // The next instance is up before this one is handed over: the name never goes free.
            let client = match instance(&self.name, &self.sddl, false) {
                Ok(fresh) => std::mem::replace(&mut self.next, fresh),
                Err(err) => return err,
            };
            // A failed connect (usually ERROR_NO_DATA: the client left first): nobody to answer;
            // the next instance is already up.
            if connected.is_err() {
                continue;
            }
            let handler = Arc::clone(&handler);
            // No thread to be had: the closure is dropped and closes this client's handle; the
            // next client may get one.
            let _ = std::thread::Builder::new().spawn(move || {
                // An error here means the client went away: nobody to tell.
                let _ = answer(File::from(client), &*handler);
            });
        }
    }
}

/// Reads one line, keeping at most `MAX_REQUEST + 1` bytes of it, writes the handler's answer and
/// waits until the client has read it. A line the client never finished reaches no handler.
fn answer(pipe: File, handler: &impl Fn(&[u8]) -> String) -> io::Result<()> {
    let mut reader = BufReader::new(&pipe);
    let mut line = Vec::new();
    (&mut reader)
        .take(MAX_REQUEST as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() > MAX_REQUEST {
        // The client writes the rest before it reads: drain it, or both ends wait forever.
        reader.skip_until(b'\n')?;
    } else {
        // The client closed before its `\n`: nobody would read the answer, so nothing runs.
        return Ok(());
    }
    let mut response = handler(&line);
    response.push('\n');
    (&pipe).write_all(response.as_bytes())?;
    // FlushFileBuffers: closing the handle before the client has read the answer would drop it.
    pipe.sync_all()
}

/// The CLI's end: sends `line` and returns the answer line. While every instance is taken the
/// pipe is busy; that is retried for up to `busy`. An answer without its `\n` is UnexpectedEof.
pub fn request(name: &str, line: &[u8], busy: Duration) -> io::Result<Vec<u8>> {
    let since = Instant::now();
    let pipe = loop {
        // Identification only: whoever holds the name cannot act as this user.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION.0)
            .open(name)
        {
            Err(err)
                if err.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                    && since.elapsed() < busy =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            opened => break opened?,
        }
    };
    let mut message = line.to_vec();
    message.push(b'\n');
    (&pipe).write_all(&message)?;
    let mut answer = Vec::new();
    BufReader::new(&pipe).read_until(b'\n', &mut answer)?;
    if answer.pop() != Some(b'\n') {
        // The handler failed, or whatever holds the name is not the app.
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the app closed the connection without an answer",
        ));
    }
    Ok(answer)
}

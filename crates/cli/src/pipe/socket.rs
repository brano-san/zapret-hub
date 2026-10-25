//! The Unix socket of Linux: one per user, in the user's runtime folder.

use std::ffi::OsStr;
use std::fs::{File, Permissions};
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// `$XDG_RUNTIME_DIR/zapret-hub.sock` (a folder only its user may enter), else
/// `/tmp/zapret-hub-<uid>/zapret-hub.sock`.
pub fn socket_path(runtime_dir: Option<&OsStr>, uid: u32) -> PathBuf {
    match runtime_dir.map(Path::new) {
        Some(dir) if dir.is_absolute() => dir.join("zapret-hub.sock"),
        _ => PathBuf::from(format!("/tmp/zapret-hub-{uid}/zapret-hub.sock")),
    }
}

/// `dir` when it is a folder of `uid` nobody else may enter, as the XDG spec requires. WSLg's
/// `/run/user/<uid>` is `0755`: the app could not serve there, so both ends fall back to `/tmp`.
pub fn private_runtime_dir(dir: Option<&OsStr>, uid: u32) -> Option<&OsStr> {
    dir.filter(|dir| std::fs::symlink_metadata(dir).is_ok_and(|meta| private(&meta, uid)))
}

fn private(meta: &std::fs::Metadata, uid: u32) -> bool {
    meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0
}

/// The socket the app serves and the CLI opens.
pub fn name() -> String {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    socket_path(private_runtime_dir(runtime_dir.as_deref(), uid), uid)
        .to_string_lossy()
        .into_owned()
}

/// The app's end: an exclusive `flock` on `<name>.lock` keeps a second server out, mode 0600 and
/// the peer's uid keep other users out (spec 5).
pub struct Server {
    _lock: File,
    listener: UnixListener,
}

impl Server {
    /// Fails with `PermissionDenied` when another server holds `name`.
    pub fn bind(name: &str) -> io::Result<Self> {
        private_dir(Path::new(name), true)?;
        let lock = File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(format!("{name}.lock"))?;
        // SAFETY: a valid fd, open as long as the `Server` lives.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let err = io::Error::last_os_error();
            return Err(if err.kind() == io::ErrorKind::WouldBlock {
                super::refused(
                    super::Refusal::Taken,
                    format!("another Zapret Hub holds {name}"),
                )
            } else {
                err
            });
        }
        // Holding the lock, whatever is there was left by a server that is gone.
        match std::fs::remove_file(name) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        let listener = UnixListener::bind(name)?;
        std::fs::set_permissions(name, Permissions::from_mode(0o600))?;
        Ok(Self {
            _lock: lock,
            listener,
        })
    }

    /// Answers clients until `accept` fails, and returns that error. `handler` gets the request
    /// line without its `\n` and returns the answer line. Each client has its own thread: one that
    /// never sends its line holds only that thread (spec 11).
    pub fn serve(self, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) -> io::Error {
        let handler = Arc::new(handler);
        loop {
            let client = match self.listener.accept() {
                Ok((client, _)) => client,
                Err(err) => return err,
            };
            // Between `bind` and `chmod` the socket had the umask's mode: the uid decides.
            if !trusted(&client) {
                continue;
            }
            let handler = Arc::clone(&handler);
            // No thread to be had: the closure is dropped and closes this client's socket; the
            // next client may get one.
            let _ = std::thread::Builder::new().spawn(move || {
                // An error here means the client went away: nobody to tell.
                let _ = super::answer(&client, &*handler);
            });
        }
    }
}

/// The folder of the socket `name`, made `0700` when `create` and missing. It must be a real
/// folder of this user that nobody else may enter: in `/tmp` another user could otherwise make it
/// first and take or fake the socket. A missing one is `NotFound`, a foreign one `PermissionDenied`.
fn private_dir(name: &Path, create: bool) -> io::Result<()> {
    let dir = name.parent().unwrap_or(Path::new("/"));
    if create {
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Err(err) if err.kind() != io::ErrorKind::AlreadyExists => return Err(err),
            _ => {}
        }
    }
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if private(&meta, uid) {
        Ok(())
    } else if meta.uid() != uid {
        Err(super::refused(
            super::Refusal::Foreign,
            format!(
                "{} belongs to another user (uid {})",
                dir.display(),
                meta.uid()
            ),
        ))
    } else {
        Err(super::refused(
            super::Refusal::Shared,
            format!("{} is not a folder only this user may enter", dir.display()),
        ))
    }
}

/// The client runs as this user or as root, like the pipe's SYSTEM, Administrators and user.
fn trusted(client: &UnixStream) -> bool {
    let mut cred = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: 0,
    };
    let mut len = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are ours and sized for SO_PEERCRED.
    let got = unsafe {
        libc::getsockopt(
            client.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &mut len,
        )
    };
    // SAFETY: geteuid has no preconditions.
    got == 0 && (cred.uid == 0 || cred.uid == unsafe { libc::geteuid() })
}

/// The CLI's end: sends `line` and returns the answer line. A full backlog makes `connect` wait,
/// so `busy` is not needed here. A socket file nobody listens on is `NotFound`, like none at all.
pub fn request(name: &str, line: &[u8], _busy: Duration) -> io::Result<Vec<u8>> {
    private_dir(Path::new(name), false)?;
    let socket = UnixStream::connect(name).map_err(|err| match err.kind() {
        io::ErrorKind::ConnectionRefused => io::Error::new(io::ErrorKind::NotFound, err),
        _ => err,
    })?;
    super::exchange(&socket, line)
}

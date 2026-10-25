//! The channel between `zapret-hub-cli` and the app (CLI spec 4, 5): a named pipe on Windows, a
//! Unix socket on Linux. Local only, one request line and one answer line per connection, a
//! thread per client. On both a name another server holds is `PermissionDenied`, so is a client
//! the channel does not let in, and no server is `NotFound`.

use std::io::{self, BufRead as _, BufReader, Read, Write};
use std::{error, fmt};

use crate::MAX_REQUEST;

#[cfg(windows)]
mod named;
#[cfg(windows)]
pub use named::*;

#[cfg(target_os = "linux")]
mod socket;
#[cfg(target_os = "linux")]
pub use socket::*;

/// Why the app's end of the channel did not open, for its notice (`refusal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Linux: other users may enter the socket's folder.
    Shared,
    /// Linux: the socket's folder is another user's.
    Foreign,
    /// Another program (or another Zapret Hub) holds the name.
    Taken,
}

#[derive(Debug)]
struct Refused {
    why: Refusal,
    text: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl error::Error for Refused {}

/// A `PermissionDenied` that says why (`refusal`).
fn refused(why: Refusal, text: String) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, Refused { why, text })
}

/// Why `err` from `Server::bind` refused the channel; `None` for any other error.
pub fn refusal(err: &io::Error) -> Option<Refusal> {
    err.get_ref()?.downcast_ref::<Refused>().map(|r| r.why)
}

/// Reads one line, keeping at most `MAX_REQUEST + 1` bytes of it, and writes the handler's answer.
/// A line the client never finished reaches no handler.
fn answer<S: Read + Write>(mut pipe: S, handler: &impl Fn(&[u8]) -> String) -> io::Result<S> {
    let mut reader = BufReader::new(&mut pipe);
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
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    drop(reader);
    let mut response = handler(&line);
    response.push('\n');
    pipe.write_all(response.as_bytes())?;
    Ok(pipe)
}

/// The CLI's half of a connection: sends `line` and returns the answer line. An answer without
/// its `\n` is UnexpectedEof.
fn exchange<S: Read + Write>(mut pipe: S, line: &[u8]) -> io::Result<Vec<u8>> {
    let mut message = line.to_vec();
    message.push(b'\n');
    pipe.write_all(&message)?;
    let mut answer = Vec::new();
    BufReader::new(pipe).read_until(b'\n', &mut answer)?;
    if answer.pop() != Some(b'\n') {
        // The handler failed, or whatever holds the name is not the app.
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the app closed the connection without an answer",
        ));
    }
    Ok(answer)
}

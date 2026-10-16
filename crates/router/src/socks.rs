//! A minimal SOCKS5 client: greeting, `UDP ASSOCIATE` and a reachability check.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

/// How long opening a flow waits for the proxy.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum SocksError {
    Timeout,
    AuthRequired,
    Refused(u8),
    Io(io::Error),
}

impl fmt::Display for SocksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("the proxy did not answer in time"),
            Self::AuthRequired => f.write_str("the proxy asks for authentication"),
            Self::Refused(7) => f.write_str("the proxy does not support UDP ASSOCIATE"),
            Self::Refused(n) => write!(f, "the proxy refused UDP ASSOCIATE: code {n}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SocksError {}

impl From<io::Error> for SocksError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Self::Timeout,
            _ => Self::Io(e),
        }
    }
}

/// A live association: closing `control` ends it.
#[derive(Debug)]
pub struct Association {
    pub control: TcpStream,
    pub relay: SocketAddrV4,
}

fn connect(proxy: SocketAddrV4, timeout: Duration) -> Result<TcpStream, SocksError> {
    let stream = TcpStream::connect_timeout(&SocketAddr::V4(proxy), timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

fn greet(stream: &mut TcpStream) -> Result<(), SocksError> {
    stream.write_all(&[5, 1, 0])?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply)?;
    if reply == [5, 0] {
        Ok(())
    } else {
        Err(SocksError::AuthRequired)
    }
}

/// Connects and greets; returns how long the proxy took to answer.
pub fn check(proxy: SocketAddrV4, timeout: Duration) -> Result<Duration, SocksError> {
    let started = Instant::now();
    let mut stream = connect(proxy, timeout)?;
    greet(&mut stream)?;
    Ok(started.elapsed())
}

/// Asks the proxy for a UDP relay towards `dst`.
pub fn udp_associate(
    proxy: SocketAddrV4,
    dst: SocketAddrV4,
    timeout: Duration,
) -> Result<Association, SocksError> {
    let mut stream = connect(proxy, timeout)?;
    greet(&mut stream)?;
    let mut request = vec![5, 3, 0, 1];
    request.extend_from_slice(&dst.ip().octets());
    request.extend_from_slice(&dst.port().to_be_bytes());
    stream.write_all(&request)?;
    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    if head[1] != 0 {
        return Err(SocksError::Refused(head[1]));
    }
    let addr_len = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            len[0] as usize
        }
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown address type").into()),
    };
    let mut rest = vec![0u8; addr_len + 2];
    stream.read_exact(&mut rest)?;
    let port = u16::from_be_bytes([rest[addr_len], rest[addr_len + 1]]);
    // The bound address is ignored: the proxy is local, so its own address is the relay.
    Ok(Association {
        control: stream,
        relay: SocketAddrV4::new(*proxy.ip(), port),
    })
}

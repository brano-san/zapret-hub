//! ICMP echo, like `System.Net.NetworkInformation.Ping.Send(host, 1000)` in `test zapret.ps1`.

use std::net::{SocketAddr, ToSocketAddrs};

use windows::Win32::NetworkManagement::IpHelper::{
    ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
};

/// Round-trip time in ms; `None` on timeout, error, or a name that does not resolve.
pub fn ping(host: &str, timeout_ms: u32) -> Option<u32> {
    let ip = (host, 0)
        .to_socket_addrs()
        .ok()?
        .find_map(|addr| match addr {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            SocketAddr::V6(_) => None,
        })?;
    let data = [0x61u8; 32];
    let mut reply = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
    // SAFETY: the handle is closed below; both buffers outlive the call and their sizes are passed.
    unsafe {
        let handle = IcmpCreateFile().ok()?;
        let count = IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            data.as_ptr().cast(),
            data.len() as u16,
            None,
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            timeout_ms,
        );
        let _ = IcmpCloseHandle(handle);
        if count == 0 {
            return None;
        }
        let echo = std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>());
        (echo.Status == 0).then_some(echo.RoundTripTime)
    }
}

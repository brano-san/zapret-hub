//! Socket options of the Linux router: the redirected TCP destination (`SO_ORIGINAL_DST`,
//! `IP6T_SO_ORIGINAL_DST`), the hub's own mark (`SO_MARK`, set before the first packet), the
//! TPROXY sockets (`IP_TRANSPARENT`/`IPV6_TRANSPARENT`, `IP_RECVORIGDSTADDR`/
//! `IPV6_RECVORIGDSTADDR`) and the original destination of a TPROXY datagram from its control
//! data.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

const SOL_IP: i32 = 0;
const IP_ORIGDSTADDR: i32 = 20;
const AF_INET: u16 = 2;
const SOL_IPV6: i32 = 41;
const IPV6_ORIGDSTADDR: i32 = 74;
const AF_INET6: u16 = 10;

/// The `IP_ORIGDSTADDR` (`sockaddr_in`) or `IPV6_ORIGDSTADDR` (`sockaddr_in6`) message among
/// `recvmsg` control data (native `cmsghdr` layout).
pub fn orig_dst(mut control: &[u8]) -> Option<SocketAddr> {
    const WORD: usize = size_of::<usize>();
    let align = |n: usize| (n + WORD - 1) & !(WORD - 1);
    let head = align(WORD + 8);
    let i32_at = |b: &[u8], at: usize| i32::from_ne_bytes(b[at..at + 4].try_into().unwrap());
    while control.len() >= head {
        let len = usize::from_ne_bytes(control[..WORD].try_into().unwrap());
        if len < head || len > control.len() {
            return None;
        }
        let (level, kind, data) = (
            i32_at(control, WORD),
            i32_at(control, WORD + 4),
            &control[head..len],
        );
        if level == SOL_IP && kind == IP_ORIGDSTADDR {
            return match data {
                [f0, f1, p0, p1, a, b, c, d, ..] if u16::from_ne_bytes([*f0, *f1]) == AF_INET => {
                    Some(SocketAddr::V4(SocketAddrV4::new(
                        Ipv4Addr::new(*a, *b, *c, *d),
                        u16::from_be_bytes([*p0, *p1]),
                    )))
                }
                _ => None,
            };
        }
        if level == SOL_IPV6 && kind == IPV6_ORIGDSTADDR {
            // family, port, flow info, address, scope id
            return match data {
                [f0, f1, p0, p1, _, _, _, _, rest @ ..]
                    if u16::from_ne_bytes([*f0, *f1]) == AF_INET6 && rest.len() >= 20 =>
                {
                    let ip: [u8; 16] = rest[..16].try_into().unwrap();
                    let scope = u32::from_ne_bytes(rest[16..20].try_into().unwrap());
                    Some(SocketAddr::V6(SocketAddrV6::new(
                        Ipv6Addr::from(ip),
                        u16::from_be_bytes([*p0, *p1]),
                        0,
                        scope,
                    )))
                }
                _ => None,
            };
        }
        control = control.get(align(len)..).unwrap_or_default();
    }
    None
}

#[cfg(target_os = "linux")]
mod sys {
    use std::io;
    use std::mem::MaybeUninit;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, TcpStream, UdpSocket};
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    use socket2::{Domain, MaybeUninitSlice, MsgHdrMut, SockAddr, SockRef, Socket, Type};

    fn ip(addr: &SockAddr) -> io::Result<SocketAddr> {
        addr.as_socket()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an IP address"))
    }

    /// Where a connection redirected by `nat output` was going (`SO_ORIGINAL_DST` or, on the
    /// IPv6 acceptor, `IP6T_SO_ORIGINAL_DST`).
    pub fn original_dst(stream: &TcpStream) -> io::Result<SocketAddr> {
        let socket = SockRef::from(stream);
        ip(&if stream.local_addr()?.is_ipv6() {
            socket.original_dst_v6()?
        } else {
            socket.original_dst_v4()?
        })
    }

    /// A TCP connection whose every packet carries `mark` (the SYN included: set before connect).
    pub fn connect_marked(dst: SocketAddr, mark: u32, timeout: Duration) -> io::Result<TcpStream> {
        let socket = Socket::new(Domain::for_address(dst), Type::STREAM, None)?;
        socket.set_mark(mark)?;
        socket.connect_timeout(&dst.into(), timeout)?;
        Ok(socket.into())
    }

    /// `IP_RECVORIGDSTADDR` (`IPV6_RECVORIGDSTADDR` with `v6`) on: `recv_orig` learns each
    /// datagram's original destination.
    pub fn recv_orig_dst(socket: &impl AsRawFd, v6: bool) -> io::Result<()> {
        let on: libc::c_int = 1;
        let (level, name) = if v6 {
            (libc::SOL_IPV6, libc::IPV6_RECVORIGDSTADDR)
        } else {
            (libc::SOL_IP, libc::IP_RECVORIGDSTADDR)
        };
        // SAFETY: a valid fd and a c_int option of the size passed.
        let rc = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                level,
                name,
                (&raw const on).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// A UDP socket bound to `addr`, which need not be local (`IP_TRANSPARENT`,
    /// `IPV6_TRANSPARENT`): a TPROXY listener, or a reply socket bound to a datagram's original
    /// destination (several may share it: `SO_REUSEADDR`). Its own packets carry `mark`. An IPv6
    /// one takes IPv6 only.
    pub fn transparent_udp(addr: SocketAddr, mark: u32) -> io::Result<UdpSocket> {
        let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, None)?;
        if addr.is_ipv6() {
            socket.set_only_v6(true)?;
            socket.set_ip_transparent_v6(true)?;
        } else {
            socket.set_ip_transparent_v4(true)?;
        }
        socket.set_reuse_address(true)?;
        socket.set_mark(mark)?;
        recv_orig_dst(&socket, addr.is_ipv6())?;
        socket.bind(&addr.into())?;
        Ok(socket.into())
    }

    /// A datagram: its length, source and original destination (`None` when the socket lacks
    /// `recv_orig_dst`).
    pub fn recv_orig(
        socket: &UdpSocket,
        buf: &mut [u8],
    ) -> io::Result<(usize, SocketAddr, Option<SocketAddr>)> {
        let mut control = [MaybeUninit::<u8>::uninit(); 128];
        // Room for either family: the kernel writes the family it has.
        let mut from = SockAddr::from(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0));
        // SAFETY: recvmsg only writes into the buffer; initialized bytes stay initialized.
        let uninit = unsafe { &mut *(std::ptr::from_mut::<[u8]>(buf) as *mut [MaybeUninit<u8>]) };
        let mut bufs = [MaybeUninitSlice::new(uninit)];
        let mut msg = MsgHdrMut::new()
            .with_addr(&mut from)
            .with_buffers(&mut bufs)
            .with_control(&mut control);
        let n = SockRef::from(socket).recvmsg(&mut msg, 0)?;
        let filled = msg.control_len();
        // SAFETY: the kernel initialized the first `filled` control bytes.
        let control = unsafe { std::slice::from_raw_parts(control.as_ptr().cast::<u8>(), filled) };
        Ok((n, ip(&from)?, super::orig_dst(control)))
    }

    /// Each local IPv4 address: `(address, prefix length, broadcast)` (`getifaddrs`).
    pub fn ipv4_addrs() -> io::Result<Vec<(Ipv4Addr, u8, Option<Ipv4Addr>)>> {
        let mut head = std::ptr::null_mut();
        // SAFETY: getifaddrs fills `head`, freed below.
        if unsafe { libc::getifaddrs(&raw mut head) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let v4 = |sa: *const libc::sockaddr| {
            // SAFETY: getifaddrs' addresses live until freeifaddrs; the family is checked first.
            unsafe {
                (!sa.is_null() && i32::from((*sa).sa_family) == libc::AF_INET).then(|| {
                    Ipv4Addr::from(u32::from_be(
                        (*sa.cast::<libc::sockaddr_in>()).sin_addr.s_addr,
                    ))
                })
            }
        };
        let mut out = Vec::new();
        let mut at = head;
        while !at.is_null() {
            // SAFETY: a node of the list getifaddrs returned.
            let ifa = unsafe { &*at };
            if let (Some(addr), Some(mask)) = (v4(ifa.ifa_addr), v4(ifa.ifa_netmask)) {
                // Without IFF_BROADCAST the union holds a point-to-point peer.
                let brd = (ifa.ifa_flags & libc::IFF_BROADCAST as u32 != 0)
                    .then(|| v4(ifa.ifa_ifu))
                    .flatten();
                out.push((addr, u32::from(mask).leading_ones() as u8, brd));
            }
            at = ifa.ifa_next;
        }
        // SAFETY: the list getifaddrs returned, freed once.
        unsafe { libc::freeifaddrs(head) };
        Ok(out)
    }
}

#[cfg(target_os = "linux")]
pub use sys::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn cmsg(level: i32, kind: i32, data: &[u8]) -> Vec<u8> {
        let word = size_of::<usize>();
        let head = (word + 8).next_multiple_of(word);
        let mut out = (head + data.len()).to_ne_bytes().to_vec();
        out.extend(level.to_ne_bytes());
        out.extend(kind.to_ne_bytes());
        out.resize(head, 0);
        out.extend(data);
        out.resize(out.len().next_multiple_of(word), 0);
        out
    }

    fn sockaddr_in(ip: [u8; 4], port: u16) -> Vec<u8> {
        let mut out = AF_INET.to_ne_bytes().to_vec();
        out.extend(port.to_be_bytes());
        out.extend(ip);
        out.extend([0; 8]);
        out
    }

    #[test]
    fn the_original_destination_is_found_among_control_messages() {
        let dst = sockaddr_in([1, 2, 3, 4], 5000);
        let mut control = cmsg(SOL_IP, 8, &[1, 0, 0, 0]); // IP_TTL first
        control.extend(cmsg(SOL_IP, IP_ORIGDSTADDR, &dst));
        assert_eq!(orig_dst(&control), Some("1.2.3.4:5000".parse().unwrap()));
        assert_eq!(orig_dst(&cmsg(SOL_IP, 8, &[1, 0, 0, 0])), None);
        assert_eq!(orig_dst(&[]), None);
        let mut short = cmsg(SOL_IP, IP_ORIGDSTADDR, &dst);
        short.truncate(20);
        assert_eq!(orig_dst(&short), None, "length past the buffer");
        assert_eq!(
            orig_dst(&cmsg(SOL_IP, IP_ORIGDSTADDR, &dst[..4])),
            None,
            "truncated address"
        );
        let mut v6 = dst.clone();
        v6[..2].copy_from_slice(&10u16.to_ne_bytes());
        assert_eq!(
            orig_dst(&cmsg(SOL_IP, IP_ORIGDSTADDR, &v6)),
            None,
            "AF_INET6"
        );
    }

    #[test]
    fn an_ipv6_original_destination_is_found_too() {
        let mut dst = AF_INET6.to_ne_bytes().to_vec();
        dst.extend(443u16.to_be_bytes());
        dst.extend([0; 4]); // flow info
        dst.extend("2001:db8::7".parse::<Ipv6Addr>().unwrap().octets());
        dst.extend(0u32.to_ne_bytes()); // scope id
        let mut control = cmsg(SOL_IP, 8, &[1, 0, 0, 0]);
        control.extend(cmsg(SOL_IPV6, IPV6_ORIGDSTADDR, &dst));
        assert_eq!(
            orig_dst(&control),
            Some("[2001:db8::7]:443".parse().unwrap())
        );
        assert_eq!(
            orig_dst(&cmsg(SOL_IPV6, IPV6_ORIGDSTADDR, &dst[..24])),
            None,
            "truncated address"
        );
        assert_eq!(
            orig_dst(&cmsg(SOL_IP, IP_ORIGDSTADDR, &dst)),
            None,
            "an IPv6 address under the IPv4 level"
        );
    }

    /// A plain socket gets its own address as the original destination: the kernel's layout.
    #[cfg(target_os = "linux")]
    #[test]
    fn recv_orig_reads_the_kernel_control_data() {
        use std::net::UdpSocket;
        for host in ["127.0.0.1", "[::1]"] {
            let socket = UdpSocket::bind(format!("{host}:0")).unwrap();
            recv_orig_dst(&socket, host.starts_with('[')).unwrap();
            let sender = UdpSocket::bind(format!("{host}:0")).unwrap();
            sender.send_to(b"hi", socket.local_addr().unwrap()).unwrap();
            let mut buf = [0; 16];
            let (n, src, dst) = recv_orig(&socket, &mut buf).unwrap();
            assert_eq!(&buf[..n], b"hi");
            assert_eq!(src, sender.local_addr().unwrap());
            assert_eq!(dst, Some(socket.local_addr().unwrap()));
        }
    }

    /// `SO_MARK` needs `CAP_NET_ADMIN`: refused without it, a working connection with it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_marked_connection_needs_the_capability() {
        use std::time::Duration;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let result = connect_marked(addr, crate::linux::nft::MARK_OWN, Duration::from_secs(5));
        if crate::linux::caps::present() {
            result.unwrap();
        } else {
            assert_eq!(
                result.unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }
}

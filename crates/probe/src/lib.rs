//! Network checks: `curl.exe` with the arguments `test zapret.ps1` uses (and HTTPS to a pinned
//! address, DoH), ICMP ping, SOCKS5, and UDP probes: Discord IP discovery, STUN, DNS.

pub mod curl;
pub mod ping;
pub mod socks;
pub mod udp;

//! Linux routing plumbing: the nftables table, capabilities, socket options and the DNS sniffer.
//! The pure parts build everywhere (their tests run on Windows too); the system calls are
//! Linux-only.

pub mod caps;
#[cfg(target_os = "linux")]
pub(crate) mod live;
pub mod nft;
pub mod sniff;
pub mod sockopt;

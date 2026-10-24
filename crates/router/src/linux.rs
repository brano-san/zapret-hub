//! Linux routing plumbing: the nftables table, capabilities, socket options and the DNS sniffer.
//! The pure parts build everywhere (their tests run on Windows too); the system calls are
//! Linux-only.

pub mod nft;

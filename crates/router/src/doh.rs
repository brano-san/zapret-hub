//! Whether the system itself resolves names over encrypted DNS: then the DNS sniffer sees no
//! answers and domain rules match nothing.

#[cfg(windows)]
mod registry;
#[cfg(windows)]
pub use registry::system_doh;

#[cfg(target_os = "linux")]
mod resolved;
#[cfg(target_os = "linux")]
pub use resolved::system_doh;

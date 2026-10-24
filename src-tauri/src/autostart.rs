//! Starting at logon (spec 4.2): Windows' Task Scheduler entry, Linux's XDG autostart entry.

#[cfg(not(windows))]
mod desktop;
#[cfg(windows)]
mod task;

#[cfg(not(windows))]
pub use desktop::{disable, enable, is_enabled, refresh};
#[cfg(windows)]
pub use task::{disable, enable, refresh};

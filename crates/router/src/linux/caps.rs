//! The capabilities routing needs: `CAP_NET_ADMIN` (nft, ip, `SO_MARK`, `IP_TRANSPARENT`) and
//! `CAP_NET_RAW` (the DNS sniffer). The hub holds them through file capabilities (`grant`) and
//! passes them to the `nft`/`ip` it starts as ambient capabilities (`inherit`), without pkexec.

pub const NET_ADMIN: u32 = 12;
pub const NET_RAW: u32 = 13;
const BITS: u64 = 1 << NET_ADMIN | 1 << NET_RAW;

/// What `Live::open` says when the capabilities are missing.
pub const HINT: &str = "routing needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities: \
     click Allow routing (or run zapret-hub-cli routes grant) and restart Zapret Hub; \
     an update of Zapret Hub removes them, grant them again then";

/// The effective set from `/proc/<pid>/status` text (`CapEff:\t0000003000000000`).
pub fn effective(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
}

/// `effective` holds both routing capabilities.
pub fn enough(effective: u64) -> bool {
    effective & BITS == BITS
}

/// This process may route now.
#[cfg(target_os = "linux")]
pub fn present() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| effective(&s))
        .is_some_and(enough)
}

/// The root step of `grant`: `setcap` on `exe`, so the hub started from it has the capabilities.
#[cfg(target_os = "linux")]
pub fn grant_batch(exe: &std::path::Path) -> zapret::root::Batch {
    let mut batch = zapret::root::Batch::new();
    batch.run(&[
        &zapret::root::tool("setcap"),
        "cap_net_admin,cap_net_raw+eip",
        &exe.display().to_string(),
    ]);
    batch
}

/// Grants the capabilities to `exe` (one sudo -n/pkexec); they take effect on its next start.
#[cfg(target_os = "linux")]
pub fn grant(exe: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    grant_batch(exe)
        .exec()
        .with_context(|| format!("cannot grant routing capabilities to {}", exe.display()))
}

/// `cmd`'s process starts with this process's routing capabilities as ambient ones, so a child
/// without file capabilities (`nft`, `ip`) keeps them. Best effort: without them the child runs
/// unprivileged and says so itself ("Operation not permitted").
#[cfg(target_os = "linux")]
pub fn inherit(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt as _;

    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    const VERSION_3: u32 = 0x2008_0522;

    // SAFETY: only raw syscalls between fork and exec (async-signal-safe), on stack memory.
    unsafe {
        cmd.pre_exec(|| {
            let mut header = Header {
                version: VERSION_3,
                pid: 0,
            };
            let mut data = [Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            }; 2];
            if libc::syscall(libc::SYS_capget, &raw mut header, data.as_mut_ptr()) == 0 {
                // An ambient capability must be inheritable, and only a permitted one may be.
                data[0].inheritable |= data[0].permitted & BITS as u32;
                if libc::syscall(libc::SYS_capset, &raw const header, data.as_ptr()) == 0 {
                    for cap in [NET_ADMIN, NET_RAW] {
                        libc::prctl(
                            libc::PR_CAP_AMBIENT,
                            libc::PR_CAP_AMBIENT_RAISE,
                            libc::c_ulong::from(cap),
                            0 as libc::c_ulong,
                            0 as libc::c_ulong,
                        );
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_effective_set_is_read_from_status() {
        let status = "Name:\tzapret-hub\nCapInh:\t0000000000000000\nCapPrm:\t0000000000003000\n\
                      CapEff:\t0000000000003000\nCapBnd:\t000001ffffffffff\n";
        assert_eq!(effective(status), Some(0x3000));
        assert!(enough(0x3000));
        assert!(enough(0x1ff_ffff_ffff));
        assert!(!enough(0x1000), "NET_ADMIN alone");
        assert!(!enough(0x2000), "NET_RAW alone");
        assert_eq!(effective("Name:\tx\n"), None);
        assert_eq!(effective("CapEff:\tzz\n"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn grant_is_one_setcap_on_the_exe() {
        let script = grant_batch(std::path::Path::new("/opt/Zapret Hub/zapret-hub")).script();
        assert!(script.starts_with("set -e\n'"), "{script}");
        assert!(
            script.ends_with(
                "setcap' 'cap_net_admin,cap_net_raw+eip' '/opt/Zapret Hub/zapret-hub'\n"
            ),
            "{script}"
        );
    }

    /// Without the capabilities the child still starts; with them (root, a user namespace) it
    /// holds them as ambient ones.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_inherits_the_capabilities_it_can() {
        let out = inherit(std::process::Command::new("cat").arg("/proc/self/status"))
            .output()
            .unwrap();
        assert!(out.status.success());
        let status = String::from_utf8(out.stdout).unwrap();
        let ambient = status
            .lines()
            .find_map(|l| l.strip_prefix("CapAmb:"))
            .map(|hex| u64::from_str_radix(hex.trim(), 16).unwrap())
            .unwrap();
        assert_eq!(ambient & BITS == BITS, present(), "{status}");
    }
}

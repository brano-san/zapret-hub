//! The capabilities routing needs: `CAP_NET_ADMIN` (nft, ip, `SO_MARK`, `IP_TRANSPARENT`) and
//! `CAP_NET_RAW` (the DNS sniffer). The hub holds them through file capabilities (`grant`) and
//! passes them to the `nft`/`ip` it starts as ambient capabilities (`inherit`), without pkexec.

pub const NET_ADMIN: u32 = 12;
pub const NET_RAW: u32 = 13;
const BITS: u64 = 1 << NET_ADMIN | 1 << NET_RAW;

/// What `Live::open` says when the capabilities are missing.
pub const HINT: &str = "routing needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities: \
     click «Разрешить маршрутизацию» on the Routes tab (or run zapret-hub-cli routes grant) and \
     restart Zapret Hub; \
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

/// Why `exe` may not get the capabilities: replaced since the hub started (`/proc/self/exe` says
/// `… (deleted)`), a symlink or no regular file (root would follow a swapped path), or owned by
/// neither the user (`uid`) nor root.
pub fn refuse(exe: &str, symlink: bool, file: bool, owner: u32, uid: u32) -> Option<String> {
    if exe.ends_with(" (deleted)") {
        return Some(format!(
            "{exe}: Zapret Hub was updated while running: restart it, then grant routing"
        ));
    }
    if symlink || !file {
        return Some(format!("{exe} is not a regular file"));
    }
    (owner != uid && owner != 0).then(|| {
        format!(
            "{exe} belongs to another user (uid {owner}): only your own or root's file is granted"
        )
    })
}

/// The root step of `grant`: `exe` becomes root's, runnable by the group `gid` only (anyone who
/// runs it holds the capabilities), then gets them (`chown` drops file capabilities, so it goes
/// first).
#[cfg(target_os = "linux")]
pub fn grant_batch(exe: &std::path::Path, gid: u32) -> zapret::root::Batch {
    use zapret::root::tool;
    let path = exe.display().to_string();
    let mut batch = zapret::root::Batch::new();
    batch
        .run(&[&tool("chown"), &format!("root:{gid}"), &path])
        .run(&[&tool("chmod"), "0750", &path])
        .run(&[&tool("setcap"), "cap_net_admin,cap_net_raw+eip", &path]);
    batch
}

/// Grants the capabilities to `exe` (one sudo -n/pkexec) for the user's primary group; they take
/// effect on its next start.
#[cfg(target_os = "linux")]
pub fn grant(exe: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    use anyhow::Context as _;
    let text = exe.display().to_string();
    let why = match std::fs::symlink_metadata(exe) {
        _ if text.ends_with(" (deleted)") => refuse(&text, false, true, 0, 0),
        Ok(meta) => refuse(
            &text,
            meta.file_type().is_symlink(),
            meta.is_file(),
            meta.uid(),
            // SAFETY: getuid and getgid cannot fail.
            unsafe { libc::getuid() },
        ),
        Err(err) => Some(format!("{text}: {err}")),
    };
    if let Some(why) = why {
        anyhow::bail!(why);
    }
    // SAFETY: as above.
    grant_batch(exe, unsafe { libc::getgid() })
        .exec()
        .with_context(|| format!("cannot grant routing capabilities to {text}"))
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
    fn only_the_users_or_roots_own_regular_file_is_granted() {
        let exe = "/home/u/zh/zapret-hub";
        assert_eq!(refuse(exe, false, true, 1000, 1000), None);
        assert_eq!(refuse(exe, false, true, 0, 1000), None, "granted before");
        assert!(
            refuse(exe, false, true, 1001, 1000)
                .unwrap()
                .contains("another user")
        );
        assert!(
            refuse(exe, true, false, 1000, 1000)
                .unwrap()
                .contains("regular file")
        );
        assert!(
            refuse(exe, false, false, 1000, 1000)
                .unwrap()
                .contains("regular file")
        );
        let updated = refuse("/home/u/zh/zapret-hub (deleted)", false, true, 1000, 1000).unwrap();
        assert!(updated.contains("restart it"), "{updated}");
    }

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
    fn grant_limits_who_runs_the_exe_then_sets_the_capabilities() {
        let script = grant_batch(std::path::Path::new("/opt/Zapret Hub/zapret-hub"), 1000).script();
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines.len(), 4, "{script}");
        assert_eq!(lines[0], "set -e", "{script}");
        let ends = [
            "chown' 'root:1000' '/opt/Zapret Hub/zapret-hub'",
            "chmod' '0750' '/opt/Zapret Hub/zapret-hub'",
            "setcap' 'cap_net_admin,cap_net_raw+eip' '/opt/Zapret Hub/zapret-hub'",
        ];
        for (line, end) in lines[1..].iter().zip(ends) {
            assert!(line.ends_with(end), "{script}");
        }
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

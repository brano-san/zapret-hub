//! The capabilities routing needs: `CAP_NET_ADMIN` (nft, ip, `SO_MARK`, `IP_TRANSPARENT`) and
//! `CAP_NET_RAW` (the DNS sniffer). The file capabilities (`grant`) sit on `zapret-hub-cli`, the
//! launcher: started by the hub (`relaunch`), it starts the hub again with them as ambient ones
//! (`launch`). A hub with file capabilities of its own would run in secure mode (`AT_SECURE`),
//! where GLib finds no D-Bus session bus: no tray, no notifications. The hub passes them on to the
//! `nft`/`ip` it starts (`inherit`), and to nothing else (`keep_from_children`).

#[cfg(target_os = "linux")]
use std::path::Path;

pub const NET_ADMIN: u32 = 12;
pub const NET_RAW: u32 = 13;
const BITS: u64 = 1 << NET_ADMIN | 1 << NET_RAW;

/// The hub's program; the launcher is next to it.
pub const HUB: &str = "zapret-hub";
/// The launcher: `zapret-hub-cli` holds the file capabilities.
pub const LAUNCHER: &str = "zapret-hub-cli";
/// `zapret-hub-cli --launch-hub [hub args]`: the launcher's step (`launch`).
pub const LAUNCH_ARG: &str = "--launch-hub";
/// Set for the hub the launcher starts: one that still lacks the capabilities (a `nosuid`
/// mount, a sandbox) does not go through the launcher again.
pub const LAUNCHED_ENV: &str = "ZAPRET_HUB_LAUNCHED";

/// What `Live::open` says when the launcher holds the capabilities but this hub does not.
pub const RESTART_HINT: &str = "routing capabilities are granted, but this Zapret Hub started \
     without them: quit it and start it again; if it still has none, click «Разрешить \
     маршрутизацию» again (zapret-hub-cli --launch-hub says why on stderr)";

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

/// The permitted set of a `security.capability` xattr (`vfs_cap_data`, revision 1 to 3).
pub fn file_permitted(xattr: &[u8]) -> Option<u64> {
    let word = |i: usize| {
        Some(u32::from_le_bytes(
            xattr.get(i * 4..i * 4 + 4)?.try_into().ok()?,
        ))
    };
    let low = u64::from(word(1)?);
    match word(0)? & 0xFF00_0000 {
        0x0100_0000 => Some(low),
        0x0200_0000 | 0x0300_0000 => Some(low | u64::from(word(3)?) << 32),
        _ => None,
    }
}

/// `path` holds both routing capabilities as file capabilities (`grant`).
#[cfg(target_os = "linux")]
pub fn file_grants(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut buf = [0u8; 32];
    // SAFETY: both strings end in NUL, and `buf` is ours with its length.
    let n = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    usize::try_from(n)
        .ok()
        .and_then(|n| file_permitted(&buf[..n]))
        .is_some_and(enough)
}

/// Why the launcher must not start the hub file with the capabilities: anything but a regular
/// file of root and the user's group `gid` with mode `0750`, as `grant` leaves it. Another file
/// (one the user or a program of theirs put there, a link to another program of root's) would run
/// with them.
pub fn refuse_hub(file: bool, owner: u32, group: u32, mode: u32, gid: u32) -> Option<String> {
    (!file || owner != 0 || group != gid || mode & 0o7777 != 0o750).then(|| {
        "is not root's file of your group with mode 0750, as «Разрешить маршрутизацию» leaves \
         it: grant routing again"
            .to_owned()
    })
}

/// The root step of `grant`: the hub and the launcher become root's, runnable by the group `gid`
/// only (whoever runs the launcher holds the capabilities, and the hub it starts must be a file
/// only root may change), then the launcher gets them. `chown` drops file capabilities, so it goes
/// first; on the hub it also drops those an older version put there.
#[cfg(target_os = "linux")]
pub fn grant_batch(hub: &Path, launcher: &Path, gid: u32) -> zapret::root::Batch {
    use zapret::root::tool;
    let mut batch = zapret::root::Batch::new();
    for exe in [hub, launcher] {
        let path = exe.display().to_string();
        batch
            .run(&[&tool("chown"), &format!("root:{gid}"), &path])
            .run(&[&tool("chmod"), "0750", &path]);
    }
    batch.run(&[
        &tool("setcap"),
        "cap_net_admin,cap_net_raw+eip",
        &launcher.display().to_string(),
    ]);
    batch
}

/// Grants the capabilities to the launcher next to `hub` (one sudo -n/pkexec) for the user's
/// primary group; the hub gets them at its next start.
#[cfg(target_os = "linux")]
pub fn grant(hub: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    use anyhow::Context as _;
    let launcher = hub.with_file_name(LAUNCHER);
    for exe in [hub, &launcher] {
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
    }
    // SAFETY: as above.
    grant_batch(hub, &launcher, unsafe { libc::getgid() })
        .exec()
        .with_context(|| {
            format!(
                "cannot grant routing capabilities to {}",
                launcher.display()
            )
        })
}

/// What loads code from the environment into the hub (GTK, GIO and GStreamer modules): unset for
/// the hub the launcher starts. The C library already drops `LD_*` in the launcher.
// ponytail: a deny list; a module path it misses runs with the capabilities. They are the user's
// own anyway: only their group may run the launcher.
const UNSAFE_ENV: [&str; 9] = [
    "GTK_MODULES",
    "GTK3_MODULES",
    "GTK_PATH",
    "GIO_EXTRA_MODULES",
    "GIO_MODULE_DIR",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GDK_PIXBUF_MODULE_FILE",
];

/// The launcher's step: the hub next to this exe, opened without following a link and checked on
/// that open file (`refuse_hub`), replaces this process, with `args` and the capabilities as
/// ambient ones. A hub the check refuses still starts, without them, and says why on stderr.
/// Returns only when no hub started.
#[cfg(target_os = "linux")]
pub fn launch(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Error {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::unix::process::CommandExt as _;

    let args: Vec<_> = args.into_iter().collect();
    let hub = match std::env::current_exe() {
        Ok(me) => me.with_file_name(HUB),
        Err(err) => return err.into(),
    };
    let run = || -> anyhow::Result<std::convert::Infallible> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&hub)
            .map_err(|e| anyhow::anyhow!("{}: {e}", hub.display()))?;
        let meta = file.metadata()?;
        // SAFETY: getgid cannot fail.
        let gid = unsafe { libc::getgid() };
        if let Some(why) = refuse_hub(meta.is_file(), meta.uid(), meta.gid(), meta.mode(), gid) {
            anyhow::bail!("{} {why}", hub.display());
        }
        // The file checked, not whatever the path names by now.
        let mut cmd = std::process::Command::new(format!("/proc/self/fd/{}", file.as_raw_fd()));
        cmd.arg0(&hub).args(&args).env(LAUNCHED_ENV, "1");
        for var in UNSAFE_ENV {
            cmd.env_remove(var);
        }
        Err(inherit(&mut cmd).exec().into())
    };
    let Err(why) = run();
    eprintln!("Zapret Hub starts without the routing capabilities: {why:#}");
    drop_all();
    let err = std::process::Command::new(&hub)
        .args(&args)
        .env(LAUNCHED_ENV, "1")
        .exec();
    anyhow::Error::new(err).context(format!("{} did not start", hub.display()))
}

/// A hub started without the capabilities while the launcher next to it holds them: replaces this
/// process with the launcher, which starts the hub again with them. `launched`: this hub is the
/// launcher's already. The launcher's error, or `None` when there was nothing to do.
#[cfg(target_os = "linux")]
pub fn relaunch(launched: bool) -> Option<std::io::Error> {
    use std::os::unix::process::CommandExt as _;
    if launched || present() {
        return None;
    }
    let launcher = std::env::current_exe().ok()?.with_file_name(LAUNCHER);
    file_grants(&launcher).then(|| {
        std::process::Command::new(&launcher)
            .arg(LAUNCH_ARG)
            .args(std::env::args_os().skip(1))
            .exec()
    })
}

/// The hub keeps its capabilities to itself: what it starts (a browser, a file manager, the web
/// view's processes) gets none; `inherit` hands them to `nft` and `ip` only.
#[cfg(target_os = "linux")]
pub fn keep_from_children() {
    // SAFETY: a plain prctl without pointers.
    unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
    }
}

/// The CLI's own commands need none of its file capabilities: they go first thing.
#[cfg(target_os = "linux")]
pub fn drop_all() {
    let header = Header {
        version: VERSION_3,
        pid: 0,
    };
    let data = [Data {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: both point at our stack memory, of the sizes capset reads.
    unsafe { libc::syscall(libc::SYS_capset, &raw const header, data.as_ptr()) };
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Header {
    version: u32,
    pid: i32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct Data {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

#[cfg(target_os = "linux")]
const VERSION_3: u32 = 0x2008_0522;

/// `cmd`'s process starts with this process's routing capabilities as ambient ones, so a child
/// without file capabilities (`nft`, `ip`) keeps them. Best effort: without them the child runs
/// unprivileged and says so itself ("Operation not permitted").
#[cfg(target_os = "linux")]
pub fn inherit(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt as _;

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
    fn file_capabilities_are_read_from_the_xattr() {
        let words = |w: &[u32]| w.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
        // `setcap cap_net_admin,cap_net_raw+eip`: revision 2 with the effective flag.
        let v2 = words(&[0x0200_0001, 0x3000, 0x3000, 0, 0]);
        assert_eq!(file_permitted(&v2), Some(0x3000));
        assert!(enough(file_permitted(&v2).unwrap()));
        let v3 = words(&[0x0300_0001, 0x1000, 0, 0x1, 0, 0]);
        assert_eq!(file_permitted(&v3), Some(0x1_0000_1000));
        let v1 = words(&[0x0100_0000, 0x3000, 0]);
        assert_eq!(file_permitted(&v1), Some(0x3000));
        assert_eq!(file_permitted(&words(&[0x0400_0000, 0x3000, 0])), None);
        assert_eq!(file_permitted(&v2[..10]), None, "cut short");
    }

    #[test]
    fn only_a_hub_as_the_grant_left_it_is_launched() {
        assert_eq!(refuse_hub(true, 0, 1000, 0o100_750, 1000), None);
        for (file, owner, group, mode) in [
            (false, 0, 1000, 0o750),
            (true, 1000, 1000, 0o750),
            (true, 0, 0, 0o750),
            (true, 0, 1000, 0o755),
            (true, 0, 1000, 0o770),
            (true, 0, 1000, 0o4750),
        ] {
            assert!(
                refuse_hub(file, owner, group, mode, 1000).is_some(),
                "{file} {owner} {group} {mode:o}"
            );
        }
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
    fn grant_limits_who_runs_both_then_gives_the_launcher_the_capabilities() {
        let script = grant_batch(
            Path::new("/opt/Zapret Hub/zapret-hub"),
            Path::new("/opt/Zapret Hub/zapret-hub-cli"),
            1000,
        )
        .script();
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines.len(), 6, "{script}");
        assert_eq!(lines[0], "set -e", "{script}");
        let ends = [
            "chown' 'root:1000' '/opt/Zapret Hub/zapret-hub'",
            "chmod' '0750' '/opt/Zapret Hub/zapret-hub'",
            "chown' 'root:1000' '/opt/Zapret Hub/zapret-hub-cli'",
            "chmod' '0750' '/opt/Zapret Hub/zapret-hub-cli'",
            "setcap' 'cap_net_admin,cap_net_raw+eip' '/opt/Zapret Hub/zapret-hub-cli'",
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

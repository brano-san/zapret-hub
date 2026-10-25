//! The capabilities routing needs: `CAP_NET_ADMIN` (nft, ip, `SO_MARK`, `IP_TRANSPARENT`) and
//! `CAP_NET_RAW` (the DNS sniffer).
//!
//! `grant` copies the hub and `zapret-hub-cli` into the root-owned `INSTALL_DIR`, gives the copy
//! of the CLI (the launcher) the file capabilities and records the one user they are for
//! (`UID_FILE`). A hub started without them (`relaunch`) whose program is the installed one,
//! byte for byte, goes through the launcher, which checks the user and starts the installed hub
//! with them as ambient ones (`launch`). A hub with file capabilities of its own would run in
//! secure mode (`AT_SECURE`), where GLib finds no D-Bus session bus: no tray, no notifications.
//! The hub passes them on to the `nft`/`ip` it starts (`inherit`), and to nothing else
//! (`keep_from_children`).
//!
//! Trust: the recorded user gets the capabilities through the hub, and with them whatever the hub
//! loads from that user's own settings (GTK modules in `~/.config`): the grant is that user's
//! choice, nobody else's.

#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

pub const NET_ADMIN: u32 = 12;
pub const NET_RAW: u32 = 13;
const BITS: u64 = 1 << NET_ADMIN | 1 << NET_RAW;

/// The hub's program.
pub const HUB: &str = "zapret-hub";
/// The launcher: the installed `zapret-hub-cli` holds the file capabilities.
pub const LAUNCHER: &str = "zapret-hub-cli";
/// Root's copies of the hub and the launcher, and `UID_FILE`.
pub const INSTALL_DIR: &str = "/usr/local/lib/zapret-hub";
/// The uid the launcher raises the capabilities for, in `INSTALL_DIR`.
pub const UID_FILE: &str = "uid";
/// `zapret-hub-cli --launch-hub <the hub started> [hub flags]`: the launcher's step (`launch`).
pub const LAUNCH_ARG: &str = "--launch-hub";
/// Set for the hub the launcher starts: one that still lacks the capabilities (a `nosuid`
/// mount, a sandbox) does not go through the launcher again.
pub const LAUNCHED_ENV: &str = "ZAPRET_HUB_LAUNCHED";
/// The hub program the user started, for the installed hub the launcher runs (`hub_exe`).
pub const ORIGIN_ENV: &str = "ZAPRET_HUB_ORIGIN";
/// The hub's own flags: the only arguments the launcher passes on.
#[cfg(target_os = "linux")]
const HUB_FLAGS: [&str; 1] = ["--autostart"];

/// What `Live::open` says when the capabilities are missing and were never granted.
pub const HINT: &str = "routing needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities: \
     click «Разрешить маршрутизацию» on the Routes tab (or run zapret-hub-cli routes grant) and \
     restart Zapret Hub; \
     an update of Zapret Hub needs the grant again";

/// How the grant on this machine stands for this hub (`installed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// No launcher with the capabilities.
    None,
    /// Granted to another user of this machine.
    OtherUser,
    /// Granted to an older (or other) build of the hub: an update since.
    Stale,
    /// The launcher would start this hub with them.
    Ready,
}

/// What `Live::open` says for a hub without the capabilities.
pub fn hint(installed: Installed) -> &'static str {
    match installed {
        Installed::None => HINT,
        Installed::OtherUser => {
            "routing capabilities on this computer are granted to another user: click \
             «Разрешить маршрутизацию» to take them over, then restart Zapret Hub"
        }
        Installed::Stale => {
            "Zapret Hub changed (an update) since routing was granted: click «Разрешить \
             маршрутизацию» again, then restart Zapret Hub"
        }
        Installed::Ready => {
            "routing capabilities are granted, but this Zapret Hub started without them: quit \
             it and start it again; if it still has none, click «Разрешить маршрутизацию» again \
             (zapret-hub-cli --launch-hub says why on stderr)"
        }
    }
}

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

/// Why `exe` may not be copied for the grant: replaced since the hub started (`/proc/self/exe`
/// says `… (deleted)`), no regular file, or owned by neither the user (`uid`) nor root.
pub fn refuse(exe: &str, file: bool, owner: u32, uid: u32) -> Option<String> {
    if exe.ends_with(" (deleted)") {
        return Some(format!(
            "{exe}: Zapret Hub was updated while running: restart it, then grant routing"
        ));
    }
    if !file {
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

/// Why a file in `INSTALL_DIR` is not to be trusted: anything but a regular file (or the folder)
/// of root that nobody else may change.
pub fn refuse_root_file(kind_ok: bool, owner: u32, mode: u32) -> Option<String> {
    (!kind_ok || owner != 0 || mode & 0o022 != 0).then(|| {
        "is not root's alone, as «Разрешить маршрутизацию» leaves it: grant routing again"
            .to_owned()
    })
}

/// The uid `UID_FILE` holds.
pub fn parse_uid(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

/// An environment variable the launcher passes to the hub: what a desktop session needs, nothing
/// that names code to load (an allow-list; `LD_*` the C library already drops in the launcher).
pub fn kept_env(name: &str) -> bool {
    const KEPT: [&str; 23] = [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "LANG",
        "LANGUAGE",
        "TZ",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_CONFIG_DIRS",
        "XDG_DATA_DIRS",
        "GTK_USE_PORTAL",
    ];
    KEPT.contains(&name) || name.starts_with("LC_")
}

/// The root step of `grant`, from the hub and launcher files the hub holds open (`/proc/<pid>/
/// fd/<n>`: root reads exactly the files checked, whatever their paths name by now): `INSTALL_DIR`
/// as root's, `UID_FILE` with `uid`, then each copy written beside its place, made root's `0755`
/// (the launcher's with the capabilities) and renamed into it.
#[cfg(target_os = "linux")]
pub fn grant_batch(hub: &str, launcher: &str, uid: u32) -> zapret::root::Batch {
    use zapret::root::tool;
    let dir = Path::new(INSTALL_DIR);
    let at = |name: &str| dir.join(name).display().to_string();
    let mut batch = zapret::root::Batch::new();
    batch.run(&[
        &tool("install"),
        "-d",
        "-o",
        "root",
        "-g",
        "root",
        "-m",
        "0755",
        INSTALL_DIR,
    ]);
    let (uid_new, uid_file) = (at(".uid.new"), at(UID_FILE));
    batch
        .run(&[
            "/bin/sh",
            "-c",
            "printf '%s\\n' \"$1\" > \"$2\"",
            "sh",
            &uid.to_string(),
            &uid_new,
        ])
        .run(&[&tool("chmod"), "0644", &uid_new])
        .run(&[&tool("mv"), "-f", &uid_new, &uid_file]);
    for (source, name, caps) in [(hub, HUB, false), (launcher, LAUNCHER, true)] {
        let new = at(&format!(".{name}.new"));
        batch
            .run(&[&tool("rm"), "-f", &new])
            .run(&[&tool("cp"), source, &new])
            .run(&[&tool("chown"), "root:root", &new])
            .run(&[&tool("chmod"), "0755", &new]);
        if caps {
            batch.run(&[&tool("setcap"), "cap_net_admin,cap_net_raw+eip", &new]);
        }
        batch.run(&[&tool("mv"), "-f", &new, &at(name)]);
    }
    batch
}

/// Grants the capabilities for this user to root's copies of `hub` and the launcher next to it
/// (one sudo -n/pkexec); the hub gets them at its next start. Again after an update.
#[cfg(target_os = "linux")]
pub fn grant(hub: &Path) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    use anyhow::Context as _;
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let mut open = Vec::new();
    for exe in [hub.to_path_buf(), hub.with_file_name(LAUNCHER)] {
        let text = exe.display().to_string();
        if let Some(why) = refuse(&text, true, uid, uid) {
            anyhow::bail!(why);
        }
        // No link followed; root copies this open file, not the path.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&exe)
            .with_context(|| format!("{text} (it must be a regular file, not a link)"))?;
        let meta = file.metadata()?;
        if let Some(why) = refuse(&text, meta.is_file(), meta.uid(), uid) {
            anyhow::bail!(why);
        }
        open.push(file);
    }
    let fd = |f: &std::fs::File| format!("/proc/{}/fd/{}", std::process::id(), f.as_raw_fd());
    grant_batch(&fd(&open[0]), &fd(&open[1]), uid)
        .exec()
        .with_context(|| format!("cannot grant routing capabilities in {INSTALL_DIR}"))
}

/// `UID_FILE` when root's alone, and the uid in it.
#[cfg(target_os = "linux")]
fn allowed_uid() -> anyhow::Result<u32> {
    use std::io::Read as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    let dir = std::fs::symlink_metadata(INSTALL_DIR)?;
    if let Some(why) = refuse_root_file(dir.is_dir(), dir.uid(), dir.mode()) {
        anyhow::bail!("{INSTALL_DIR} {why}");
    }
    let path = Path::new(INSTALL_DIR).join(UID_FILE);
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    let meta = file.metadata()?;
    if let Some(why) = refuse_root_file(meta.is_file(), meta.uid(), meta.mode()) {
        anyhow::bail!("{} {why}", path.display());
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    parse_uid(&text).ok_or_else(|| anyhow::anyhow!("{} holds no uid", path.display()))
}

/// Both files hold the same bytes (or are one).
#[cfg(target_os = "linux")]
fn same_program(a: &Path, b: &Path) -> bool {
    let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    a == b
        || (len(a).is_some() && len(a) == len(b) && {
            matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
        })
}

/// How the grant stands for the hub program `exe` and this user.
#[cfg(target_os = "linux")]
pub fn installed(exe: &Path) -> Installed {
    let dir = Path::new(INSTALL_DIR);
    if !file_grants(&dir.join(LAUNCHER)) {
        return Installed::None;
    }
    // SAFETY: getuid cannot fail.
    if allowed_uid().ok() != Some(unsafe { libc::getuid() }) {
        return Installed::OtherUser;
    }
    if !same_program(exe, &dir.join(HUB)) {
        return Installed::Stale;
    }
    Installed::Ready
}

/// The hub program as the user started it: the launcher runs the installed copy and names the
/// original in `ORIGIN_ENV` (autostart, the next grant).
#[cfg(target_os = "linux")]
pub fn hub_exe() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    match std::env::var_os(ORIGIN_ENV) {
        Some(origin) if exe.starts_with(INSTALL_DIR) => Ok(PathBuf::from(origin)),
        _ => Ok(exe),
    }
}

/// The launcher's step, in the installed `zapret-hub-cli`: for the user `UID_FILE` names only,
/// the installed hub, opened without following a link and checked on that open file, replaces
/// this process with the hub's flags of `args`, the session's environment (`kept_env`) and the
/// capabilities as ambient ones. Otherwise the capabilities go and the hub the user started (the
/// first of `args`) starts without them, saying why on stderr. Returns only when no hub started.
#[cfg(target_os = "linux")]
pub fn launch(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Error {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::unix::process::CommandExt as _;

    let mut args = args.into_iter();
    let origin = args.next();
    let flags: Vec<_> = args.filter(|a| HUB_FLAGS.iter().any(|f| a == f)).collect();
    let run = || -> anyhow::Result<std::convert::Infallible> {
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let allowed = allowed_uid()?;
        anyhow::ensure!(
            allowed == uid,
            "routing capabilities on this computer are granted to uid {allowed}, not to you \
             (uid {uid}): click «Разрешить маршрутизацию» in your Zapret Hub"
        );
        let hub = Path::new(INSTALL_DIR).join(HUB);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&hub)
            .map_err(|e| anyhow::anyhow!("{}: {e}", hub.display()))?;
        let meta = file.metadata()?;
        if let Some(why) = refuse_root_file(meta.is_file(), meta.uid(), meta.mode()) {
            anyhow::bail!("{} {why}", hub.display());
        }
        // The file checked, not whatever the path names by now.
        let mut cmd = std::process::Command::new(format!("/proc/self/fd/{}", file.as_raw_fd()));
        cmd.arg0(&hub).args(&flags).env_clear();
        cmd.envs(std::env::vars_os().filter(|(k, _)| k.to_str().is_some_and(kept_env)));
        cmd.env(LAUNCHED_ENV, "1");
        if let Some(origin) = &origin {
            cmd.env(ORIGIN_ENV, origin);
        }
        Err(inherit(&mut cmd).exec().into())
    };
    let Err(why) = run();
    eprintln!("Zapret Hub starts without the routing capabilities: {why:#}");
    if let Err(err) = drop_all() {
        return err.into();
    }
    let Some(origin) = origin else {
        return why;
    };
    let err = std::process::Command::new(&origin)
        .args(&flags)
        .env(LAUNCHED_ENV, "1")
        .exec();
    anyhow::Error::new(err).context(format!("{} did not start", origin.display()))
}

/// A hub started without the capabilities whose program is the installed one, for this user
/// (`Installed::Ready`): replaces this process with the installed launcher, which starts the hub
/// again with them. `launched`: this hub is the launcher's already. The launcher's error, or
/// `None` when there was nothing to do.
#[cfg(target_os = "linux")]
pub fn relaunch(launched: bool) -> Option<std::io::Error> {
    use std::os::unix::process::CommandExt as _;
    if launched || present() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    (installed(&exe) == Installed::Ready).then(|| {
        std::process::Command::new(Path::new(INSTALL_DIR).join(LAUNCHER))
            .arg(LAUNCH_ARG)
            .arg(&exe)
            .args(std::env::args_os().skip(1))
            .exec()
    })
}

/// The hub keeps its capabilities to itself: what it starts (a browser, a file manager, the web
/// view's processes) gets none, neither ambient nor inheritable; `inherit` hands them to `nft`
/// and `ip` only.
#[cfg(target_os = "linux")]
pub fn keep_from_children() {
    let mut header = Header {
        version: VERSION_3,
        pid: 0,
    };
    let mut data = [Data {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: a plain prctl, then capget/capset on our stack memory of the sizes they use.
    unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
        if libc::syscall(libc::SYS_capget, &raw mut header, data.as_mut_ptr()) == 0 {
            data[0].inheritable = 0;
            data[1].inheritable = 0;
            libc::syscall(libc::SYS_capset, &raw const header, data.as_ptr());
        }
    }
}

/// The CLI's own commands need none of the launcher's file capabilities: they go first thing.
#[cfg(target_os = "linux")]
pub fn drop_all() -> std::io::Result<()> {
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
    if unsafe { libc::syscall(libc::SYS_capset, &raw const header, data.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
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
        assert_eq!(refuse(exe, true, 1000, 1000), None);
        assert_eq!(refuse(exe, true, 0, 1000), None, "root's");
        assert!(
            refuse(exe, true, 1001, 1000)
                .unwrap()
                .contains("another user")
        );
        assert!(
            refuse(exe, false, 1000, 1000)
                .unwrap()
                .contains("regular file")
        );
        let updated = refuse("/home/u/zh/zapret-hub (deleted)", true, 1000, 1000).unwrap();
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
    fn only_roots_own_installed_files_are_trusted() {
        assert_eq!(refuse_root_file(true, 0, 0o100_755), None);
        assert_eq!(refuse_root_file(true, 0, 0o040_755), None);
        for (ok, owner, mode) in [
            (false, 0, 0o755),
            (true, 1000, 0o755),
            (true, 0, 0o775),
            (true, 0, 0o757),
            (true, 0, 0o1777),
        ] {
            assert!(
                refuse_root_file(ok, owner, mode).is_some(),
                "{ok} {owner} {mode:o}"
            );
        }
        assert_eq!(parse_uid("1000\n"), Some(1000));
        assert_eq!(parse_uid("x"), None);
        assert_eq!(parse_uid(""), None);
    }

    #[test]
    fn the_hub_gets_the_session_and_nothing_that_loads_code() {
        for kept in [
            "DISPLAY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
            "LC_ALL",
            "HOME",
        ] {
            assert!(kept_env(kept), "{kept}");
        }
        for dropped in [
            "GTK_MODULES",
            "GTK_IM_MODULE_FILE",
            "GTK_EXE_PREFIX",
            "GIO_EXTRA_MODULES",
            "LIBGL_DRIVERS_PATH",
            "__EGL_VENDOR_LIBRARY_DIRS",
            "VK_ICD_FILENAMES",
            "LD_PRELOAD",
            "SUDO_UID",
        ] {
            assert!(!kept_env(dropped), "{dropped}");
        }
    }

    #[test]
    fn each_grant_state_says_what_to_do() {
        assert_eq!(hint(Installed::None), HINT);
        assert!(hint(Installed::OtherUser).contains("another user"));
        assert!(hint(Installed::Stale).contains("again"));
        assert!(hint(Installed::Ready).contains("start it again"));
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
    fn grant_copies_the_open_files_into_roots_folder_then_gives_the_launcher_the_caps() {
        let script = grant_batch("/proc/7/fd/3", "/proc/7/fd/4", 1000).script();
        let lines: Vec<&str> = script.lines().collect();
        let ends = [
            "set -e",
            "install' '-d' '-o' 'root' '-g' 'root' '-m' '0755' '/usr/local/lib/zapret-hub'",
            "'sh' '1000' '/usr/local/lib/zapret-hub/.uid.new'",
            "chmod' '0644' '/usr/local/lib/zapret-hub/.uid.new'",
            "mv' '-f' '/usr/local/lib/zapret-hub/.uid.new' '/usr/local/lib/zapret-hub/uid'",
            "rm' '-f' '/usr/local/lib/zapret-hub/.zapret-hub.new'",
            "cp' '/proc/7/fd/3' '/usr/local/lib/zapret-hub/.zapret-hub.new'",
            "chown' 'root:root' '/usr/local/lib/zapret-hub/.zapret-hub.new'",
            "chmod' '0755' '/usr/local/lib/zapret-hub/.zapret-hub.new'",
            "mv' '-f' '/usr/local/lib/zapret-hub/.zapret-hub.new' '/usr/local/lib/zapret-hub/zapret-hub'",
            "rm' '-f' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new'",
            "cp' '/proc/7/fd/4' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new'",
            "chown' 'root:root' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new'",
            "chmod' '0755' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new'",
            "setcap' 'cap_net_admin,cap_net_raw+eip' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new'",
            "mv' '-f' '/usr/local/lib/zapret-hub/.zapret-hub-cli.new' '/usr/local/lib/zapret-hub/zapret-hub-cli'",
        ];
        assert_eq!(lines.len(), ends.len(), "{script}");
        for (line, end) in lines.iter().zip(ends) {
            assert!(line.ends_with(end), "{line}\n{script}");
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

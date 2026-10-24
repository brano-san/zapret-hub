//! The `zapret-hub` block in the hosts file and when it needs refreshing (`:hosts_update`).

use std::path::{Path, PathBuf};

use anyhow::Context as _;

pub const BEGIN: &str = "# zapret-hub begin";
pub const END: &str = "# zapret-hub end";

/// The hosts file's line break: CRLF on Windows, LF on Linux (a CR there would end up in a name).
#[cfg(windows)]
pub const NL: &str = "\r\n";
#[cfg(target_os = "linux")]
pub const NL: &str = "\n";

#[cfg(target_os = "linux")]
pub fn path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

#[cfg(windows)]
pub fn path() -> PathBuf {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windows
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

/// Up to date when the first and the last non-empty line of Flowseal's file are in hosts.
pub fn needs_update(current: &str, flowseal: &str) -> bool {
    let lines: Vec<&str> = flowseal
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
        return false;
    };
    !current.contains(first) || !current.contains(last)
}

/// The lines of `current` outside the zapret-hub block, and the lines inside it. A block is a
/// begin marker closed by an end marker. Any other marker is a stray line and goes alone: a begin
/// that lost its end (hosts is edited by hand) must not eat the lines after it.
fn split(current: &str) -> (Vec<&str>, Vec<&str>) {
    let (mut outside, mut block, mut pending) = (Vec::new(), Vec::new(), Vec::new());
    let mut open = false;
    for line in current.lines() {
        match line.trim() {
            // A begin that is still open was never closed: what it collected is ordinary lines.
            BEGIN => {
                outside.append(&mut pending);
                open = true;
            }
            END => {
                block.append(&mut pending);
                open = false;
            }
            _ if open => pending.push(line),
            _ => outside.push(line),
        }
    }
    outside.append(&mut pending);
    (outside, block)
}

/// `current` with its zapret-hub block replaced by `block`, or the block appended. A begin marker
/// without an end marker is dropped on its own; every line after it stays.
pub fn merge(current: &str, block: &str) -> String {
    let (mut kept, _) = split(current);
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join(NL);
    if !out.is_empty() {
        out.push_str(NL);
        out.push_str(NL);
    }
    out.push_str(BEGIN);
    out.push_str(NL);
    for line in block.lines() {
        out.push_str(line);
        out.push_str(NL);
    }
    out.push_str(END);
    out.push_str(NL);
    out
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HostsPlan {
    pub needs_update: bool,
    pub block: String,
    /// Lines of Flowseal's file that hosts does not have yet.
    pub added: Vec<String>,
    /// Lines of the current zapret-hub block that Flowseal no longer has.
    pub removed: Vec<String>,
}

pub fn plan(current: &str, flowseal: &str) -> HostsPlan {
    let (_, old_block) = split(current);
    let added = flowseal
        .lines()
        .filter(|line| !line.trim().is_empty() && !current.lines().any(|c| c == *line))
        .map(str::to_owned)
        .collect();
    let removed = old_block
        .into_iter()
        .filter(|line| !line.trim().is_empty() && !flowseal.lines().any(|f| f == *line))
        .map(str::to_owned)
        .collect();
    HostsPlan {
        needs_update: needs_update(current, flowseal),
        block: flowseal.to_owned(),
        added,
        removed,
    }
}

/// The hosts file; a missing one is empty. A file that is not UTF-8 is refused, not rewritten:
/// a lossy round trip would mangle its comments.
pub fn read() -> anyhow::Result<String> {
    read_at(&path())
}

fn read_at(path: &Path) -> anyhow::Result<String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            anyhow::anyhow!(
                "{} is not UTF-8 text: edit it by hand, nothing was changed",
                path.display()
            )
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
    }
}

/// Writes `block` into hosts (as root on Linux); the text it replaces is kept as
/// `hosts.zapret-hub.bak`. The DNS cache is flushed after; a flush that fails is only a warning.
pub fn apply(block: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    apply_at(&path(), block, plain_write)?;
    #[cfg(target_os = "linux")]
    apply_at(&path(), block, crate::root::write)?;
    if let Err(err) = flush_dns() {
        log::warn!(target: "exec", "the DNS cache was not flushed: {err:#}");
    }
    Ok(())
}

/// `ipconfig /flushdns`.
#[cfg(windows)]
pub fn flush_dns() -> anyhow::Result<()> {
    exec::run(
        std::process::Command::new("ipconfig").arg("/flushdns"),
        std::time::Duration::from_secs(15),
        None,
    )?
    .check()?;
    Ok(())
}

/// What may cache lookups on Linux, in the order tried: systemd-resolved (two spellings),
/// NetworkManager, nscd, dnsmasq. The first that succeeds is enough.
#[cfg(target_os = "linux")]
pub const FLUSHERS: [(&str, &[&str]); 5] = [
    ("resolvectl", &["flush-caches"]),
    ("systemd-resolve", &["--flush-caches"]),
    ("nmcli", &["general", "reload", "dns-full"]),
    ("nscd", &["-i", "hosts"]),
    ("pkill", &["-HUP", "dnsmasq"]),
];

/// Each of `FLUSHERS` that is installed, as the user and then as root; nscd and dnsmasq only
/// while they run. Nothing worked and `/etc/resolv.conf` points at no local stub: there is no
/// cache to flush, which is success too.
#[cfg(target_os = "linux")]
pub fn flush_dns() -> anyhow::Result<()> {
    use std::time::Duration;
    let mut tried = Vec::new();
    for (prog, args) in FLUSHERS {
        let daemon = match prog {
            "nscd" => Some("nscd"),
            "pkill" => Some("dnsmasq"),
            _ => None,
        };
        if daemon.is_some_and(|daemon| !crate::procs::is_running(daemon)) {
            continue;
        }
        let mut cmd = std::process::Command::new(prog);
        cmd.args(args);
        match exec::run(&mut cmd, Duration::from_secs(15), None) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(out) if out.success() => return Ok(()),
            _ => {}
        }
        match crate::root::run(prog, args) {
            Ok(out) if out.success() => return Ok(()),
            Ok(out) => tried.push(out.describe()),
            Err(err) => tried.push(format!("{err:#}")),
        }
    }
    let stub = std::fs::read_to_string("/etc/resolv.conf").is_ok_and(|text| local_stub(&text));
    anyhow::ensure!(
        !stub,
        "the DNS cache was not flushed: none of systemd-resolved, NetworkManager, nscd, dnsmasq \
         answered\n\n{}",
        tried.join("\n\n")
    );
    Ok(())
}

/// `/etc/resolv.conf` sends lookups to a local stub (127.0.0.53 of systemd-resolved, 127.0.0.1
/// of dnsmasq): something caches them.
#[cfg(target_os = "linux")]
fn local_stub(resolv_conf: &str) -> bool {
    resolv_conf.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("nameserver") && words.next().is_some_and(|ip| ip.starts_with("127."))
    })
}

/// A plain file write: Windows, and the tests.
#[cfg(any(windows, test))]
fn plain_write(path: &Path, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Flowseal's hosts file for Discord and the rest: the block the hub keeps in hosts.
pub fn flowseal_url() -> String {
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/hosts"
        .to_owned()
}

fn flowseal_hosts(socks5: Option<&str>) -> anyhow::Result<String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    probe::curl::fetch_text(&format!("{}?t={nonce}", flowseal_url()), socks5)
}

/// What applying Flowseal's hosts would change.
pub fn download_plan(socks5: Option<&str>) -> anyhow::Result<HostsPlan> {
    let flowseal = flowseal_hosts(socks5)?;
    Ok(plan(&read()?, &flowseal))
}

/// Downloads Flowseal's hosts again and writes it as the zapret-hub block.
pub fn download_and_apply(socks5: Option<&str>) -> anyhow::Result<()> {
    apply(&flowseal_hosts(socks5)?)
}

/// `apply` on the hosts file at `path` through `write`, without the DNS flush (tests run it on a
/// temp file).
fn apply_at(
    path: &Path,
    block: &str,
    write: impl Fn(&Path, &str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let current = read_at(path)?;
    if path.is_file() {
        // Written from the text just read, not copied: a copy of the file takes over its read-only
        // attribute, and the next backup would then fail on the read-only copy.
        let bak = path.with_file_name("hosts.zapret-hub.bak");
        write(&bak, &current).context("cannot write the backup")?;
    }
    write(path, &merge(&current, block))?;
    log::info!(target: "exec", "hosts: zapret-hub block written, {} lines", block.lines().count());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const OLD: &str = "127.0.0.1 localhost\r\n";

    fn apply_at(path: &Path, block: &str) -> anyhow::Result<()> {
        super::apply_at(path, block, plain_write)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_loopback_nameserver_is_a_local_stub() {
        assert!(local_stub(
            "# generated\nnameserver 127.0.0.53\noptions edns0\n"
        ));
        assert!(!local_stub("#nameserver 127.0.0.1\nnameserver 1.1.1.1\n"));
        assert!(!local_stub(""));
    }

    /// Windows only, so the Unix caveat of the lint (a world-writable file) does not apply.
    #[allow(clippy::permissions_set_readonly_false)]
    fn set_read_only(path: &Path, read_only: bool) {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(read_only);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn is_read_only(path: &Path) -> bool {
        fs::metadata(path).unwrap().permissions().readonly()
    }

    /// A temp folder with a `hosts` file holding `OLD`, and where its backup goes.
    fn hosts_in_temp() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let hosts = dir.path().join("hosts");
        fs::write(&hosts, OLD).unwrap();
        let bak = dir.path().join("hosts.zapret-hub.bak");
        (dir, hosts, bak)
    }

    #[test]
    fn apply_at_writes_the_block_and_backs_up_the_text_it_read() {
        let (_dir, hosts, bak) = hosts_in_temp();
        apply_at(&hosts, "1.2.3.4 a.com").unwrap();
        assert_eq!(
            fs::read_to_string(&hosts).unwrap(),
            merge(OLD, "1.2.3.4 a.com")
        );
        assert_eq!(fs::read_to_string(&bak).unwrap(), OLD);
    }

    #[test]
    fn a_read_only_hosts_does_not_block_the_next_apply() {
        let (_dir, hosts, bak) = hosts_in_temp();
        set_read_only(&hosts, true);
        // Writing into the protected file fails, but the backup it left must be a plain file...
        assert!(apply_at(&hosts, "1.2.3.4 a.com").is_err());
        assert!(
            !is_read_only(&bak),
            "the backup took over the read-only attribute of hosts"
        );
        // ...so that the retry, after the user lifted the protection, goes through.
        set_read_only(&hosts, false);
        apply_at(&hosts, "1.2.3.4 a.com").unwrap();
        assert_eq!(
            fs::read_to_string(&hosts).unwrap(),
            merge(OLD, "1.2.3.4 a.com")
        );
        assert_eq!(fs::read_to_string(&bak).unwrap(), OLD);
    }

    #[test]
    fn write_errors_name_the_file() {
        let (_dir, hosts, bak) = hosts_in_temp();
        set_read_only(&hosts, true);
        let err = format!("{:#}", apply_at(&hosts, "x").unwrap_err());
        assert!(err.contains(&hosts.display().to_string()), "{err}");
        assert!(!err.contains("zapret-hub.bak"), "{err}");
        set_read_only(&hosts, false);

        // A backup that is read-only (an older build left one, or the user set it) is named too,
        // and hosts is not touched while its backup is missing.
        fs::write(&bak, "older backup").unwrap();
        set_read_only(&bak, true);
        let err = format!("{:#}", apply_at(&hosts, "x").unwrap_err());
        assert!(err.contains(&bak.display().to_string()), "{err}");
        assert_eq!(fs::read_to_string(&hosts).unwrap(), OLD);
        set_read_only(&bak, false);
    }

    #[test]
    fn a_missing_hosts_is_created_without_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let hosts = dir.path().join("hosts");
        apply_at(&hosts, "1.2.3.4 a.com").unwrap();
        assert_eq!(
            fs::read_to_string(&hosts).unwrap(),
            merge("", "1.2.3.4 a.com")
        );
        assert!(!dir.path().join("hosts.zapret-hub.bak").exists());
    }
}

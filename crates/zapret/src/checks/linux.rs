//! Linux: the fork's `doctor`, as checks. Reading the queue and the nftables tables needs root:
//! they are read directly or through `sudo -n` only (opening the window must not raise a polkit
//! dialog), and pass when neither works rather than raise a false alarm.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    CheckAction, CheckId, CheckLevel, SystemCheck, check, conflicting_services, found,
    hosts_youtube, tcp_timestamps, vpn,
};
use crate::procs::queue;
use crate::service::{self, NFT_TABLE, SERVICE2, SvcState};

/// Where a zapret2 folder keeps `nfqws2`: the release's `binaries/linux-x86_64`, a build's root
/// or `bin`.
const NFQWS_DIRS: [&str; 4] = ["binaries/linux-x86_64", "binaries/x86_64", "bin", ""];
/// The lists every built-in strategy reads (`%LISTS%`).
const LISTS: [&str; 2] = ["list-general.txt", "list-google.txt"];
/// What `nfqws2`'s own packets carry and the table lets pass (NFQWS2_COMPAT).
const FWMARK: &str = "0x40000000";
/// Other engines that would take packets from a queue too.
const ENGINES: [&str; 3] = ["nfqws2", "nfqws", "tpws"];

/// `zapret2` is the zapret2 folder setting: the checks of its files pass without one.
pub fn run_all(zapret2: Option<&Path>) -> Vec<SystemCheck> {
    use CheckLevel::{Problem, Warn};
    let services = service::active_services();
    let status = service::status(SERVICE2);
    let running = matches!(status, Some((SvcState::Running, _)));
    let nfqws = zapret2.and_then(find_nfqws);
    let rights = nfqws.as_deref().map(rights);
    let table = running.then(table_text).flatten();
    let queue = running.then(queue::text).flatten();
    let foreign = foreign_engines(status.map_or(0, |(_, pid)| pid));
    vec![
        check(
            CheckId::ServiceState,
            status.is_some_and(|(state, _)| state != SvcState::Running),
            Problem,
            status
                .map(|(state, _)| format!("{state:?}"))
                .unwrap_or_default(),
            None,
        ),
        check(
            CheckId::Nfqws2Binary,
            zapret2.is_some() && nfqws.is_none(),
            Problem,
            zapret2
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
            None,
        ),
        check(
            CheckId::Nfqws2Caps,
            rights.is_some_and(|(exec, caps)| !(exec && caps)),
            Warn,
            nfqws
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            Some(CheckAction::GrantCapabilities),
        ),
        found(
            CheckId::FakeBins,
            zapret2
                .map(|dir| empty_bins(&dir.join("bin")).join(", "))
                .unwrap_or_default(),
            Warn,
        ),
        found(
            CheckId::Lists,
            zapret2
                .map(|dir| missing_lists(&dir.join("lists")).join(", "))
                .unwrap_or_default(),
            Warn,
        ),
        check(
            CheckId::NfQueue,
            queue.is_some_and(|text| {
                queue::parse(&text, queue::QNUM).is_none_or(|(portid, _)| portid == 0)
            }),
            Problem,
            queue::QNUM.to_string(),
            None,
        ),
        found(
            CheckId::NftTable,
            table
                .as_deref()
                .map(table_gaps)
                .unwrap_or_default()
                .join(", "),
            Problem,
        ),
        tcp_timestamps(),
        vpn(&services),
        hosts_youtube(),
        found(CheckId::ForeignEngines, foreign.join(", "), Problem),
        found(
            CheckId::ForeignTables,
            tables_text()
                .as_deref()
                .map(foreign_tables)
                .unwrap_or_default()
                .join(", "),
            Problem,
        ),
        conflicting_services(),
    ]
}

/// `nfqws2` under `dir`, as `zapret2::Engine` finds it.
fn find_nfqws(dir: &Path) -> Option<PathBuf> {
    NFQWS_DIRS
        .iter()
        .map(|sub| dir.join(sub).join("nfqws2"))
        .find(|path| path.is_file())
}

/// The exec bits, and both network capabilities in `getcap`'s answer.
fn rights(exe: &Path) -> (bool, bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let executable =
        std::fs::metadata(exe).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0);
    let getcap = exec::run(
        crate::root::c_locale(&mut std::process::Command::new(crate::root::tool("getcap")))
            .arg(exe),
        Duration::from_secs(10),
        None,
    )
    .map(|out| out.stdout)
    .unwrap_or_default();
    (executable, has_caps(&getcap))
}

/// `getcap` prints `<path> cap_net_admin,cap_net_raw=eip` (older: `= cap_net_admin,cap_net_raw+eip`).
fn has_caps(getcap: &str) -> bool {
    let text = getcap.to_ascii_lowercase();
    text.contains("cap_net_admin") && text.contains("cap_net_raw")
}

/// The button: `chmod 755` as the owner, then `setcap` as root.
pub(super) fn grant_capabilities(zapret2: Option<&Path>) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let exe = zapret2
        .and_then(find_nfqws)
        .ok_or_else(|| anyhow::anyhow!("there is no nfqws2 in the zapret2 folder"))?;
    let shown = exe.display().to_string();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
        .map_err(|err| anyhow::anyhow!("chmod 755 {shown}: {err}"))?;
    crate::root::check("setcap", &["cap_net_admin,cap_net_raw=eip", &shown])
}

/// Every empty `.bin` in the `bin` folder, by name.
fn empty_bins(bin: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(bin)
        .map(|entries| entries.flatten().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() == 0)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".bin"))
        .collect();
    names.sort();
    names
}

/// Each of `LISTS` missing from `lists` or holding no entry (only blanks and `#` comments).
fn missing_lists(lists: &Path) -> Vec<String> {
    LISTS
        .iter()
        .filter(|name| {
            std::fs::read_to_string(lists.join(name)).map_or(true, |text| {
                !text.lines().any(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with('#')
                })
            })
        })
        .map(|name| (*name).to_owned())
        .collect()
}

/// `nft list table inet zapret_hub`, as the user or through `sudo -n`; `None` when unreadable.
/// An absent table is an empty text: `nft` fails for it, and the failure is the answer.
fn table_text() -> Option<String> {
    let args = ["list", "table", "inet", "zapret_hub"];
    debug_assert_eq!(format!("{} {}", args[2], args[3]), NFT_TABLE);
    nft(&args).map(|out| {
        if out.success() {
            out.stdout
        } else {
            String::new()
        }
    })
}

/// `nft list tables`; `None` when unreadable.
fn tables_text() -> Option<String> {
    nft(&["list", "tables"])
        .filter(exec::Output::success)
        .map(|out| out.stdout)
}

/// `nft` as the user, else as root without asking. `None` when neither may read the ruleset (or
/// there is no `nft`): an `Operation not permitted` is not an answer about the table.
fn nft(args: &[&str]) -> Option<exec::Output> {
    let mut cmd = std::process::Command::new(crate::root::tool("nft"));
    crate::root::c_locale(&mut cmd).args(args);
    match exec::run(&mut cmd, Duration::from_secs(10), None) {
        Ok(out) if !denied(&out) => Some(out),
        Ok(_) => crate::root::run_unprompted("nft", args).filter(|out| !denied(out)),
        Err(_) => None,
    }
}

fn denied(out: &exec::Output) -> bool {
    !out.success() && out.stderr.contains("Operation not permitted")
}

/// What the running table lacks: itself, the loopback skip, the engine's own mark.
fn table_gaps(text: &str) -> Vec<&'static str> {
    if text.trim().is_empty() {
        return vec!["inet zapret_hub"];
    }
    let mut gaps = Vec::new();
    if !text.contains(r#"oifname "lo""#) {
        gaps.push(r#"oifname "lo" accept"#);
    }
    if !text.contains(FWMARK) {
        gaps.push("meta mark 0x40000000");
    }
    gaps
}

/// Tables with `zapret` in the name that are not the hub's: `table inet zapretunix`.
fn foreign_tables(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.contains("zapret") && !line.ends_with("zapret_hub"))
        .map(|line| line.trim_start_matches("table ").to_owned())
        .collect()
}

/// Engine processes the service does not own (nor this app): `name (pid)`.
fn foreign_engines(service_pid: u32) -> Vec<String> {
    ENGINES
        .iter()
        .flat_map(|image| {
            crate::procs::find(image)
                .into_iter()
                .map(move |p| (image, p))
        })
        .filter(|(_, proc)| proc.pid != service_pid && proc.pid != std::process::id())
        .map(|(image, proc)| format!("{image} ({})", proc.pid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getcap_answers_in_both_spellings() {
        assert!(has_caps("/z/nfqws2 cap_net_admin,cap_net_raw=eip\n"));
        assert!(has_caps("/z/nfqws2 = cap_net_admin,cap_net_raw+eip\n"));
        assert!(!has_caps("/z/nfqws2 cap_net_raw=ep\n"));
        assert!(!has_caps(""));
    }

    #[test]
    fn the_table_needs_its_two_guards() {
        let full = "table inet zapret_hub {\n chain output {\n  meta mark & 0x40000000 != 0 return\n  oifname \"lo\" accept\n }\n}\n";
        assert!(table_gaps(full).is_empty());
        assert_eq!(table_gaps(""), ["inet zapret_hub"]);
        assert_eq!(
            table_gaps("table inet zapret_hub {\n}\n"),
            [r#"oifname "lo" accept"#, "meta mark 0x40000000"]
        );
    }

    #[test]
    fn only_other_zapret_tables_are_foreign() {
        let text = "table inet filter\ntable inet zapret_hub\ntable inet zapretunix\n";
        assert_eq!(foreign_tables(text), ["inet zapretunix"]);
        assert!(foreign_tables("").is_empty());
    }

    #[test]
    fn empty_fakes_and_missing_lists_are_named() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("stun.bin"), "x").unwrap();
        std::fs::write(bin.join("tls.bin"), "").unwrap();
        std::fs::write(bin.join("notes.txt"), "").unwrap();
        assert_eq!(empty_bins(&bin), ["tls.bin"]);
        assert!(empty_bins(&tmp.path().join("nope")).is_empty());

        let lists = tmp.path().join("lists");
        std::fs::create_dir_all(&lists).unwrap();
        std::fs::write(lists.join("list-general.txt"), "# only a comment\n\n").unwrap();
        assert_eq!(missing_lists(&lists), LISTS);
        std::fs::write(lists.join("list-general.txt"), "discord.com\n").unwrap();
        std::fs::write(lists.join("list-google.txt"), "youtube.com\n").unwrap();
        assert!(missing_lists(&lists).is_empty());
    }

    #[test]
    fn nfqws_is_found_where_a_release_keeps_it() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(find_nfqws(tmp.path()), None);
        let exe = tmp.path().join("binaries/linux-x86_64/nfqws2");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        assert_eq!(find_nfqws(tmp.path()), Some(exe));
    }
}

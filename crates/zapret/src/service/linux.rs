//! Linux: state through `systemctl`, the unit file under `/etc/systemd/system` written with root
//! rights (`crate::root`). The `zapret2` unit runs the engine as this user with the network
//! capabilities its queue needs, and brings its nftables table up and down as root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Context as _;

use super::{SERVICE, SERVICE2, SvcConfig, SvcState, WINWS2, state, stop_and_delete};

/// The nftables table file the `zapret2` unit applies before the engine starts
/// (`zapret2::nft` writes it).
pub const NFT_CONFIG: &str = "/etc/zapret-hub/nfqws2.nft";
/// The table `NFT_CONFIG` makes, deleted when the engine stops.
pub const NFT_TABLE: &str = "inet zapret_hub";

const DESCRIPTION: &str = "zapret2 DPI bypass software";

fn cmd(program: &str, args: &[&str]) -> anyhow::Result<exec::Output> {
    Ok(exec::run(
        Command::new(program).args(args),
        Duration::from_secs(60),
        None,
    )?)
}

/// State and main pid; `None` when systemd has no such unit.
pub fn status(name: &str) -> Option<(SvcState, u32)> {
    let out = cmd(
        "systemctl",
        &[
            "show",
            &unit_name(name),
            "--property=LoadState,ActiveState,MainPID",
        ],
    )
    .ok()?;
    parse_show(&out.stdout)
}

fn unit_name(name: &str) -> String {
    format!("{name}.service")
}

/// `systemctl show` lines `Key=value`: a `LoadState` of `not-found` is no unit at all.
fn parse_show(text: &str) -> Option<(SvcState, u32)> {
    let props: BTreeMap<&str, &str> = text
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .collect();
    if matches!(props.get("LoadState"), None | Some(&"not-found")) {
        return None;
    }
    let state = match props.get("ActiveState").copied().unwrap_or_default() {
        "active" | "reloading" => SvcState::Running,
        "activating" => SvcState::StartPending,
        "deactivating" => SvcState::StopPending,
        "inactive" | "failed" => SvcState::Stopped,
        _ => SvcState::Other,
    };
    let pid = props.get("MainPID").and_then(|pid| pid.parse().ok());
    Some((state, pid.unwrap_or(0)))
}

/// `ExecStart` of the unit as `systemctl cat` shows it (drop-ins included), and whether it starts
/// at boot. `None` when the unit or its `ExecStart` is missing.
pub fn config(name: &str) -> Option<SvcConfig> {
    let unit = cmd("systemctl", &["cat", &unit_name(name)])
        .ok()?
        .check()
        .ok()?
        .stdout;
    Some(SvcConfig {
        image_path: exec_start(&unit)?,
        auto_start: cmd("systemctl", &["is-enabled", &unit_name(name)])
            .is_ok_and(|out| out.success()),
    })
}

/// The last `ExecStart=` (a drop-in's replaces the unit's), without systemd's `-+!@:` prefixes
/// and with `%%` back to `%`, as `unit` wrote it.
fn exec_start(unit: &str) -> Option<String> {
    unit.lines()
        .filter_map(|line| line.trim().strip_prefix("ExecStart="))
        .rfind(|value| !value.is_empty())
        .map(|value| {
            value
                .trim_start_matches(['-', '+', '!', '@', ':'])
                .replace("%%", "%")
        })
}

/// Name (without `.service`) and description of every running service: what the checks search.
pub fn active_services() -> Vec<(String, String)> {
    let args = [
        "list-units",
        "--type=service",
        "--state=running",
        "--no-legend",
        "--plain",
    ];
    cmd("systemctl", &args)
        .map(|out| parse_units(&out.stdout))
        .unwrap_or_default()
}

/// `name.service loaded active running Description words` lines.
fn parse_units(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?.strip_suffix(".service")?.to_owned();
            Some((name, fields.skip(3).collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

/// `$XDG_DATA_HOME/zapret-hub/services.json`: service -> the strategy the app installed in it.
/// The user's file, not the unit's: recording a name needs no root rights.
fn markers_path() -> PathBuf {
    crate::data_home().join("zapret-hub").join("services.json")
}

fn markers() -> BTreeMap<String, String> {
    std::fs::read_to_string(markers_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The strategy name the app recorded for service `name`: `zapret2:<name>` (zapret2 spec 6).
pub fn strategy_of(name: &str) -> Option<String> {
    markers().remove(name)
}

pub fn set_strategy_of(service: &str, name: &str) -> anyhow::Result<()> {
    let path = markers_path();
    let mut map = markers();
    map.insert(service.to_owned(), name.to_owned());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&map)?)
        .with_context(|| format!("cannot write {}", path.display()))?;
    log::info!(target: "exec", "{}: {service} = {name}", path.display());
    Ok(())
}

fn unit_path(name: &str) -> PathBuf {
    Path::new("/etc/systemd/system").join(unit_name(name))
}

/// What the `zapret2` unit adds around its engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineUnit {
    /// The uid the engine runs as; `None` runs it as root.
    pub user: Option<u32>,
    /// The absolute `nft`: the table comes up before the engine and goes down after it.
    pub nft: PathBuf,
}

/// The unit file. `exec` is the whole command line, quoted as systemd reads it; `%` is doubled
/// (systemd's specifiers) and a line break is refused: it would add lines to the unit.
pub fn unit(exec: &str, engine: Option<&EngineUnit>) -> anyhow::Result<String> {
    anyhow::ensure!(
        !exec.contains(['\n', '\r']),
        "a service command line cannot hold a line break"
    );
    let mut service = String::from("Type=simple\n");
    if let Some(engine) = engine {
        if let Some(uid) = engine.user {
            service.push_str(&format!("User={uid}\n"));
        }
        // nfqws2 binds its NFQUEUE and sends raw packets: as a user it needs both.
        service.push_str("AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW\n");
        // `+`: the table is root's even when the engine runs as the user.
        service.push_str(&format!(
            "ExecStartPre=+{} -f {NFT_CONFIG}\n",
            engine.nft.display()
        ));
    }
    service.push_str(&format!("ExecStart={}\n", exec.replace('%', "%%")));
    if let Some(engine) = engine {
        service.push_str(&format!(
            "ExecStopPost=+-{} delete table {NFT_TABLE}\n",
            engine.nft.display()
        ));
    }
    // An early boot may find the network or /home not ready yet.
    service.push_str("Restart=on-failure\nRestartSec=2\n");
    Ok(format!(
        "[Unit]\nDescription={DESCRIPTION}\nWants=network-online.target\n\
         After=network-online.target\n\n[Service]\n{service}\n\
         [Install]\nWantedBy=multi-user.target\n"
    ))
}

/// The first `nft` of the system folders: systemd's lines need an absolute path, and a user's
/// `PATH` often lacks `sbin`.
fn nft_path() -> anyhow::Result<PathBuf> {
    ["/usr/sbin/nft", "/sbin/nft", "/usr/bin/nft", "/bin/nft"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .context(
            "nft was not found: install nftables, zapret2 on Linux takes its packets through it",
        )
}

/// The unit file of `name`, systemd reloaded, enabled at boot when `auto_start`. The `zapret2`
/// unit runs the engine as this process's user (root when that is root).
pub fn create(name: &str, bin_path: &str, auto_start: bool) -> anyhow::Result<()> {
    let engine = if name == SERVICE2 {
        // SAFETY: `getuid` is a plain syscall without invariants.
        let uid = unsafe { libc::getuid() };
        Some(EngineUnit {
            user: (uid != 0).then_some(uid),
            nft: nft_path()?,
        })
    } else {
        None
    };
    crate::root::write(&unit_path(name), &unit(bin_path, engine.as_ref())?)?;
    crate::root::check("systemctl", &["daemon-reload"])?;
    let toggle = if auto_start { "enable" } else { "disable" };
    crate::root::check("systemctl", &[toggle, &unit_name(name)])
}

/// `systemctl stop`, `disable`, the unit file gone, errors ignored.
pub fn request_delete(name: &str) {
    let unit = unit_name(name);
    let _ = crate::root::run("systemctl", &["stop", &unit]);
    let _ = crate::root::run("systemctl", &["disable", &unit]);
    let path = unit_path(name);
    if path.exists() {
        crate::root::remove(&path);
        let _ = crate::root::run("systemctl", &["daemon-reload"]);
    }
}

/// Why `stop_and_delete` gave up.
pub(super) fn still_there(name: &str) -> String {
    format!(
        "service {name} is still known to systemd after its unit file was removed: it may live \
         outside /etc/systemd/system, remove it with the tool that installed it"
    )
}

/// The `zapret` and `zapret2` services and every engine process. Only the services have to go;
/// a process that does not end goes to the journal as a warning.
pub fn remove() -> anyhow::Result<()> {
    stop_and_delete(SERVICE)?;
    stop_and_delete(SERVICE2)?;
    if let Err(err) = crate::procs::kill_all(WINWS2) {
        log::warn!(target: "exec", "{WINWS2} was not ended: {err:#}");
    }
    Ok(())
}

pub fn start(name: &str) -> anyhow::Result<()> {
    if state(name) != SvcState::Running {
        crate::root::check("systemctl", &["start", &unit_name(name)])?;
    }
    Ok(())
}

pub fn stop(name: &str) -> anyhow::Result<()> {
    if matches!(
        state(name),
        SvcState::Running | SvcState::StartPending | SvcState::Other
    ) {
        crate::root::check("systemctl", &["stop", &unit_name(name)])?;
    }
    Ok(())
}

/// `net.ipv4.tcp_timestamps`; `None` when unreadable.
pub fn tcp_timestamps() -> Option<bool> {
    parse_timestamps(&std::fs::read_to_string("/proc/sys/net/ipv4/tcp_timestamps").ok()?)
}

/// `1` on, `0` off; `2` (timestamps without the random offset) is on too.
pub fn parse_timestamps(text: &str) -> Option<bool> {
    match text.trim() {
        "0" => Some(false),
        "1" | "2" => Some(true),
        _ => None,
    }
}

pub fn enable_tcp_timestamps() -> anyhow::Result<()> {
    crate::root::check("sysctl", &["-w", "net.ipv4.tcp_timestamps=1"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemctl_show_gives_the_state_and_pid() {
        let shown = "LoadState=loaded\nActiveState=active\nMainPID=4242\n";
        assert_eq!(parse_show(shown), Some((SvcState::Running, 4242)));
        let failed = "MainPID=0\nLoadState=loaded\nActiveState=failed\n";
        assert_eq!(parse_show(failed), Some((SvcState::Stopped, 0)));
        let starting = "LoadState=loaded\nActiveState=activating\nMainPID=7\n";
        assert_eq!(parse_show(starting), Some((SvcState::StartPending, 7)));
        let gone = "LoadState=not-found\nActiveState=inactive\nMainPID=0\n";
        assert_eq!(parse_show(gone), None);
        assert_eq!(parse_show(""), None);
    }

    #[test]
    fn the_engine_unit_runs_as_the_user_with_its_table_around_it() {
        let engine = EngineUnit {
            user: Some(1000),
            nft: PathBuf::from("/usr/sbin/nft"),
        };
        let exec = r#""/opt/z2/binaries/linux-x86_64/nfqws2" --chdir="/opt/z2" --qnum=200"#;
        let text = unit(exec, Some(&engine)).unwrap();
        assert_eq!(
            text,
            format!(
                "[Unit]\nDescription=zapret2 DPI bypass software\nWants=network-online.target\n\
                 After=network-online.target\n\n[Service]\nType=simple\nUser=1000\n\
                 AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW\n\
                 ExecStartPre=+/usr/sbin/nft -f /etc/zapret-hub/nfqws2.nft\n\
                 ExecStart={exec}\n\
                 ExecStopPost=+-/usr/sbin/nft delete table inet zapret_hub\n\
                 Restart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=multi-user.target\n"
            )
        );
        assert_eq!(exec_start(&text).as_deref(), Some(exec), "round trip");
    }

    #[test]
    fn a_root_engine_has_no_user_line_and_another_unit_no_engine_lines() {
        let engine = EngineUnit {
            user: None,
            nft: PathBuf::from("/sbin/nft"),
        };
        let text = unit("/bin/true", Some(&engine)).unwrap();
        assert!(!text.contains("User="), "{text}");
        assert!(text.contains("ExecStartPre=+/sbin/nft -f "), "{text}");
        let plain = unit("/bin/true", None).unwrap();
        for absent in ["User=", "Ambient", "nft"] {
            assert!(!plain.contains(absent), "{absent}: {plain}");
        }
    }

    #[test]
    fn a_percent_is_escaped_and_a_line_break_refused() {
        let text = unit("/bin/echo 100%", None).unwrap();
        assert!(text.contains("ExecStart=/bin/echo 100%%\n"), "{text}");
        assert_eq!(exec_start(&text).as_deref(), Some("/bin/echo 100%"));
        assert!(unit("/bin/true\nExecStartPre=+/bin/sh", None).is_err());
    }

    #[test]
    fn exec_start_takes_the_last_one_without_its_prefixes() {
        let cat = "# /etc/systemd/system/x.service\n[Service]\nExecStart=/old\n\
                   # /etc/systemd/system/x.service.d/o.conf\n[Service]\nExecStart=\n\
                   ExecStart=-/new --a\n";
        assert_eq!(exec_start(cat).as_deref(), Some("/new --a"));
        assert_eq!(exec_start("[Service]\nType=simple\n"), None);
    }

    #[test]
    fn running_units_list_names_and_descriptions() {
        let text = "openvpn.service loaded active running OpenVPN service\n\
                    cron.service loaded active running Regular background program\n\
                    junk\n";
        assert_eq!(
            parse_units(text),
            [
                ("openvpn".to_owned(), "OpenVPN service".to_owned()),
                ("cron".to_owned(), "Regular background program".to_owned()),
            ]
        );
    }

    #[test]
    fn reads_the_timestamps_value_as_the_kernel_keeps_it() {
        assert_eq!(parse_timestamps("1\n"), Some(true));
        assert_eq!(parse_timestamps("2\n"), Some(true));
        assert_eq!(parse_timestamps("0\n"), Some(false));
        assert_eq!(parse_timestamps("x"), None);
    }
}

//! Linux: state through `systemctl`, the unit file under `/etc/systemd/system` written with root
//! rights (`crate::root`). The `zapret2` unit runs the engine as this user with the network
//! capabilities its queue needs, and brings its nftables table up and down as root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Context as _;

use super::{SERVICE2, SvcConfig, SvcState, WINWS2, state, stop_and_delete};

/// The nftables table file the `zapret2` unit applies before the engine starts
/// (`zapret2::nft` writes it).
pub const NFT_CONFIG: &str = "/etc/zapret-hub/nfqws2.nft";
/// The table `NFT_CONFIG` makes, deleted when the engine stops.
pub const NFT_TABLE: &str = "inet zapret_hub";

const DESCRIPTION: &str = "zapret2 DPI bypass software";

fn cmd(program: &str, args: &[&str]) -> anyhow::Result<exec::Output> {
    Ok(exec::run(
        crate::root::c_locale(&mut Command::new(program)).args(args),
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

/// The last `ExecStart=` (a drop-in's replaces the unit's), without systemd's `-+!@:` prefixes:
/// the line as `exec_line` wrote it, which `create` takes back unchanged.
fn exec_start(unit: &str) -> Option<String> {
    unit.lines()
        .filter_map(|line| line.trim().strip_prefix("ExecStart="))
        .rfind(|value| !value.is_empty())
        .map(|value| {
            value
                .trim_start_matches(['-', '+', '!', '@', ':'])
                .to_owned()
        })
}

/// `argv` as an `ExecStart` value that systemd splits back into exactly `argv`: every word in
/// double quotes, with `\` and `"` escaped (systemd C-unescapes), `$` as `$$` (no variable
/// expansion) and `%` as `%%` (no specifiers). The engine runs with the argv it was checked with.
pub fn exec_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            let escaped = arg
                .replace('\\', r"\\")
                .replace('"', "\\\"")
                .replace('$', "$$")
                .replace('%', "%%");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
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

/// The unit file. `exec` is the command line as `exec_line` quotes it; a line break is refused:
/// it would add lines to the unit.
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
    service.push_str(&format!("ExecStart={exec}\n"));
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

/// `nft` by its absolute path: systemd's lines need one.
fn nft_path() -> anyhow::Result<PathBuf> {
    let nft = crate::root::tool("nft");
    anyhow::ensure!(
        nft.starts_with('/'),
        "nft was not found: install nftables, zapret2 on Linux takes its packets through it"
    );
    Ok(PathBuf::from(nft))
}

/// The user the engine runs as: this process's, or for a hub started through `sudo`/`pkexec` the
/// user who started it (root's engine drops to a uid that cannot read a home folder).
fn engine_user() -> Option<u32> {
    // SAFETY: `getuid` is a plain syscall without invariants.
    let uid = unsafe { libc::getuid() };
    let invoker = ["SUDO_UID", "PKEXEC_UID"]
        .iter()
        .find_map(|var| std::env::var(var).ok()?.parse::<u32>().ok());
    let uid = if uid == 0 { invoker.unwrap_or(0) } else { uid };
    (uid != 0).then_some(uid)
}

/// The first word of an `exec_line` line, unquoted, and the byte offset after its closing quote.
fn first_word(exec: &str) -> Option<(String, usize)> {
    let mut word = String::new();
    let mut chars = exec.strip_prefix('"')?.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((word.replace("$$", "$").replace("%%", "%"), i + 2)),
            '\\' => word.push(chars.next()?.1),
            c => word.push(c),
        }
    }
    None
}

/// The engine line of a root unit: `--uid=0:0` right after the program keeps nfqws2 root, as it
/// otherwise drops to uid 0x7FFFFFFF, which cannot read a zapret2 folder in root's home. A line
/// that names its user already (or had it added before) stays.
fn as_root(exec: &str) -> String {
    match first_word(exec) {
        Some((_, end)) if !exec.contains("\"--uid=") && !exec.contains("\"--user=") => {
            format!("{} \"--uid=0:0\"{}", &exec[..end], &exec[end..])
        }
        _ => exec.to_owned(),
    }
}

/// Why root must not run `exe` (`owner`/`mode` of the file, then of its folder): a file or folder
/// another user may change would run their code as root.
fn refuse_root_engine(exe: &str, file: (u32, u32), dir: (u32, u32)) -> Option<String> {
    let safe = |(owner, mode): (u32, u32)| owner == 0 && mode & 0o022 == 0;
    (!safe(file) || !safe(dir)).then(|| {
        format!(
            "Zapret Hub runs as root without sudo or pkexec, so zapret2 would run {exe} as root, \
             but that file or its folder is not root's alone: start Zapret Hub as your user (or \
             through sudo), or keep zapret2 in a folder only root may change"
        )
    })
}

/// The engine line of a unit that runs it as root (`as_root`), refused for a file that is not
/// root's alone (`refuse_root_engine`).
fn root_engine(exec: &str) -> anyhow::Result<String> {
    use std::os::unix::fs::MetadataExt as _;
    let exe = first_word(exec)
        .map(|(w, _)| PathBuf::from(w))
        .context("the engine's command line has no program")?;
    let meta = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| (m.uid(), m.mode()))
            .with_context(|| p.display().to_string())
    };
    let dir = exe.parent().unwrap_or(Path::new("/"));
    if let Some(why) = refuse_root_engine(&exe.display().to_string(), meta(&exe)?, meta(dir)?) {
        anyhow::bail!(why);
    }
    Ok(as_root(exec))
}

/// The unit of `name` as it should be: the `zapret2` one runs the engine (`EngineUnit`).
fn unit_for(name: &str, exec: &str) -> anyhow::Result<String> {
    if name != SERVICE2 {
        return unit(exec, None);
    }
    let user = engine_user();
    let exec = match user {
        Some(_) => exec.to_owned(),
        None => root_engine(exec)?,
    };
    let engine = EngineUnit {
        user,
        nft: nft_path()?,
    };
    unit(&exec, Some(&engine))
}

/// Steps that put `name` in place with command line `exec` (`exec_line`): stopped, its unit
/// written, systemd reloaded, enabled at boot when `auto_start`, and running again when `running`.
/// Writing over a unit and restarting it replaces it: no delete first.
pub fn add_replace(
    batch: &mut crate::root::Batch,
    name: &str,
    exec: &str,
    auto_start: bool,
    running: bool,
) -> anyhow::Result<()> {
    let unit = unit_name(name);
    batch.try_run(&["systemctl", "stop", &unit]);
    batch.write(&unit_path(name), &unit_for(name, exec)?)?;
    batch.run(&["systemctl", "daemon-reload"]);
    batch.run(&[
        "systemctl",
        if auto_start { "enable" } else { "disable" },
        &unit,
    ]);
    if running {
        batch.run(&["systemctl", "restart", &unit]);
    }
    Ok(())
}

/// Steps that remove `name`: stopped, disabled, its unit file gone, systemd reloaded.
pub fn add_remove(batch: &mut crate::root::Batch, name: &str) {
    let unit = unit_name(name);
    batch.try_run(&["systemctl", "stop", &unit]);
    batch.try_run(&["systemctl", "disable", &unit]);
    batch.run(&["rm", "-f", "--", &unit_path(name).display().to_string()]);
    batch.try_run(&["systemctl", "daemon-reload"]);
}

/// The unit file of `name`, systemd reloaded, enabled at boot when `auto_start`: one root batch.
pub fn create(name: &str, exec: &str, auto_start: bool) -> anyhow::Result<()> {
    let mut batch = crate::root::Batch::new();
    add_replace(&mut batch, name, exec, auto_start, false)?;
    batch.exec()
}

/// `systemctl stop`, `disable`, the unit file gone: one root batch, errors ignored.
pub fn request_delete(name: &str) {
    let mut batch = crate::root::Batch::new();
    add_remove(&mut batch, name);
    if let Err(err) = batch.exec() {
        log::warn!(target: "exec", "{name} was not removed: {err:#}");
    }
}

/// Why `stop_and_delete` gave up.
pub(super) fn still_there(name: &str) -> String {
    format!(
        "service {name} is still known to systemd after its unit file was removed: it may live \
         outside /etc/systemd/system, remove it with the tool that installed it"
    )
}

/// Whether the hub wrote `name`'s unit file (`/etc/systemd/system`, the hub's description): only
/// such a unit is disabled and deleted. bol-van/zapret's `zapret` is the user's own install.
pub fn ours(name: &str) -> bool {
    std::fs::read_to_string(unit_path(name)).is_ok_and(|text| written_by_hub(&text))
}

fn written_by_hub(unit: &str) -> bool {
    unit.lines()
        .any(|line| line.trim().strip_prefix("Description=") == Some(DESCRIPTION))
}

/// Why `stop_and_delete` stopped `name` and left it in place.
pub(super) fn not_ours(name: &str) -> String {
    format!(
        "{name}.service was not installed by Zapret Hub (bol-van/zapret's own unit?): it was \
         stopped, not disabled or removed; uninstall it with the tool that installed it"
    )
}

/// The `zapret2` service and every engine process. Only the service has to go; a process that
/// does not end goes to the journal as a warning. bol-van's `zapret` is not the hub's: the
/// system checks offer to deal with it.
pub fn remove() -> anyhow::Result<()> {
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

/// `start` without a polkit dialog (root or `sudo -n` only): for what the hub does on its own.
pub fn start_unprompted(name: &str) -> anyhow::Result<()> {
    let unit = unit_name(name);
    crate::root::run_unprompted("systemctl", &["start", &unit])
        .with_context(|| {
            format!(
                "{unit} is enabled but down and was not started: that needs root without a \
                 password (sudo -n); start it in Tools or apply the strategy again"
            )
        })?
        .check()?;
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

/// The last 100 lines of the unit's journal: as this user (enough in the `systemd-journal` or
/// `adm` group), else as root without a dialog (`sudo -n`).
pub fn logs(name: &str) -> anyhow::Result<String> {
    let unit = unit_name(name);
    let args = ["-u", unit.as_str(), "-n", "100", "--no-pager"];
    let lines = |out: exec::Output| {
        let text = out.stdout.trim().to_owned();
        (out.success() && !text.is_empty() && !text.contains("-- No entries --")).then_some(text)
    };
    cmd("journalctl", &args)
        .ok()
        .and_then(lines)
        .or_else(|| crate::root::run_unprompted("journalctl", &args).and_then(lines))
        .with_context(|| {
            format!(
                "the journal of {unit} is empty or unreadable for this user: \
                 run `journalctl -u {unit}` as root or join the systemd-journal group"
            )
        })
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
    fn only_a_unit_the_hub_wrote_is_its_to_remove() {
        let hub = unit(r#""/z/nfqws2""#, None).unwrap();
        assert!(written_by_hub(&hub));
        // bol-van/zapret's init.d/systemd/zapret.service
        let bol_van = "[Unit]\nDescription=zapret\nAfter=network-online.target\n\n\
                       [Service]\nType=forking\nExecStart=/opt/zapret/init.d/sysv/zapret start\n";
        assert!(!written_by_hub(bol_van));
        assert!(!written_by_hub(""));
    }

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
        let exec = r#""/opt/z2/binaries/linux-x86_64/nfqws2" "--chdir=/opt/z2" "--qnum=200""#;
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
    fn a_root_engine_keeps_root_once_and_only_for_roots_own_file() {
        let argv: Vec<String> = ["/root/my \"z2\"/nfqws2", "--qnum=200"]
            .map(str::to_owned)
            .to_vec();
        let line = exec_line(&argv);
        assert_eq!(
            first_word(&line),
            Some(("/root/my \"z2\"/nfqws2".to_owned(), 24))
        );
        let root = as_root(&line);
        assert_eq!(
            systemd_split(&root),
            ["/root/my \"z2\"/nfqws2", "--uid=0:0", "--qnum=200"]
        );
        assert_eq!(as_root(&root), root, "added once");
        let own = exec_line(&["/z/nfqws2".into(), "--user=nobody".into()]);
        assert_eq!(as_root(&own), own, "the strategy's own user stays");

        let exe = "/root/z2/nfqws2";
        assert_eq!(refuse_root_engine(exe, (0, 0o755), (0, 0o700)), None);
        for (file, dir) in [
            ((1000, 0o755), (0, 0o755)),
            ((0, 0o775), (0, 0o755)),
            ((0, 0o755), (1000, 0o755)),
            ((0, 0o755), (0, 0o777)),
        ] {
            assert!(
                refuse_root_engine(exe, file, dir).is_some(),
                "{file:?} {dir:?}"
            );
        }
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

    /// systemd's reading of a line `exec_line` wrote: double-quoted words, `\` escapes, then
    /// `$` and `%%` (the subset of systemd's grammar that line uses).
    fn systemd_split(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c != '"' {
                assert_eq!(c, ' ', "only quoted words: {line}");
                continue;
            }
            let mut word = String::new();
            loop {
                match chars.next().expect("closed quote") {
                    '"' => break,
                    '\\' => word.push(chars.next().expect("escaped char")),
                    other => word.push(other),
                }
            }
            words.push(word.replace("$$", "$").replace("%%", "%"));
        }
        words
    }

    #[test]
    fn the_engine_runs_with_exactly_the_argv_it_was_checked_with() {
        let argv: Vec<String> = [
            "/home/me/my zapret/nfqws2",
            "--chdir=/home/me/my zapret",
            r"--lua-init=x='\x16' .. $HOME",
            "'a b'",
            r#"say "hi"\"#,
            "100%",
        ]
        .map(str::to_owned)
        .to_vec();
        let line = exec_line(&argv);
        assert_eq!(systemd_split(&line), argv);
        let text = unit(&line, None).unwrap();
        assert_eq!(
            exec_start(&text).as_deref(),
            Some(line.as_str()),
            "round trip"
        );
        assert!(unit("/bin/true\nExecStartPre=+/bin/sh", None).is_err());
    }

    #[test]
    fn a_replace_is_one_batch_and_a_remove_too() {
        let mut batch = crate::root::Batch::new();
        add_replace(&mut batch, "other", "\"/bin/true\"", true, true).unwrap();
        let script = batch.script();
        for step in [
            "'systemctl' 'stop' 'other.service' || true",
            "'/etc/systemd/system/other.service'",
            "'systemctl' 'daemon-reload'",
            "'systemctl' 'enable' 'other.service'",
            "'systemctl' 'restart' 'other.service'",
        ] {
            assert!(script.contains(step), "{step}: {script}");
        }
        let mut batch = crate::root::Batch::new();
        add_remove(&mut batch, "other");
        assert!(
            batch
                .script()
                .contains("'rm' '-f' '--' '/etc/systemd/system/other.service'")
        );
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

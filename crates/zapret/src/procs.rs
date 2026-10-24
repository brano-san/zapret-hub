//! Running processes: finding the engine and its arguments, stopping it, starting it again,
//! and the engine the tests run themselves.

use std::io::Read;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub exe: Option<PathBuf>,
    /// Arguments after the program name.
    pub args: Vec<String>,
}

/// Processes whose image name is `image` (case-insensitive), with exe path and arguments.
pub fn find(image: &str) -> Vec<Proc> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
    system
        .processes()
        .values()
        .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(image))
        .map(|p| Proc {
            pid: p.pid().as_u32(),
            exe: p.exe().map(Path::to_path_buf),
            args: p
                .cmd()
                .iter()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
        })
        .collect()
}

pub fn is_running(image: &str) -> bool {
    !find(image).is_empty()
}

#[cfg(windows)]
/// `taskkill /IM <image> /F`, when one runs. A process that ends between the check and taskkill
/// makes taskkill fail: with none left that is done; otherwise the error carries taskkill's output.
/// taskkill returns once termination is asked for, not done: this waits until the processes are
/// gone, so their handles (WinDivert's among them) are closed before what comes next starts.
pub fn kill_all(image: &str) -> anyhow::Result<()> {
    if !is_running(image) {
        return Ok(());
    }
    let output = exec::run(
        Command::new("taskkill").args(["/IM", image, "/F"]),
        Duration::from_secs(15),
        None,
    )?;
    if !output.success() && !is_running(image) {
        log::info!(target: "exec", "{image} ended by itself before taskkill reached it");
        return Ok(());
    }
    output.check()?;
    let started = Instant::now();
    while is_running(image) {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(10),
            "{image} is still running 10 s after taskkill /F"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// SIGKILL to every process named `image` but this one, then waits until the name is gone, so
/// their queue and sockets are closed before what comes next starts.
#[cfg(target_os = "linux")]
pub fn kill_all(image: &str) -> anyhow::Result<()> {
    use sysinfo::Pid;
    let started = Instant::now();
    loop {
        let alive: Vec<Pid> = find(image)
            .into_iter()
            .filter(|proc| proc.pid != std::process::id())
            .map(|proc| Pid::from_u32(proc.pid))
            .collect();
        if alive.is_empty() {
            return Ok(());
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(10),
            "{image} is still running 10 s after SIGKILL ({alive:?}): it may be root's, stop its service"
        );
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&alive),
            true,
            ProcessRefreshKind::nothing(),
        );
        for proc in system.processes().values() {
            proc.kill();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Starts a process from a snapshot again, without a window, not waiting for it. It gets no
/// standard handles: a long-lived winws must not hold a pipe of ours open.
pub fn spawn_detached(proc: &Proc) -> anyhow::Result<()> {
    let exe = proc
        .exe
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pid {} had no executable path", proc.pid))?;
    let mut cmd = Command::new(exe);
    cmd.args(&proc.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(exec::CREATE_NO_WINDOW);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    let child = cmd.spawn()?;
    log::info!(target: "exec", "started {} (pid {}) {}", exe.display(), child.id(), proc.args.join(" "));
    Ok(())
}

/// A `winws.exe` or `winws2.exe` the tests start directly, with the arguments the service would
/// get (7.3).
pub struct Winws {
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
    readers: Vec<JoinHandle<()>>,
    /// `winws.exe` or `winws2.exe`: the messages name it.
    image: String,
}

impl Winws {
    /// `"<exe>" <tail>` in `cwd`, no window: the engine from zapret2's root. Windows passes
    /// `tail` as it is; Linux splits it as a command line (quotes group, then go).
    pub fn start(exe: &Path, cwd: &Path, tail: &str) -> std::io::Result<Self> {
        let mut command = Command::new(exe);
        #[cfg(windows)]
        command.raw_arg(tail).creation_flags(exec::CREATE_NO_WINDOW);
        #[cfg(target_os = "linux")]
        command.args(crate::spawn_tokens(tail));
        let mut child = command
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::new();
        let pipes: [Option<Box<dyn Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ];
        for mut pipe in pipes.into_iter().flatten() {
            let output = Arc::clone(&output);
            readers.push(std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    output.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            }));
        }
        let image = exe.file_name().map_or_else(
            || exe.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        log::info!(target: "exec", "started {image} (pid {}) {}", child.id(), tail.trim_start());
        Ok(Self {
            child,
            output,
            readers,
            image,
        })
    }

    /// Waits until the engine is ready, exits, or `max` passes. Windows: winws2 reports capture
    /// started (`windivert initialized. capture is started.`). Linux: nfqws2 says nothing a pipe
    /// shows in time, so ready is its queue bound (`queue::ready`), which
    /// happens after its Lua init, plus a short settle. One that exited is a launch error: `Err`
    /// carries its exit status and output.
    pub fn wait_ready(&mut self, max: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if let Some(error) = self.exited_error() {
                return Err(error);
            }
            if started.elapsed() >= max {
                return Ok(());
            }
            #[cfg(windows)]
            if self.output_text().contains("capture is started") {
                return Ok(());
            }
            #[cfg(target_os = "linux")]
            if queue::ready(self.child.id()) {
                std::thread::sleep(queue::SETTLE);
                return self.exited_error().map_or(Ok(()), Err);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// The status and whole output of an engine that ended by itself: the pipes close with the
    /// process, so once the readers end the output is whole.
    fn exited_error(&mut self) -> Option<String> {
        let status = self.child.try_wait().ok().flatten()?;
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        Some(format!(
            "{} exited ({status}):\n{}",
            self.image,
            self.output_text()
        ))
    }

    pub fn output_text(&self) -> String {
        exec::decode(&self.output.lock().unwrap())
    }

    /// The exit status of a winws that ended by itself. One that exits while the checks run is a
    /// launch error too.
    pub fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Kills winws and returns everything it printed.
    pub fn stop(mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        let text = self.output_text();
        log::info!(target: "exec", "stopped {} (pid {})\n{text}", self.image, self.child.id());
        text
    }
}

/// Waits for the exit too: the next strategy must not start while this one's WinDivert handle is
/// open.
impl Drop for Winws {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The kernel's NFQUEUE list, where nfqws2 shows it is ready.
#[cfg(target_os = "linux")]
pub mod queue {
    use std::time::Duration;

    /// The queue the nftables table fills and `nfqws2 --qnum=` binds (`zapret2::nft`).
    pub const QNUM: u32 = 200;
    pub const FILE: &str = "/proc/net/netfilter/nfnetlink_queue";
    /// After the queue is bound: the last of the init reaching the pipes.
    pub const SETTLE: Duration = Duration::from_millis(150);

    /// The queue file: read directly, else through `sudo -n` (the kernel makes it `0440 root`).
    /// Never pkexec: this is polled, a dialog per poll is not an option. `None` when neither works;
    /// once `sudo -n` failed it is not tried again.
    // ponytail: the refusal is kept until the app restarts; a sudoers rule added meanwhile needs it.
    pub fn text() -> Option<String> {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SUDO_FAILED: AtomicBool = AtomicBool::new(false);
        std::fs::read_to_string(FILE).ok().or_else(|| {
            if SUDO_FAILED.load(Ordering::Relaxed) {
                return None;
            }
            let text = crate::root::run_unprompted("cat", &[FILE])
                .filter(exec::Output::success)
                .map(|out| out.stdout);
            SUDO_FAILED.store(text.is_none(), Ordering::Relaxed);
            text
        })
    }

    /// The engine `pid` is ready: queue `QNUM` has a listener. Without the queue file (`0440
    /// root`, no `sudo -n`): `pid` holds a NETLINK_NETFILTER socket, which nfqws2 opens and binds
    /// to the queue at once, after its Lua init.
    pub fn ready(pid: u32) -> bool {
        match text() {
            Some(text) => parse(&text, QNUM).is_some_and(|(portid, _)| portid != 0),
            None => holds_netfilter(pid),
        }
    }

    /// `/proc/net/netlink` (`0444`): the inodes of the NETLINK_NETFILTER (`Eth` 12) sockets.
    pub fn netfilter_inodes(text: &str) -> Vec<u64> {
        text.lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                (fields.get(1) == Some(&"12")).then_some(())?;
                fields.get(9)?.parse().ok()
            })
            .collect()
    }

    /// One of `pid`'s descriptors (`socket:[inode]`) is a NETLINK_NETFILTER socket.
    fn holds_netfilter(pid: u32) -> bool {
        let Ok(text) = std::fs::read_to_string("/proc/net/netlink") else {
            return false;
        };
        let inodes = netfilter_inodes(&text);
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            return false;
        };
        fds.filter_map(Result::ok).any(|fd| {
            std::fs::read_link(fd.path()).is_ok_and(|link| {
                let link = link.to_string_lossy();
                link.strip_prefix("socket:[")
                    .and_then(|rest| rest.strip_suffix(']'))
                    .and_then(|inode| inode.parse::<u64>().ok())
                    .is_some_and(|inode| inodes.contains(&inode))
            })
        })
    }

    /// `(peer_portid, queue_total)` of queue `qnum`. The kernel writes, in order: the queue
    /// number, the listener's netlink port, the packets waiting, then counters
    /// (`  200  159031     0 2 65531 0 0 254  1`).
    pub fn parse(text: &str, qnum: u32) -> Option<(u32, u32)> {
        text.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()?.parse::<u32>().ok()? == qnum).then_some(())?;
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_queue_line_yields_the_port_and_the_packets() {
            let text = "  100 1 0 2 65531 0 0 1 1\n  200  159031     0 2 65531 0 0 254  1\n";
            assert_eq!(parse(text, 200), Some((159031, 0)));
            assert_eq!(parse("200 0 3 2 65531", 200), Some((0, 3)));
            assert_eq!(parse("300 1 2", 200), None);
            assert_eq!(parse("2000 1 2", 200), None);
            assert_eq!(parse("", 200), None);
        }

        #[test]
        fn netfilter_sockets_are_protocol_12() {
            let text = "sk               Eth Pid        Groups   Rmem     Wmem     Dump  Locks    Drops    Inode
                        0000000000000000 0   3283398358 00000000 0        0        0     2        0        10252
                        ffff8a0c1b2d4000 12  4021       00000000 0        0        0     2        0        88731
                        ffff8a0c1b2d5000 12  -1123      00000000 0        0        0     2        0        88740
";
            assert_eq!(netfilter_inodes(text), [88731, 88740]);
            assert!(netfilter_inodes("").is_empty());
        }
    }
}

//! Running processes: finding `winws.exe` and its arguments, stopping it, starting it again,
//! and the winws the tests run themselves.

use std::io::Read;
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
        .stderr(Stdio::null())
        .creation_flags(exec::CREATE_NO_WINDOW);
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
    /// `"<exe>" <tail>` in `cwd`, no window: Flowseal's `bin\winws.exe` from `bin`, zapret2's
    /// `winws2.exe` from its root.
    pub fn start(exe: &Path, cwd: &Path, tail: &str) -> std::io::Result<Self> {
        let mut child = Command::new(exe)
            .raw_arg(tail)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(exec::CREATE_NO_WINDOW)
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

    /// Waits until winws reports capture started (winws2: `windivert initialized. capture is
    /// started.`), exits, or `max` passes. One that exited is a launch error: `Err` carries its exit
    /// status and output.
    pub fn wait_ready(&mut self, max: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                // The pipes close with the process: once the readers end, the output is whole.
                for reader in self.readers.drain(..) {
                    let _ = reader.join();
                }
                return Err(format!(
                    "{} exited ({status}):\n{}",
                    self.image,
                    self.output_text()
                ));
            }
            if self.output_text().contains("capture is started") || started.elapsed() >= max {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
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

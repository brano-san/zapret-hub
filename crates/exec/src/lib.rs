//! Every external program Zapret Hub starts goes through [`run`]: no console window, a hard
//! timeout, an optional cancel flag, and one journal entry with the whole output (spec 10).

mod time;

pub use time::LocalTime;

use std::io::Read;
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// `CREATE_NO_WINDOW`: console programs (sc, netsh, curl, winws) must not flash a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a finished, or killed, program left behind.
#[derive(Debug, Clone)]
pub struct Output {
    /// Program and arguments, as the journal shows them.
    pub command: String,
    /// `None` when the process was killed on timeout or cancel.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub canceled: bool,
    pub elapsed: Duration,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// `self` when the exit code is 0, otherwise an error that carries all of it.
    pub fn check(self) -> anyhow::Result<Self> {
        if self.success() {
            Ok(self)
        } else {
            Err(anyhow::anyhow!(self.describe()))
        }
    }

    /// Command, outcome and both streams, never shortened.
    pub fn describe(&self) -> String {
        let outcome = if self.canceled {
            "canceled".to_owned()
        } else if self.timed_out {
            format!("killed after {} ms (timeout)", self.elapsed.as_millis())
        } else {
            match self.code {
                Some(code) => format!("exit code {code}"),
                None => "no exit code".to_owned(),
            }
        };
        format!(
            "{} -> {outcome}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.command, self.stdout, self.stderr
        )
    }
}

/// Runs `cmd` to the end, or kills it and everything it started after `timeout` or once `cancel`
/// turns true. When `run` returns it has ended every process the program started, so long-lived
/// processes must not be started through it.
pub fn run(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
) -> std::io::Result<Output> {
    let command = command_line(cmd);
    let started = Instant::now();
    let spawned = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            log::error!(target: "exec", "{command} -> failed to start: {err}");
            return Err(err);
        }
    };
    // ponytail: a grandchild started before this assignment escapes the job; CREATE_SUSPENDED fixes it
    let job = match Job::holding(&child) {
        Ok(job) => Some(job),
        Err(err) => {
            log::warn!(
                target: "exec",
                "{command} -> no job object, a timeout ends only the program itself: {err}"
            );
            None
        }
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (mut timed_out, mut canceled) = (false, false);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        canceled = cancel.is_some_and(|flag| flag.load(Ordering::Relaxed));
        timed_out = started.elapsed() >= timeout;
        if canceled || timed_out {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Whatever the program started and left running keeps the pipes open, and `join` below waiting.
    if let Some(job) = &job {
        job.terminate();
    }
    let output = Output {
        command,
        code: status.and_then(|status| status.code()),
        stdout: decode(&stdout.join().unwrap_or_default()),
        stderr: decode(&stderr.join().unwrap_or_default()),
        timed_out,
        canceled,
        elapsed: started.elapsed(),
    };
    log::info!(target: "exec", "{}", output.describe());
    Ok(output)
}

fn command_line(cmd: &Command) -> String {
    let mut line = cmd.get_program().to_string_lossy().into_owned();
    for arg in cmd.get_args() {
        line.push(' ');
        line.push_str(&arg.to_string_lossy());
    }
    line
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// A job object holding a started program and everything it starts. Closing it ends them all
/// (`KILL_ON_JOB_CLOSE`), so even a crash of ours leaves no orphan behind.
struct Job(HANDLE);

impl Job {
    /// A new job with `child` already in it.
    fn holding(child: &Child) -> windows::core::Result<Self> {
        // SAFETY: no security attributes and no name; the handle is owned by the `Job` built here.
        let job = Self(unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null())? });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` is the structure this information class takes, and the size is its own;
        // the process handle stays valid while `child` is borrowed.
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of_val(&limits) as u32,
            )?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))?;
        }
        Ok(job)
    }

    /// Ends every process in the job at once.
    fn terminate(&self) {
        // SAFETY: the handle is a live job object, owned by `self`.
        let _ = unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is closed here and nowhere else.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Console programs write UTF-8, UTF-16 (some system tools when redirected) or the OEM code
/// page (cp866 on Russian Windows); this reads all three.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16le(rest);
    }
    let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) && odd_zeros * 4 > bytes.len() {
        return utf16le(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => oem(bytes),
    }
}

fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

fn oem(bytes: &[u8]) -> String {
    use windows::Win32::Globalization::{
        CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar,
    };
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    // SAFETY: valid slices; the first call only measures.
    let len = unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, None) };
    if len <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; len as usize];
    // SAFETY: `wide` is exactly as long as the first call asked for.
    unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, Some(&mut wide)) };
    String::from_utf16_lossy(&wide)
}

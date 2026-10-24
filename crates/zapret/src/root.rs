//! Linux steps that only root may do (`nft`, system `systemctl`, files under `/etc`): directly
//! when this process is root, else through `sudo -n` (never a password prompt), else through
//! `pkexec` (a polkit agent asks in the desktop). Standard input is closed (`exec::run`), so
//! neither ever waits on a terminal. The error says what is missing when both are refused.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::Context as _;

/// A polkit dialog waits for the user: as long as `exec::run` lets it.
const WAIT: Duration = Duration::from_secs(120);

/// What a refused step says: the user can fix the rights themselves.
pub const HINT: &str = "not enough rights for this step: allow it without a password in sudoers \
     (sudo -n), run a polkit agent so pkexec can ask, or start Zapret Hub as root";

/// Has this process root rights?
pub fn is_root() -> bool {
    // SAFETY: `geteuid` is a plain syscall without invariants.
    (unsafe { libc::geteuid() }) == 0
}

/// How a step reaches root, in the order tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Via {
    Direct,
    Sudo,
    Pkexec,
}

impl Via {
    fn command(self, prog: &str, args: &[&str]) -> Command {
        let mut cmd = match self {
            Self::Direct => Command::new(prog),
            Self::Sudo => {
                let mut cmd = Command::new("sudo");
                cmd.args(["-n", "--", prog]);
                cmd
            }
            Self::Pkexec => {
                // Never its text agent: it would read the terminal from a background process
                // group and stop there. Without a desktop agent pkexec says so and exits 127.
                let mut cmd = Command::new("pkexec");
                cmd.args(["--disable-internal-agent", prog]);
                cmd
            }
        };
        cmd.args(args);
        cmd
    }
}

fn order(root: bool) -> &'static [Via] {
    if root {
        &[Via::Direct]
    } else {
        &[Via::Sudo, Via::Pkexec]
    }
}

/// `sudo -n` said no before running anything: a password is needed, or the rule is missing.
fn sudo_refused(out: &exec::Output) -> bool {
    out.code == Some(1)
        && [
            "a password is required",
            "a terminal is required",
            "not in the sudoers file",
            "is not allowed to execute",
            "may not run sudo",
        ]
        .iter()
        .any(|phrase| out.stderr.contains(phrase))
}

/// `pkexec` exits 126 when the user dismissed or failed the dialog, 127 when no polkit agent can
/// ask; the program it would run never started.
fn pkexec_refused(out: &exec::Output) -> bool {
    matches!(out.code, Some(126 | 127))
        && [
            "Not authorized",
            "dismissed",
            "No authentication agent",
            "Error executing command as another user",
        ]
        .iter()
        .any(|phrase| out.stderr.contains(phrase))
}

/// Runs `prog args` as root, and what came of it: the program's own failure is the caller's to
/// read. Only when every way to root was refused (or none is installed) it is an error with
/// {HINT} and what each way said.
pub fn run(prog: &str, args: &[&str]) -> anyhow::Result<exec::Output> {
    let mut refusals = Vec::new();
    for &via in order(is_root()) {
        let out = match exec::run(&mut via.command(prog, args), WAIT, None) {
            Ok(out) => out,
            // No sudo (or no pkexec) on this system: the next way.
            Err(err) if via != Via::Direct && err.kind() == ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(anyhow::Error::new(err).context(format!("starting {prog}")));
            }
        };
        let refused = match via {
            Via::Direct => false,
            Via::Sudo => sudo_refused(&out),
            Via::Pkexec => pkexec_refused(&out),
        };
        if !refused {
            return Ok(out);
        }
        refusals.push(out.describe());
    }
    anyhow::bail!("{HINT}\n\n{}", refusals.join("\n\n"))
}

/// `prog args` as root without ever asking: directly as root, else `sudo -n` only. `None` when
/// that is refused or impossible. For what is polled or merely nice to read (no pkexec dialog).
pub fn run_unprompted(prog: &str, args: &[&str]) -> Option<exec::Output> {
    let via = if is_root() { Via::Direct } else { Via::Sudo };
    let out = exec::run(&mut via.command(prog, args), WAIT, None).ok()?;
    (via == Via::Direct || !sudo_refused(&out)).then_some(out)
}

/// `run` that must succeed.
pub fn check(prog: &str, args: &[&str]) -> anyhow::Result<()> {
    run(prog, args)?.check()?;
    Ok(())
}

/// A system tool by its absolute path when it is in a system folder: a user's `PATH` often lacks
/// `sbin` (`nft`, `getcap`, `setcap`). Else the bare name, for `PATH` to find.
pub fn tool(name: &str) -> String {
    ["/usr/sbin", "/sbin", "/usr/bin", "/bin"]
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|path| path.is_file())
        .map_or_else(|| name.to_owned(), |path| path.display().to_string())
}

/// Several root steps as one shell script, so they cost one `sudo -n`/pkexec (one dialog). Files
/// to write are staged in private temp files (created with `O_EXCL`, mode 0600: nobody else can
/// swap their text before root copies them) that live as long as the batch.
#[derive(Default)]
pub struct Batch {
    lines: Vec<String>,
    staged: Vec<tempfile::NamedTempFile>,
}

/// `arg` as one shell word: single-quoted, a `'` inside as `'\''`.
fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

impl Batch {
    pub fn new() -> Self {
        Self::default()
    }

    /// A step whose failure stops the batch.
    pub fn run(&mut self, argv: &[&str]) -> &mut Self {
        self.lines.push(
            argv.iter()
                .map(|arg| quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        );
        self
    }

    /// A step whose failure does not matter (stopping what may not run).
    pub fn try_run(&mut self, argv: &[&str]) -> &mut Self {
        self.run(argv);
        if let Some(line) = self.lines.last_mut() {
            line.push_str(" || true");
        }
        self
    }

    /// `contents` in a private temp file that lives as long as the batch; its path.
    pub fn stage(&mut self, contents: &str) -> anyhow::Result<String> {
        let mut temp = tempfile::NamedTempFile::new().context("cannot create a temp file")?;
        std::io::Write::write_all(&mut temp, contents.as_bytes())
            .context("cannot write a temp file")?;
        let path = temp.path().display().to_string();
        self.staged.push(temp);
        Ok(path)
    }

    /// Writes `contents` into `dest`: `cp` over a file that exists (its inode, owner, mode and
    /// SELinux label stay: `/etc/hosts`), a new one with `install -D -m 0644` (folder created).
    pub fn write(&mut self, dest: &Path, contents: &str) -> anyhow::Result<&mut Self> {
        let from = self.stage(contents)?;
        let to = dest.display().to_string();
        if dest.exists() {
            self.run(&["cp", "--", &from, &to]);
        } else {
            self.run(&["install", "-D", "-m", "0644", "--", &from, &to]);
        }
        Ok(self)
    }

    /// The script: stop at the first failing step (`set -e`).
    pub fn script(&self) -> String {
        format!("set -e\n{}\n", self.lines.join("\n"))
    }

    /// Runs every step as root at once; nothing to run is done. The error carries the script's
    /// whole output.
    pub fn exec(&self) -> anyhow::Result<()> {
        if self.lines.is_empty() {
            return Ok(());
        }
        run("sh", &["-c", &self.script()])?.check()?;
        Ok(())
    }
}

/// Writes `contents` into a root-owned file (see `Batch::write`).
pub fn write(path: &Path, contents: &str) -> anyhow::Result<()> {
    Batch::new()
        .write(path, contents)?
        .exec()
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stderr: &str) -> exec::Output {
        exec::Output {
            command: "sudo -n -- nft".into(),
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
            timed_out: false,
            canceled: false,
            elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn root_runs_directly_and_a_user_tries_sudo_then_pkexec() {
        assert_eq!(order(true), [Via::Direct]);
        assert_eq!(order(false), [Via::Sudo, Via::Pkexec]);
        let sudo = Via::Sudo.command("nft", &["-f", "x"]);
        assert_eq!(sudo.get_program(), "sudo");
        assert_eq!(
            sudo.get_args().collect::<Vec<_>>(),
            ["-n", "--", "nft", "-f", "x"]
        );
        let pkexec = Via::Pkexec.command("nft", &["-f", "x"]);
        assert_eq!(pkexec.get_program(), "pkexec");
        assert_eq!(
            pkexec.get_args().collect::<Vec<_>>(),
            ["--disable-internal-agent", "nft", "-f", "x"]
        );
    }

    #[test]
    fn a_sudo_refusal_is_told_from_the_program_failing() {
        assert!(sudo_refused(&out(1, "sudo: a password is required\n")));
        assert!(sudo_refused(&out(
            1,
            "me is not in the sudoers file.  This incident will be reported.\n"
        )));
        // `nft` itself exiting 1 under sudo is the program's answer, not a refusal.
        assert!(!sudo_refused(&out(1, "Error: No such file or directory\n")));
        assert!(!sudo_refused(&out(0, "")));
    }

    #[test]
    fn a_pkexec_refusal_is_told_from_the_program_failing() {
        assert!(pkexec_refused(&out(
            126,
            "Error executing command as another user: Not authorized\n"
        )));
        assert!(pkexec_refused(&out(
            127,
            "Error executing command as another user: No authentication agent found.\n"
        )));
        assert!(!pkexec_refused(&out(1, "Error: syntax error\n")));
        assert!(!pkexec_refused(&out(127, "sh: 1: nft: not found\n")));
    }

    #[test]
    fn a_batch_is_one_script_with_every_argument_quoted() {
        let mut batch = Batch::new();
        batch.try_run(&["systemctl", "stop", "zapret2.service"]);
        batch.run(&["install", "-D", "/tmp/a", "/etc/my dir/it's.conf"]);
        assert_eq!(
            batch.script(),
            "set -e\n'systemctl' 'stop' 'zapret2.service' || true\n\
             'install' '-D' '/tmp/a' '/etc/my dir/it'\\''s.conf'\n"
        );
        let path = batch.stage("text").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "text");
        assert!(Batch::new().exec().is_ok(), "an empty batch runs nothing");
    }

    #[test]
    fn the_quoted_script_runs_as_written() {
        // `sh` reads the quoting back: the odd argument arrives whole.
        let odd = r"a b'c$HOME\x16 %d";
        let mut batch = Batch::new();
        batch.run(&["printf", "%s", odd]);
        let out = exec::run(
            Command::new("sh").args(["-c", &batch.script()]),
            Duration::from_secs(10),
            None,
        )
        .unwrap();
        assert_eq!(out.stdout, odd);
    }

    #[test]
    fn the_hint_names_every_way_out() {
        for part in ["sudo -n", "polkit", "root"] {
            assert!(HINT.contains(part), "{part}");
        }
    }
}

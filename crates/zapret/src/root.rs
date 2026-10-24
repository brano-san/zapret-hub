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
                let mut cmd = Command::new("pkexec");
                cmd.arg(prog);
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

/// Writes `contents` into a root-owned file, mode 0644, its folder created: a plain write as
/// root, else one privileged `install` from a private temp file (created with `O_EXCL`, mode 0600:
/// nobody else can swap its text before root copies it).
pub fn write(path: &Path, contents: &str) -> anyhow::Result<()> {
    if is_root() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        return std::fs::write(path, contents)
            .with_context(|| format!("cannot write {}", path.display()));
    }
    let mut temp = tempfile::NamedTempFile::new().context("cannot create a temp file")?;
    std::io::Write::write_all(&mut temp, contents.as_bytes())
        .context("cannot write a temp file")?;
    let from = temp.path().display().to_string();
    let to = path.display().to_string();
    run("install", &["-D", "-m", "0644", &from, &to])?
        .check()
        .with_context(|| format!("cannot write {to}"))?;
    Ok(())
}

/// Removes `path` as root, best effort: a missing file is fine.
pub fn remove(path: &Path) {
    if path.exists() {
        let _ = run("rm", &["-f", "--", &path.display().to_string()]);
    }
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
        assert_eq!(pkexec.get_args().collect::<Vec<_>>(), ["nft", "-f", "x"]);
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
    fn the_hint_names_every_way_out() {
        for part in ["sudo -n", "polkit", "root"] {
            assert!(HINT.contains(part), "{part}");
        }
    }
}

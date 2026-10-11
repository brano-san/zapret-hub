//! The Task Scheduler entry «Zapret Hub»: at logon, with the highest rights, so the elevated app
//! starts without a UAC prompt (spec 4.2).

use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// `zapret-hub-cli start` runs it by this name.
pub const TASK: &str = cli::AUTOSTART_TASK;
const TIMEOUT: Duration = Duration::from_secs(30);

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn task_xml(user: &str, exe: &Path) -> String {
    let command = escape(&exe.display().to_string());
    let dir = escape(
        &exe.parent()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
    );
    let user = escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Zapret Hub at logon</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>--autostart</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The exe a task's XML starts.
pub fn command_of(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = start + xml[start..].find("</Command>")?;
    Some(
        xml[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

fn schtasks(args: &[&std::ffi::OsStr]) -> anyhow::Result<exec::Output> {
    let mut cmd = Command::new("schtasks.exe");
    cmd.args(args);
    Ok(exec::run(&mut cmd, TIMEOUT, None)?)
}

pub fn enable(exe: &Path) -> anyhow::Result<()> {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    // schtasks reads the file as the declaration says: UTF-16 with a byte order mark.
    let file = std::env::temp_dir().join("zapret-hub-task.xml");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(
        task_xml(&user, exe)
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    std::fs::write(&file, bytes)?;
    let created = schtasks(&[
        "/Create".as_ref(),
        "/TN".as_ref(),
        TASK.as_ref(),
        "/XML".as_ref(),
        file.as_os_str(),
        "/F".as_ref(),
    ]);
    let _ = std::fs::remove_file(&file);
    created?.check()?;
    Ok(())
}

pub fn disable() -> anyhow::Result<()> {
    let query = schtasks(&["/Query".as_ref(), "/TN".as_ref(), TASK.as_ref()])?;
    if query.success() {
        schtasks(&[
            "/Delete".as_ref(),
            "/TN".as_ref(),
            TASK.as_ref(),
            "/F".as_ref(),
        ])?
        .check()?;
    }
    Ok(())
}

/// The exe moved since the task was made, or the task is gone: it is made again for `exe`.
pub fn refresh(exe: &Path) -> anyhow::Result<()> {
    let query = schtasks(&[
        "/Query".as_ref(),
        "/TN".as_ref(),
        TASK.as_ref(),
        "/XML".as_ref(),
    ])?;
    if query.success()
        && command_of(&query.stdout).is_some_and(|command| Path::new(&command) == exe)
    {
        return Ok(());
    }
    enable(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_runs_this_exe_at_logon_with_the_highest_rights() {
        let exe = Path::new(r"C:\Tools & Apps\Zapret Hub\zapret-hub.exe");
        let xml = task_xml(r"PC\me", exe);
        assert!(
            xml.contains(r"<Command>C:\Tools &amp; Apps\Zapret Hub\zapret-hub.exe</Command>"),
            "{xml}"
        );
        assert!(
            xml.contains(r"<WorkingDirectory>C:\Tools &amp; Apps\Zapret Hub</WorkingDirectory>"),
            "{xml}"
        );
        assert!(xml.contains(r"<UserId>PC\me</UserId>"), "{xml}");
        assert!(
            xml.contains("<RunLevel>HighestAvailable</RunLevel>"),
            "{xml}"
        );
        assert!(xml.contains("<Arguments>--autostart</Arguments>"), "{xml}");
        assert_eq!(
            command_of(&xml).as_deref(),
            Some(r"C:\Tools & Apps\Zapret Hub\zapret-hub.exe")
        );
    }
}

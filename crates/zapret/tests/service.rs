use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use zapret::procs::Winws;
use zapret::service::{SvcState, config, parse_timestamps, state};

#[test]
fn reads_the_rfc1323_line_in_any_language() {
    let english = "TCP Global Parameters\r\n----\r\nECN Capability                      : disabled\r\nRFC 1323 Timestamps                 : enabled \r\n";
    assert_eq!(parse_timestamps(english), Some(true));
    let russian = "Временные метки RFC 1323            : disabled\r\n";
    assert_eq!(parse_timestamps(russian), Some(false));
    assert_eq!(parse_timestamps("nothing here"), None);
}

#[test]
fn reads_service_states_through_the_scm() {
    assert_eq!(state("BFE"), SvcState::Running);
    assert_eq!(state("zapret-hub-no-such-service"), SvcState::NotInstalled);
    assert!(!zapret::service::active_services().is_empty());
}

#[test]
fn reads_a_service_config_from_the_registry() {
    // BFE's ImagePath is a REG_EXPAND_SZ value, and BFE starts automatically.
    let bfe = config("BFE").expect("BFE has an ImagePath in the registry");
    assert!(
        bfe.image_path.to_lowercase().contains("svchost.exe"),
        "{}",
        bfe.image_path
    );
    assert!(bfe.auto_start);
    assert_eq!(config("zapret-hub-no-such-service"), None);
}

#[test]
fn a_service_without_a_recorded_strategy_has_none() {
    assert_eq!(
        zapret::service::strategy_of("zapret-hub-no-such-service"),
        None
    );
}

fn cmd_exe() -> PathBuf {
    PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec is set"))
}

#[test]
fn a_started_engine_is_ready_when_capture_starts() {
    // winws2 prints `windivert initialized. capture is started.`; cmd.exe stands in for it.
    let dir = tempfile::tempdir().unwrap();
    let mut winws = Winws::start(
        &cmd_exe(),
        dir.path(),
        r#"/c "cd&echo windivert initialized. capture is started.&ping -n 3 127.0.0.1>nul""#,
    )
    .unwrap();
    winws.wait_ready(Duration::from_secs(5)).unwrap();
    assert!(winws.exited().is_none());
    let output = winws.stop();
    assert!(output.contains("capture is started."), "{output}");
    let dir_name = dir.path().file_name().unwrap().to_string_lossy();
    assert!(
        output.contains(&*dir_name),
        "runs in the given folder: {output}"
    );
}

#[test]
fn an_engine_that_exits_is_a_launch_error_naming_its_image() {
    let dir = tempfile::tempdir().unwrap();
    let mut winws = Winws::start(
        &cmd_exe(),
        dir.path(),
        r#"/c "echo LUA ERROR: no such function&exit 87""#,
    )
    .unwrap();
    let err = winws.wait_ready(Duration::from_secs(5)).unwrap_err();
    assert!(err.to_lowercase().starts_with("cmd.exe exited"), "{err}");
    assert!(err.contains("87") && err.contains("LUA ERROR"), "{err}");
}

#[test]
fn finds_a_process_with_its_arguments() {
    let mut child = Command::new("ping")
        .args(["-n", "20", "127.0.0.1"])
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let found = zapret::procs::find("PING.EXE");
    let ours = found
        .iter()
        .find(|p| p.pid == child.id())
        .expect("our ping is listed");
    assert_eq!(ours.args, ["-n", "20", "127.0.0.1"]);
    assert!(
        ours.exe
            .as_ref()
            .is_some_and(|e| e.to_string_lossy().to_lowercase().ends_with("ping.exe"))
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
#[ignore = "needs admin rights and the zapret2 service installed"]
fn live_status_of_the_zapret2_service() {
    use zapret::service::SERVICE2;
    let status = zapret::service::status(SERVICE2);
    let config = zapret::service::config(SERVICE2);
    let strategy = zapret::service::strategy_of(SERVICE2);
    let timestamps = zapret::service::tcp_timestamps();
    eprintln!(
        "zapret2: {status:?}
config: {config:?}
strategy: {strategy:?}
timestamps: {timestamps:?}"
    );
    assert!(status.is_some(), "the zapret2 service is not installed");
    let config = config.expect("the service has a configuration");
    assert!(
        config.image_path.to_lowercase().contains("winws2.exe"),
        "{}",
        config.image_path
    );
    assert!(
        strategy.is_some(),
        "the app records the installed strategy in the registry"
    );
    assert!(
        timestamps.is_some(),
        "netsh reports the TCP timestamps setting"
    );
}

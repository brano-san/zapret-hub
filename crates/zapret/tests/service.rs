use std::process::Command;
use std::time::Duration;

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
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret"]
fn live_status_of_the_zapret_service() {
    let status = zapret::service::status(zapret::service::SERVICE);
    let config = zapret::service::config(zapret::service::SERVICE);
    let strategy = zapret::service::strategy();
    let timestamps = zapret::service::tcp_timestamps();
    let candidates = zapret::folder::candidates();
    eprintln!(
        "zapret: {status:?}\nconfig: {config:?}\nstrategy: {strategy:?}\ntimestamps: {timestamps:?}\ncandidates: {candidates:?}"
    );
    assert!(status.is_some(), "the zapret service is not installed");
    let config = config.expect("the service has a configuration");
    assert!(
        config.image_path.to_lowercase().contains("winws.exe"),
        "{}",
        config.image_path
    );
    assert!(
        strategy.is_some(),
        "service.bat records the installed strategy in the registry"
    );
    assert!(
        timestamps.is_some(),
        "netsh reports the TCP timestamps setting"
    );
    assert!(
        candidates.iter().any(|root| root
            .to_string_lossy()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(r"E:\Zapret")),
        "{candidates:?}"
    );
}

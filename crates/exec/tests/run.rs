use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[test]
fn captures_stdout_and_the_exit_code() {
    let out = exec::run(
        Command::new("cmd").args(["/c", "echo hello& exit 3"]),
        Duration::from_secs(10),
        None,
    )
    .unwrap();
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout.trim(), "hello");
    assert!(!out.timed_out && !out.canceled);
}

#[test]
fn kills_a_program_that_outlives_its_timeout() {
    let out = exec::run(
        Command::new("ping").args(["-n", "30", "127.0.0.1"]),
        Duration::from_millis(300),
        None,
    )
    .unwrap();
    assert!(out.timed_out);
    assert_eq!(out.code, None);
    assert!(out.elapsed < Duration::from_secs(5));
}

#[test]
fn stops_when_canceled() {
    let cancel = AtomicBool::new(true);
    let out = exec::run(
        Command::new("ping").args(["-n", "30", "127.0.0.1"]),
        Duration::from_secs(60),
        Some(&cancel),
    )
    .unwrap();
    assert!(out.canceled);
    assert!(out.elapsed < Duration::from_secs(5));
}

#[test]
fn a_grandchild_holding_the_pipes_does_not_outlive_the_timeout() {
    // `cmd` starts `ping` and `ping` keeps stdout open: killing only `cmd` leaves `run` waiting
    // for the pipes (about 7 s here), so the whole process tree has to end.
    let started = Instant::now();
    let out = exec::run(
        Command::new("cmd").args(["/c", "ping -n 8 127.0.0.1"]),
        Duration::from_millis(300),
        None,
    )
    .unwrap();
    assert!(out.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_process_left_behind_by_a_finished_program_does_not_hold_run() {
    // `cmd` exits at once and leaves `ping` running with the pipes open (about 4 s here): `run`
    // ends it and returns, and the exit code is still the program's own.
    let started = Instant::now();
    let out = exec::run(
        Command::new("cmd").args(["/c", "start /b ping -n 5 127.0.0.1"]),
        Duration::from_secs(10),
        None,
    )
    .unwrap();
    assert!(!out.timed_out);
    assert_eq!(out.code, Some(0));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_failed_check_carries_the_whole_output() {
    let out = exec::run(
        Command::new("cmd").args(["/c", "echo out& echo err 1>&2& exit 1"]),
        Duration::from_secs(10),
        None,
    )
    .unwrap();
    let err = out.check().unwrap_err().to_string();
    assert!(err.contains("exit code 1"), "{err}");
    assert!(
        err.contains("--- stdout ---\nout") && err.contains("--- stderr ---\nerr"),
        "{err}"
    );
}

#[test]
fn decode_reads_utf8_utf16_and_oem() {
    assert_eq!(exec::decode("ok ✓".as_bytes()), "ok ✓");
    assert_eq!(exec::decode(&[b'a', 0, b'b', 0, b'c', 0, b'd', 0]), "abcd");
    assert_eq!(exec::decode(&[0xFF, 0xFE, b'x', 0]), "x");
    // "Привет" in cp866, as `sc` prints it on Russian Windows: one char per byte in any single-byte
    // OEM page. With Windows' "UTF-8 for worldwide language support" the OEM page is 65001 and
    // these bytes are not text there.
    let text = exec::decode(&[0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2]);
    if unsafe { windows::Win32::Globalization::GetOEMCP() } == 65001 {
        assert!(!text.is_empty());
    } else {
        assert_eq!(text.chars().count(), 6);
    }
}

#[test]
fn stamp_has_the_shape_of_test_zapret_ps1_results() {
    let stamp = exec::LocalTime::now().stamp();
    assert_eq!(stamp.len(), "2026-10-10_05-03-34".len());
    assert_eq!(&stamp[4..5], "-");
    assert_eq!(&stamp[10..11], "_");
    assert_eq!(&stamp[13..14], "-");
}

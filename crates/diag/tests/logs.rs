use diag::logs::{discord, size, tail_from, tail_since};
use std::io::Write;
use std::path::PathBuf;

fn fx(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn lines(name: &str) -> Vec<String> {
    tail_since(&fx(name), 8 << 20, |_| true).unwrap().unwrap()
}

#[test]
fn discord_stalled_with_peer() {
    let v = discord::parse(&lines("discord-dave-stalled.log"), None);
    assert_eq!(v.own_ssrc, Some(11029));
    assert_eq!(v.peers, vec![10923]);
    assert_eq!(v.local_address.as_deref(), Some("203.0.113.10"));
    assert_eq!(v.media_server.as_deref(), Some("104.29.149.39:19327"));
    assert!(v.no_voice_state);
    assert_eq!(v.ws_closed.len(), 1);
    assert!(discord::stalled(&v));
}

#[test]
fn discord_alone_stalled() {
    let v = discord::parse(&lines("discord-dave-alone.log"), None);
    assert!(v.peers.is_empty());
    assert!(discord::stalled(&v));
}

#[test]
fn discord_done() {
    let v = discord::parse(&lines("discord-dave-done.log"), None);
    assert_eq!(v.dave, discord::Dave::Done);
    assert!(!discord::stalled(&v));
}

#[test]
fn discord_waits_by_the_wall_clock() {
    let wait = |wall| match discord::parse(&lines("discord-dave-alone.log"), wall).dave {
        discord::Dave::KeyPackageSent { secs_waiting } => secs_waiting,
        other => panic!("{other:?}"),
    };
    // The key package went at 00:37:55.5, the last line at 00:38:15.6.
    assert_eq!(wait(None), 20);
    // A quiet log after it: the clock still runs.
    assert_eq!(wait(Some(37.0 * 60.0 + 55.5 + 300.5)), 300);
    // Past midnight the wall clock is near 0 and the line near 24 h: one rollover.
    let line = discord::parse(
        &[
            "[2026-10-18 23:59:50.000] [info]  [RTCControlSocket(default)] Sending MLS key package"
                .to_owned(),
        ],
        Some(15.0),
    );
    assert_eq!(
        line.dave,
        discord::Dave::KeyPackageSent { secs_waiting: 25 }
    );
    // A log in another time zone: hours off, so the last line it is.
    assert_eq!(wait(Some(3.0 * 3600.0 + 37.0 * 60.0)), 20);
}

#[test]
fn missing_log_is_skipped() {
    assert!(
        tail_since(&fx("nope.log"), 1024, |_| true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn reads_only_lines_since_start() {
    let l = tail_since(&fx("discord-dave-done.log"), 8 << 20, |l| {
        l >= "[2026-10-18 00:37:55.600"
    })
    .unwrap()
    .unwrap();
    assert_eq!(l.len(), 2);
}

#[test]
fn tail_limits_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.log");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
    let line = format!("{}\n", "x".repeat(99));
    for _ in 0..(10 << 20) / 100 {
        f.write_all(line.as_bytes()).unwrap();
    }
    f.write_all(b"LAST\n").unwrap();
    drop(f);
    let l = tail_since(&p, 8 << 20, |_| true).unwrap().unwrap();
    assert!(l.iter().map(|s| s.len() as u64 + 1).sum::<u64>() <= 8 << 20);
    assert_eq!(l.last().unwrap(), "LAST");
}

#[test]
fn tail_from_reads_new_lines_and_survives_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("proxy.log");
    std::fs::write(
        &p, "a
b
c
",
    )
    .unwrap();
    let off = size(&p).unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(
        b"d
e
",
    )
    .unwrap();
    assert_eq!(tail_from(&p, off, 1 << 20).unwrap().unwrap(), ["d", "e"]);
    std::fs::write(
        &p, "x
",
    )
    .unwrap();
    assert_eq!(tail_from(&p, off, 1 << 20).unwrap().unwrap(), ["x"]);
    assert!(
        tail_from(&dir.path().join("none"), 0, 10)
            .unwrap()
            .is_none()
    );
}

fn voice_of(lines: &[&str]) -> discord::Voice {
    let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    discord::parse(&lines, None)
}

#[test]
fn discord_dave_disabled_reconnect_and_rekey() {
    const AT: &str = "[2026-10-18 10:00:00.000] [info]  ";
    let line = |s: &str| format!("{AT}{s}");
    let disabled =
        line("[RTCConnection(1, default)] DAVE protocol init with protocol version: 0, foo");
    assert_eq!(voice_of(&[&disabled]).dave, discord::Dave::Disabled);
    // A new connection starts over: the old peers and address go.
    let v = voice_of(&[
        &line("[Connection(default)] Creating user: 5 with audio SSRC: 42"),
        &line("[Connection(default)] Connected with local address 1.2.3.4:5000 and protocol: udp"),
        &line("[Connection(default)] Creating connection to c.discord.media with audio ssrc: 7"),
        &line(
            "[Connection(default)] Connected with local address [2001:db8::1]:5000 and protocol: udp",
        ),
    ]);
    assert!(v.peers.is_empty(), "{v:?}");
    assert_eq!(v.own_ssrc, Some(7));
    assert_eq!(v.local_address.as_deref(), Some("2001:db8::1"));
    let bare = voice_of(&[&line(
        "[Connection(default)] Connected with local address 2001:db8::2 and protocol: udp",
    )]);
    assert_eq!(bare.local_address.as_deref(), Some("2001:db8::2"));
    // A key package after the group was made: a new handshake, waiting again.
    let v = voice_of(&[
        &line("[RTCControlSocket(default)] Sending MLS key package"),
        &line("[RTCControlSocket(default)] Received MLS commit"),
        &line("[RTCControlSocket(default)] Sending MLS key package"),
    ]);
    assert_eq!(v.dave, discord::Dave::KeyPackageSent { secs_waiting: 0 });
}

#[test]
fn tail_from_a_mark_mid_line_skips_the_rest_of_that_line() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("renderer_js.log");
    std::fs::write(&p, "old\npart").unwrap();
    let mark = size(&p).unwrap();
    // The log stays open for writing, as Discord keeps it.
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"ial\nnew\n").unwrap();
    assert_eq!(tail_from(&p, mark, 1 << 20).unwrap().unwrap(), ["new"]);
    let whole = size(&p).unwrap();
    f.write_all(b"next\n").unwrap();
    assert_eq!(tail_from(&p, whole, 1 << 20).unwrap().unwrap(), ["next"]);
}

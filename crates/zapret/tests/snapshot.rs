use std::fs;
use std::path::PathBuf;

use zapret::procs::Proc;
use zapret::snapshot::{self, RouterSnap, ServiceSnap, Snapshot, pending_path};

fn sample() -> Snapshot {
    Snapshot {
        operation: "tests".into(),
        service2: Some(ServiceSnap {
            image_path: r#""D:\zapret2\winws2.exe" --chdir="D:\zapret2" --wf-tcp-out=443"#.into(),
            auto_start: true,
            running: false,
        }),
        strategy2: Some("zapret2:mine".into()),
        manual_winws2: vec![Proc {
            pid: 43,
            exe: Some(PathBuf::from(r"D:\zapret2\winws2.exe")),
            args: vec!["--wf-tcp-out=443".into()],
        }],
        zapret2: true,
        router: None,
    }
}

#[test]
fn pending_json_round_trips() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    assert_eq!(Snapshot::load(&path).unwrap(), None);
    sample().save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(sample()));
    Snapshot::clear(&path).unwrap();
    Snapshot::clear(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), None);
}

#[test]
fn a_pending_json_from_before_zapret2_still_loads_and_restores_nothing() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{
  "operation": "apply",
  "service": {
    "image_path": "\"E:\\Zapret\\bin\\winws.exe\"   --wf-tcp 80",
    "auto_start": true,
    "running": true
  },
  "strategy": "general (ALT11)",
  "manual_winws": [],
  "ipset": "loaded",
  "ipset_backup": false,
  "game_filter": {
    "mode": "tcp",
    "tcp": "2000-2100",
    "udp": "1024-65535"
  }
}"#,
    )
    .unwrap();
    let snap = Snapshot::load(&path).unwrap().unwrap();
    assert_eq!(snap.operation, "apply");
    assert_eq!(snap.service2, None);
    assert_eq!(snap.strategy2, None);
    assert!(snap.manual_winws2.is_empty());
    assert!(!snap.zapret2, "its restore leaves zapret2 alone");
    snap.restore().unwrap();
}

#[test]
fn a_pending_json_of_0_8_0_loads_without_its_flowseal_part() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{
  "operation": "tests",
  "service": null,
  "strategy": null,
  "manual_winws": [],
  "service2": null,
  "strategy2": "zapret2:general",
  "manual_winws2": [],
  "zapret2": true,
  "ipset": null,
  "ipset_backup": false,
  "game_filter": null,
  "router": { "enabled": true, "paused": true }
}"#,
    )
    .unwrap();
    let snap = Snapshot::load(&path).unwrap().unwrap();
    assert_eq!(snap.strategy2.as_deref(), Some("zapret2:general"));
    assert!(snap.zapret2);
    assert_eq!(
        snap.router,
        Some(RouterSnap {
            enabled: true,
            paused: true
        })
    );
}

#[test]
fn old_pending_without_router_loads() {
    let json = serde_json::to_string(&sample()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("router");
    let snap: Snapshot = serde_json::from_value(value).unwrap();
    assert_eq!(snap.router, None);
}

#[test]
fn router_section_roundtrips() {
    let snap = Snapshot {
        router: Some(RouterSnap {
            enabled: true,
            paused: false,
        }),
        ..sample()
    };
    let back: Snapshot = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
    assert_eq!(back, snap);
}

#[test]
fn recover_restores_router_section() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    let snap = Snapshot {
        router: Some(RouterSnap {
            enabled: true,
            paused: true,
        }),
        ..sample()
    };
    snap.save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(snap));
}

#[test]
fn a_pending_json_that_cannot_be_written_is_an_error_naming_it() {
    let data = tempfile::tempdir().unwrap();
    fs::write(data.path().join("state"), "").unwrap(); // a file where its folder goes
    let err = sample().save(&pending_path(data.path())).unwrap_err();
    assert!(format!("{err:#}").contains("pending.json"), "{err:#}");
}

#[test]
fn a_damaged_pending_json_is_an_error_naming_the_file() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{").unwrap();
    let err = Snapshot::load(&path).unwrap_err().to_string();
    assert!(err.contains("pending.json"), "{err}");
}

// `begin` restores an earlier pending.json first; that touches the services, so only the paths
// that restore nothing are tested here.
#[test]
fn begin_with_nothing_pending_takes_and_saves_a_snapshot() {
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());

    let snap = snapshot::begin("tests", &pending).unwrap();
    assert_eq!(snap.operation, "tests");
    assert!(snap.zapret2, "a snapshot taken now records zapret2");
    assert_eq!(Snapshot::load(&pending).unwrap(), Some(snap));
}

#[test]
fn begin_does_not_start_over_a_damaged_pending_json() {
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    fs::create_dir_all(pending.parent().unwrap()).unwrap();
    fs::write(&pending, "{").unwrap();

    let err = snapshot::begin("tests", &pending).unwrap_err().to_string();
    assert!(err.contains("pending.json"), "{err}");
    assert_eq!(
        fs::read_to_string(&pending).unwrap(),
        "{",
        "the file stays for whoever looks into it"
    );
}

#[test]
fn begin_says_how_to_get_past_a_pending_json_it_cannot_read() {
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    fs::create_dir_all(&pending).unwrap(); // reading it fails with Access denied
    let err = format!("{:#}", snapshot::begin("tests", &pending).unwrap_err());
    for part in ["pending.json", "did not start", "Deleting"] {
        assert!(err.contains(part), "{part}: {err}");
    }
    let err = format!("{:#}", Snapshot::clear(&pending).unwrap_err());
    assert!(err.contains("pending.json"), "{err}");
}

#[test]
#[ignore = "needs admin rights and the zapret2 service installed; restarts zapret2"]
fn live_begin_restores_a_pending_json_first() {
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    // What an interrupted operation would have left: this machine's state as it is now.
    let earlier = Snapshot::take("apply").unwrap();
    earlier.save(&pending).unwrap();

    let snap = snapshot::begin("tests", &pending).unwrap();
    assert_eq!(snap.operation, "tests");
    assert_eq!(
        snap.service2, earlier.service2,
        "the service came back as it was"
    );
    assert_eq!(snap.strategy2, earlier.strategy2);
    assert_eq!(Snapshot::load(&pending).unwrap(), Some(snap));
}

#[test]
#[ignore = "needs admin rights and the zapret2 service installed"]
fn live_snapshot_of_this_machine() {
    let snap = Snapshot::take("probe").unwrap();
    eprintln!("{snap:#?}");
    assert_eq!(snap.operation, "probe");
    let service = snap
        .service2
        .as_ref()
        .expect("the zapret2 service is installed");
    assert!(
        service.image_path.to_lowercase().contains("winws2.exe"),
        "{}",
        service.image_path
    );
    assert!(
        snap.strategy2.is_some(),
        "the app records the installed strategy in the registry"
    );
    // What a crash leaves behind reads back as it was taken.
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    snap.save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(snap));
}

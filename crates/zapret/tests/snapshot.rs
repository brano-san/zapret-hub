mod common;

use std::fs;
use std::path::PathBuf;

use zapret::ipset::IpsetMode;
use zapret::procs::Proc;
use zapret::snapshot::{ServiceSnap, Snapshot, pending_path};

fn sample(ipset: IpsetMode) -> Snapshot {
    Snapshot {
        operation: "tests".into(),
        service: Some(ServiceSnap {
            image_path: r#""E:\Zapret\bin\winws.exe"   --wf-tcp 80"#.into(),
            auto_start: true,
            running: true,
        }),
        strategy: Some("general (ALT11)".into()),
        manual_winws: vec![Proc {
            pid: 42,
            exe: Some(PathBuf::from(r"E:\Zapret\bin\winws.exe")),
            args: vec!["--wf-tcp=80".into()],
        }],
        ipset,
        ipset_backup: false,
    }
}

#[test]
fn pending_json_round_trips() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    assert_eq!(Snapshot::load(&path).unwrap(), None);
    sample(IpsetMode::Loaded).save(&path).unwrap();
    assert_eq!(
        Snapshot::load(&path).unwrap(),
        Some(sample(IpsetMode::Loaded))
    );
    Snapshot::clear(&path).unwrap();
    Snapshot::clear(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), None);
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

#[test]
fn restore_files_returns_the_ipset_after_a_killed_run() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    let snap = sample(IpsetMode::Loaded);

    zapret::ipset::enter_test_any(&folder).unwrap(); // the app was killed during the DPI suite
    snap.restore_files(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");

    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap(); // a mode switch that failed later
    snap.restore_files(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
}

#[test]
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret"]
fn live_snapshot_of_this_machine() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let snap = Snapshot::take(&folder, "probe");
    eprintln!("{snap:#?}");
    assert_eq!(snap.operation, "probe");
    let service = snap
        .service
        .as_ref()
        .expect("the zapret service is installed");
    assert!(
        service.image_path.to_lowercase().contains("winws.exe"),
        "{}",
        service.image_path
    );
    assert!(
        snap.strategy.is_some(),
        "service.bat records the installed strategy in the registry"
    );
    // What a crash leaves behind reads back as it was taken.
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    snap.save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(snap));
}

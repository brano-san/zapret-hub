mod common;

use std::fs;
use std::path::PathBuf;

use zapret::game_filter::{self, GameFilter, GameFilterMode};
use zapret::ipset::IpsetMode;
use zapret::procs::Proc;
use zapret::snapshot::{self, ServiceSnap, Snapshot, pending_path};

fn filter(mode: GameFilterMode, tcp: &str, udp: &str) -> GameFilter {
    GameFilter {
        mode,
        tcp: tcp.into(),
        udp: udp.into(),
    }
}

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
        ipset: Some(ipset),
        ipset_backup: false,
        game_filter: Some(filter(GameFilterMode::Tcp, "2000-2100", "1024-65535")),
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
fn a_pending_json_from_before_zapret2_still_loads() {
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
    assert_eq!(snap.strategy.as_deref(), Some("general (ALT11)"));
    assert_eq!(snap.service2, None);
    assert_eq!(snap.strategy2, None);
    assert!(snap.manual_winws2.is_empty());
    assert!(!snap.zapret2, "its restore leaves zapret2 alone");
}

#[test]
fn a_pending_json_that_cannot_be_written_is_an_error_naming_it() {
    let data = tempfile::tempdir().unwrap();
    fs::write(data.path().join("state"), "").unwrap(); // a file where its folder goes
    let err = sample(IpsetMode::Loaded)
        .save(&pending_path(data.path()))
        .unwrap_err();
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
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());

    let snap = snapshot::begin(Some(&folder), "tests", &pending).unwrap();
    assert_eq!(snap.operation, "tests");
    assert!(snap.zapret2, "a snapshot taken now records zapret2");
    assert_eq!(Snapshot::load(&pending).unwrap(), Some(snap));
}

#[test]
fn begin_does_not_start_over_a_damaged_pending_json() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    fs::create_dir_all(pending.parent().unwrap()).unwrap();
    fs::write(&pending, "{").unwrap();

    let err = snapshot::begin(Some(&folder), "tests", &pending)
        .unwrap_err()
        .to_string();
    assert!(err.contains("pending.json"), "{err}");
    assert_eq!(
        fs::read_to_string(&pending).unwrap(),
        "{",
        "the file stays for whoever looks into it"
    );
}

#[test]
fn begin_says_how_to_get_past_a_pending_json_it_cannot_read() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    fs::create_dir_all(&pending).unwrap(); // reading it fails with Access denied
    let err = format!(
        "{:#}",
        snapshot::begin(Some(&folder), "tests", &pending).unwrap_err()
    );
    for part in ["pending.json", "did not start", "Deleting"] {
        assert!(err.contains(part), "{part}: {err}");
    }
    let err = format!("{:#}", Snapshot::clear(&pending).unwrap_err());
    assert!(err.contains("pending.json"), "{err}");
}

#[test]
fn begin_does_not_start_without_the_whole_state() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    let ipset = dir.path().join("lists").join("ipset-all.txt");
    let filter = dir.path().join("utils").join("game_filter.enabled");
    for (file, text) in [(&ipset, "1.1.1.0/24\r\n"), (&filter, "mode=all\r\n")] {
        fs::write(file, text).unwrap();
        let lock = common::lock_reads(file);
        let began = snapshot::begin(Some(&folder), "ipset", &pending);
        drop(lock);
        let err = format!("{:#}", began.unwrap_err());
        assert!(err.contains(&file.display().to_string()), "{err}");
        assert!(!pending.exists(), "a snapshot of a state not read whole");
    }
}

#[test]
fn restore_files_returns_the_ipset_after_a_killed_run() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    let snap = sample(IpsetMode::Loaded);

    zapret::ipset::enter_test_any(&folder).unwrap(); // the app was killed during the DPI suite
    snap.restore_files(Some(&folder)).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");

    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap(); // a mode switch that failed later
    snap.restore_files(Some(&folder)).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
}

#[test]
fn restore_files_returns_the_game_filter_a_failed_change_left_behind() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let before = filter(GameFilterMode::Tcp, "2000-2100", "1024-65535");
    game_filter::write(&folder, &before).unwrap();
    let snap = Snapshot::take(Some(&folder), "game_filter").unwrap();
    assert_eq!(snap.game_filter.as_ref(), Some(&before));

    // The change writes the file first and reinstalls the service after it, which failed.
    game_filter::write(&folder, &filter(GameFilterMode::Udp, "1024-65535", "3000")).unwrap();
    snap.restore_files(Some(&folder)).unwrap();
    assert_eq!(game_filter::read(&folder), before);
}

#[test]
fn without_a_flowseal_folder_only_recorded_files_need_one() {
    let alone = Snapshot {
        ipset: None,
        ipset_backup: false,
        game_filter: None,
        ..sample(IpsetMode::Loaded)
    };
    alone.restore_files(None).unwrap();
    let err = sample(IpsetMode::Loaded).restore_files(None).unwrap_err();
    assert!(
        format!("{err:#}").contains("zapret folder is not set"),
        "{err:#}"
    );
    // A folder chosen since is left alone: the snapshot holds nothing of it.
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap();
    alone.restore_files(Some(&folder)).unwrap();
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::None);
}

#[test]
fn restore_files_leaves_a_game_filter_file_that_reads_the_same() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let file = dir.path().join("utils").join("game_filter.enabled");
    let by_hand = "; my ports\r\nmode=all\r\ntcp=2000\r\n";
    fs::write(&file, by_hand).unwrap();
    let snap = Snapshot::take(Some(&folder), "tests").unwrap();

    snap.restore_files(Some(&folder)).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), by_hand);
}

#[test]
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret; restarts zapret"]
fn live_begin_restores_a_pending_json_first() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let pending = pending_path(data.path());
    // What an interrupted operation would have left: this machine's state as it is now.
    let earlier = Snapshot::take(Some(&folder), "apply").unwrap();
    earlier.save(&pending).unwrap();

    let snap = snapshot::begin(Some(&folder), "tests", &pending).unwrap();
    assert_eq!(snap.operation, "tests");
    assert_eq!(
        snap.service, earlier.service,
        "the service came back as it was"
    );
    assert_eq!(snap.strategy, earlier.strategy);
    assert_eq!(snap.service2, earlier.service2);
    assert_eq!(snap.strategy2, earlier.strategy2);
    assert_eq!(snap.ipset, earlier.ipset);
    assert_eq!(Snapshot::load(&pending).unwrap(), Some(snap));
}

#[test]
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret"]
fn live_snapshot_of_this_machine() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let snap = Snapshot::take(Some(&folder), "probe").unwrap();
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

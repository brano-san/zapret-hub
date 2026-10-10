mod common;

use std::path::PathBuf;

use zapret::folder::{Folder, root_from_image_path};

#[test]
fn opens_a_complete_folder_and_reads_its_version() {
    let dir = common::fake_zapret();
    let folder = Folder::open(dir.path()).unwrap();
    assert_eq!(folder.version().as_deref(), Some("1.10.3"));
    assert!(folder.root_dp0().ends_with('\\'));
    assert_eq!(folder.winws(), dir.path().join("bin").join("winws.exe"));
}

#[test]
fn names_everything_that_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let err = Folder::open(dir.path()).unwrap_err().to_string();
    for part in ["service.bat", "winws.exe", "WinDivert64.sys", "lists"] {
        assert!(err.contains(part), "{err}");
    }
    let gone = dir.path().join("moved-away");
    assert!(Folder::open(&gone).is_err());
}

#[test]
fn root_from_the_service_image_path() {
    assert_eq!(
        root_from_image_path(r#""E:\Zapret\bin\winws.exe"   --wf-tcp 80"#),
        Some(PathBuf::from(r"E:\Zapret"))
    );
    assert_eq!(
        root_from_image_path(r"C:\z\bin\winws.exe --x"),
        Some(PathBuf::from(r"C:\z"))
    );
}

#[test]
fn strategies_come_in_service_bat_order() {
    let dir = common::fake_zapret();
    for name in [
        "general (ALT10).bat",
        "general (ALT2).bat",
        "general.bat",
        "general (ALT).bat",
        "service.bat",
        "Service backup.bat",
        "general (SIMPLE FAKE).bat",
        "general (SIMPLE FAKE ALT).bat",
        "notes.txt",
    ] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    let names: Vec<String> = zapret::strategies::list(&common::folder(&dir))
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(
        names,
        [
            "general (ALT)",
            "general (ALT2)",
            "general (ALT10)",
            "general (SIMPLE FAKE ALT)",
            "general (SIMPLE FAKE)",
            "general"
        ]
    );
    assert!(zapret::strategies::find(&common::folder(&dir), "GENERAL (alt2)").is_ok());
    assert!(zapret::strategies::find(&common::folder(&dir), "missing").is_err());
}

#[test]
fn natural_key_pads_numbers_like_service_bat() {
    assert_eq!(
        zapret::strategies::natural_key("general (ALT10).bat"),
        "general (alt00000010).bat"
    );
    assert_eq!(zapret::strategies::natural_key("a123456789"), "a123456789");
}

#[test]
fn parses_targets_like_the_ps1() {
    let text = concat!(
        "# comment = \"x\"\n",
        "DiscordMain = \"https://discord.com\"\n",
        "Bad Key = \"https://x\"\n",
        "Ping1 = \"PING: 1.1.1.1\"\n",
        "DiscordMain = \"https://discord.com/app\"\n",
        "Empty = \"\"\n",
    );
    let targets = zapret::targets::parse(text);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].name, "DiscordMain");
    assert_eq!(targets[0].url.as_deref(), Some("https://discord.com/app"));
    assert_eq!(targets[0].ping, "discord.com");
    assert_eq!(targets[1].url, None);
    assert_eq!(targets[1].ping, "1.1.1.1");
}

#[test]
fn without_targets_txt_the_ps1_defaults_are_used() {
    let dir = common::fake_zapret();
    let targets = zapret::targets::load(&common::folder(&dir));
    assert_eq!(targets.len(), 17);
    assert_eq!(targets.iter().filter(|t| t.url.is_some()).count(), 12);
    std::fs::write(
        dir.path().join("utils").join("targets.txt"),
        "A = \"https://a.example\"\r\n",
    )
    .unwrap();
    assert_eq!(zapret::targets::load(&common::folder(&dir)).len(), 1);
}

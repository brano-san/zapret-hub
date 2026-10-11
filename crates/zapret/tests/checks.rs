use zapret::checks::{
    conflicts, has_cyrillic, hosts_mentions_youtube, in_onedrive, matching_services,
};
use zapret::discord_cache::{CacheOutcome, clear_in};

#[test]
fn a_service_matches_when_one_line_has_every_word() {
    let services = vec![
        (
            "ICNS".to_owned(),
            "Intel(R) Connectivity Network Service".to_owned(),
        ),
        ("NetSvc".to_owned(), "Intel Connectivity Helper".to_owned()),
        (
            "KillerNetworkService".to_owned(),
            "Killer Network Service".to_owned(),
        ),
    ];
    assert_eq!(
        matching_services(&services, &["intel", "connectivity", "network"]),
        ["ICNS"]
    );
    assert_eq!(
        matching_services(&services, &["killer"]),
        ["KillerNetworkService"]
    );
    assert!(matching_services(&services, &["vpn"]).is_empty());
}

#[test]
fn zapret2_is_a_conflict_only_next_to_zapret() {
    let installed = |names: &'static [&'static str]| move |name: &str| names.contains(&name);
    assert!(conflicts(installed(&["zapret2"])).is_empty());
    assert!(conflicts(installed(&["zapret"])).is_empty());
    assert_eq!(conflicts(installed(&["zapret", "zapret2"])), ["zapret2"]);
    assert_eq!(
        conflicts(installed(&["zapret2", "winws2", "zapret", "GoodbyeDPI"])),
        ["GoodbyeDPI", "winws2", "zapret2"]
    );
}

#[test]
fn cyrillic_and_onedrive_paths_are_spotted() {
    assert!(has_cyrillic(r"C:\Запрет\"));
    assert!(has_cyrillic("ёлка"));
    assert!(!has_cyrillic(r"E:\Zapret\"));
    assert!(in_onedrive(
        r"C:\Users\me\OneDrive\zapret\",
        Some(r"C:\Users\me\OneDrive")
    ));
    assert!(in_onedrive(
        r"c:\users\me\onedrive\",
        Some(r"C:\Users\me\OneDrive\")
    ));
    assert!(!in_onedrive(
        r"C:\Users\me\OneDriveBackup\z\",
        Some(r"C:\Users\me\OneDrive")
    ));
    assert!(!in_onedrive(r"E:\Zapret\", None));
}

#[test]
fn youtube_is_found_in_a_hosts_file_that_is_not_utf8() {
    // `findstr` searches raw bytes: a CP1251 comment ("Привет") must not hide an entry.
    let comment: &[u8] = b"# \xcf\xf0\xe8\xe2\xe5\xf2\r\n";
    assert!(String::from_utf8(comment.to_vec()).is_err());
    let upper_case = [comment, b"127.0.0.1 YouTube.com\r\n"].concat();
    let short_link = [comment, b"0.0.0.0 youtu.be\r\n"].concat();
    let neither = [comment, b"127.0.0.1 localhost\r\n"].concat();
    assert!(hosts_mentions_youtube(&upper_case));
    assert!(hosts_mentions_youtube(&short_link));
    assert!(!hosts_mentions_youtube(&neither));
}

#[test]
fn discord_cache_is_cleared_for_installed_builds_only() {
    let appdata = tempfile::tempdir().unwrap();
    let discord = appdata.path().join("discord");
    std::fs::create_dir_all(discord.join("Cache").join("Cache_Data")).unwrap();
    std::fs::write(
        discord.join("Cache").join("Cache_Data").join("f_000001"),
        "x",
    )
    .unwrap();
    std::fs::create_dir_all(discord.join("GPUCache")).unwrap();
    let mut asked = Vec::new();
    let reports = clear_in(appdata.path(), |image| {
        asked.push(image.to_owned());
        false
    });
    assert_eq!(asked, ["Discord.exe"]);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].install, "Discord");
    let outcomes: Vec<CacheOutcome> = reports[0].dirs.iter().map(|d| d.outcome).collect();
    assert_eq!(
        outcomes,
        [
            CacheOutcome::Deleted,
            CacheOutcome::Missing,
            CacheOutcome::Deleted
        ]
    );
    assert!(!discord.join("Cache").exists());
}

#[test]
#[ignore = "reads this machine's services, registry and E:\\Zapret"]
fn live_checks_of_this_machine() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let checks = zapret::checks::run_all(&folder);
    for check in &checks {
        eprintln!("{check:?}");
    }
    // Every diagnostic of service.bat, once each.
    let ids: Vec<&zapret::checks::CheckId> = checks.iter().map(|check| &check.id).collect();
    assert_eq!(ids.len(), 16, "{ids:?}");
    for (i, id) in ids.iter().enumerate() {
        assert!(!ids[i + 1..].contains(id), "{id:?} twice");
    }
    // zapret works on this machine, so the filtering engine runs.
    assert!(
        checks
            .iter()
            .any(|c| c.id == zapret::checks::CheckId::Bfe
                && c.level == zapret::checks::CheckLevel::Ok)
    );
}

//! The Rust parser against `service.bat` itself: fixtures from `tools\gen-bat-fixtures.cmd`.

use std::path::Path;

use zapret::batparse::{GameFilterVars, parse};

#[test]
fn final_args_match_service_bat_byte_for_byte() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let root = std::fs::read_to_string(dir.join("ROOT.txt"))
        .expect("run tools\\gen-bat-fixtures.cmd first");
    let root = root.trim_end();
    let vars = GameFilterVars {
        all: "1024-65535".into(),
        tcp: "1024-65535".into(),
        udp: "1024-65535".into(),
    };
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bat"))
        {
            continue;
        }
        let bat = zapret::read_text(&path).unwrap();
        let expected = zapret::read_text(&path.with_extension("args")).unwrap();
        let expected = expected.trim_end_matches(['\r', '\n']);
        let parsed = parse(&bat, root, &vars);
        if !expected.contains("--") {
            // service.bat captured nothing: not a strategy (e.g. a helper .bat in the root).
            assert!(parsed.is_err(), "{} should not parse", path.display());
            continue;
        }
        let parsed = parsed.unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        assert_eq!(
            format!("Final args: {}", parsed.final_args),
            expected,
            "{}",
            path.display()
        );
        compared += 1;
    }
    assert!(compared >= 20, "only {compared} strategies compared");
}

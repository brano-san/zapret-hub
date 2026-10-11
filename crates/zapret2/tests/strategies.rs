mod common;

use std::fs;
use std::path::Path;

use zapret::game_filter::{GameFilter, GameFilterMode};
use zapret2::Engine;
use zapret2::strategies::{self, args, check, claim, split, tail};

fn engine(root: &Path) -> Engine {
    common::fake_engine(root, "", common::LIB);
    Engine::open(root).unwrap()
}

fn names(data: &Path) -> Vec<(String, bool)> {
    strategies::list(data)
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.builtin))
        .collect()
}

#[test]
fn built_in_ones_come_first_then_the_users_in_natural_order() {
    let data = tempfile::tempdir().unwrap();
    let own = strategies::dir(data.path());
    fs::create_dir_all(&own).unwrap();
    for file in ["b 10.txt", "b 9.txt", "a.txt", "General.txt", "notes.md"] {
        fs::write(own.join(file), "--wf-tcp-out=443").unwrap();
    }
    fs::write(own.join("bom.txt"), "\u{feff}--wf-tcp-out=80").unwrap();
    let expected: Vec<(String, bool)> = [
        ("general", true),
        ("general-sni", true),
        ("a", false),
        ("b 9", false),
        ("b 10", false),
        ("bom", false),
    ]
    .iter()
    .map(|(name, builtin)| (name.to_string(), *builtin))
    .collect();
    assert_eq!(names(data.path()), expected);
    let bom = strategies::find(data.path(), "BOM").unwrap();
    assert_eq!(bom.text, "--wf-tcp-out=80");
    assert_eq!(bom.full_name(), "zapret2:bom");
    assert!(strategies::find(data.path(), "c").is_err());
}

#[test]
fn without_a_folder_there_are_the_built_in_ones() {
    let data = tempfile::tempdir().unwrap();
    assert_eq!(
        names(data.path()),
        [
            ("general".to_owned(), true),
            ("general-sni".to_owned(), true)
        ]
    );
}

#[test]
fn a_save_claims_a_new_name_or_one_of_the_users() {
    let data = tempfile::tempdir().unwrap();
    let err =
        |name: &str, create: bool| format!("{:#}", claim(data.path(), name, create).unwrap_err());
    assert!(err("General", true).contains("built in"));
    assert!(err("mine", false).contains("no zapret2 strategy of yours"));
    assert!(err("a/b", true).contains("1-40"));
    assert_eq!(claim(data.path(), " mine ", true).unwrap(), "mine");
    strategies::write(data.path(), "mine", "--wf-tcp-out=443\n").unwrap();
    assert!(err("MINE", true).contains("taken"));
    assert_eq!(claim(data.path(), "mine", false).unwrap(), "mine");
}

#[test]
fn only_the_users_strategies_that_are_not_active_are_deleted() {
    let data = tempfile::tempdir().unwrap();
    strategies::write(data.path(), "mine", "--wf-tcp-out=443\n").unwrap();
    let err = |name: &str, active: Option<&str>| {
        format!(
            "{:#}",
            strategies::delete(data.path(), name, active).unwrap_err()
        )
    };
    assert!(err("general", None).contains("built in"));
    assert!(err("mine", Some("ZAPRET2:Mine")).contains("active"));
    strategies::delete(data.path(), "Mine", Some("zapret2:general")).unwrap();
    assert!(!strategies::dir(data.path()).join("mine.txt").exists());
}

#[test]
fn comment_lines_go_and_zapret2_hashes_stay() {
    let text = "# a comment\r\n--wf-tcp-out=443\r\n\r\n  # indented comment\n--lua-desync=fake:blob=x:tls_mod=sni=%var:repeats=#var\n";
    assert_eq!(
        args(text),
        "--wf-tcp-out=443 --lua-desync=fake:blob=x:tls_mod=sni=%var:repeats=#var"
    );
}

#[test]
fn a_byte_order_mark_does_not_turn_the_first_comment_into_arguments() {
    // `trim` keeps U+FEFF, so a text with a BOM (the editor, the CLI) must lose it in `args`.
    assert_eq!(args("\u{feff}# comment\n--x"), "--x");
    assert_eq!(args("\u{feff}--x\n# comment"), "--x");
}

#[test]
fn a_file_that_cannot_be_read_is_skipped_and_the_rest_still_works() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let data = tempfile::tempdir().unwrap();
    for name in ["mine", "locked"] {
        strategies::write(data.path(), name, "--wf-tcp-out=443\n").unwrap();
    }
    // Held with no sharing, as an editor or a scanner may: every other open fails (os error 32).
    let locked = strategies::dir(data.path()).join("locked.txt");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    assert!(fs::read(&locked).is_err());
    // The built-in ones, then these of the user's.
    let listed = |users: &[&str]| -> Vec<(String, bool)> {
        ["general", "general-sni"]
            .iter()
            .map(|name| (name.to_string(), true))
            .chain(users.iter().map(|name| (name.to_string(), false)))
            .collect()
    };
    assert_eq!(names(data.path()), listed(&["mine"]));
    strategies::delete(data.path(), "mine", None).unwrap();
    assert_eq!(names(data.path()), listed(&[]));
    // Skipped, not forgotten: it is listed again once it can be read.
    drop(held);
    assert_eq!(names(data.path()), listed(&["locked"]));
}

#[test]
fn the_command_line_fills_every_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    // Spaces as in `C:\Program Files\…`: every path must stay one quoted argument.
    let engine = engine(&dir.path().join("zapret 2"));
    let flowseal = dir.path().join("Flowseal zapret");
    let folder = common::fake_flowseal(&flowseal, &[]);
    let text = "--wf-tcp-out=443,%GameFilterTCP% --wf-udp-out=443,%GameFilterUDP%\n\
                --lua-init=@\"%LUA%zapret-lib.lua\" --blob=a:@\"%FILES%fake\\a.bin\"\n\
                --hostlist=\"%LISTS%list-general.txt\" --blob=b:@\"%bin%stun.bin\" --filter-tcp=%GameFilter%";
    let root = engine.root().display().to_string();
    let lists = flowseal.join("lists").display().to_string();
    let bin = flowseal.join("bin").display().to_string();
    let line = tail(&engine, Some(&folder), text).unwrap();
    assert_eq!(
        line,
        format!(
            "--chdir=\"{root}\" --wf-tcp-out=443,12 --wf-udp-out=443,12 \
             --lua-init=@\"{root}\\lua\\zapret-lib.lua\" --blob=a:@\"{root}\\files\\fake\\a.bin\" \
             --hostlist=\"{lists}\\list-general.txt\" --blob=b:@\"{bin}\\stun.bin\" --filter-tcp=12"
        )
    );
    let tokens = zapret::custom::quoted_tokens(&line);
    assert_eq!(tokens.len(), 8, "{tokens:?}");
    assert_eq!(tokens[0], format!("--chdir=\"{root}\""));
    assert_eq!(
        tokens[5],
        format!("--hostlist=\"{lists}\\list-general.txt\"")
    );
    zapret::game_filter::write(
        &folder,
        &GameFilter {
            mode: GameFilterMode::Tcp,
            tcp: "1024-65535".into(),
            udp: "1024-65535".into(),
        },
    )
    .unwrap();
    let filtered = tail(&engine, Some(&folder), text).unwrap();
    assert!(
        filtered.contains("--wf-tcp-out=443,1024-65535 --wf-udp-out=443,12"),
        "{filtered}"
    );
    assert!(filtered.ends_with("--filter-tcp=1024-65535"), "{filtered}");
}

#[test]
fn flowseal_placeholders_need_the_flowseal_folder() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let err = format!(
        "{:#}",
        tail(&engine, None, "--hostlist=\"%lists%list-general.txt\"").unwrap_err()
    );
    assert!(
        err.contains("%LISTS%") && err.contains("zapret folder"),
        "{err}"
    );
    let own = tail(
        &engine,
        None,
        "--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"",
    )
    .unwrap();
    assert!(own.starts_with(&format!("--chdir=\"{}\" ", engine.root().display())));
}

/// A lenient first pass, not winws2's own option parsing: step 3 (`--intercept=0`) is the real
/// check.
#[test]
fn the_static_check_looks_for_each_desync_function_in_the_lua_files_and_inits() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    fs::write(
        engine.lua().join("zapret-antidpi.lua"),
        "function fake(ctx, desync)\nend\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("mine.lua"),
        "function mine(ctx, desync) end",
    )
    .unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let theirs = elsewhere.path().join("theirs.lua");
    fs::write(&theirs, "function theirs(ctx, desync) end").unwrap();
    let ok = format!(
        "--chdir=\"{}\" --lua-init=@mine.lua --lua-init=@\"{}\" \
         --lua-init=\"function inline(ctx, desync) end\" --lua-desync=fake:blob=x \
         --new --lua-desync=mine --lua-desync=theirs:x=1 --lua-desync=inline",
        engine.root().display(),
        theirs.display()
    );
    check(&engine, &ok).unwrap();
    let err = format!(
        "{:#}",
        check(
            &engine,
            "--lua-desync=fakee:blob=x --lua-desync=multisplit:pos=1 --lua-desync=fakee"
        )
        .unwrap_err()
    );
    assert!(err.contains("fakee, multisplit"), "{err}");
    let err = format!(
        "{:#}",
        check(&engine, "--lua-init=@missing.lua --lua-desync=fake").unwrap_err()
    );
    assert!(err.contains("missing.lua"), "{err}");
}

#[test]
fn a_quote_before_the_at_sign_still_names_a_lua_init_file() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let spaced = dir.path().join("My Lua");
    fs::create_dir_all(&spaced).unwrap();
    let file = spaced.join("x.lua");
    fs::write(&file, "function spaced(ctx, desync) end").unwrap();
    // A file with a space, whichever side of the `@` the quote is on: not inline code.
    for init in [
        format!("--lua-init=\"@{}\"", file.display()),
        format!("--lua-init=@\"{}\"", file.display()),
    ] {
        check(&engine, &format!("{init} --lua-desync=spaced:x=1")).unwrap();
    }
    let err = format!(
        "{:#}",
        check(&engine, "--lua-init=\"@missing.lua\"").unwrap_err()
    );
    assert!(err.contains("missing.lua"), "{err}");
}

#[test]
fn a_quoted_desync_option_is_checked_like_an_unquoted_one() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let err = format!(
        "{:#}",
        check(
            &engine,
            "\"--lua-desync=fakee:blob=x\" --lua-desync=\"multisplit:pos=1\""
        )
        .unwrap_err()
    );
    assert!(err.contains("fakee, multisplit"), "{err}");
}

#[test]
fn a_backslash_before_a_quote_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let err = format!(
        "{:#}",
        check(
            &engine,
            r#"--hostlist="E:\Zapret\lists\" --lua-desync=fake"#
        )
        .unwrap_err()
    );
    assert!(err.contains(r#"\""#), "{err}");
}

#[test]
fn the_card_shows_the_global_part_and_each_profile() {
    let (global, profiles) = split(
        "--wf-tcp-out=443 --lua-init=@\"a b.lua\" --blob=x:0x00 --filter-tcp=443 --lua-desync=fake \
         --new --filter-udp=443 --lua-desync=fake:blob=x --new=last --filter-l7=stun --new",
    );
    assert_eq!(
        global,
        "--wf-tcp-out=443 --lua-init=@\"a b.lua\" --blob=x:0x00"
    );
    assert_eq!(
        profiles,
        [
            "--filter-tcp=443 --lua-desync=fake",
            "--filter-udp=443 --lua-desync=fake:blob=x",
            "--filter-l7=stun"
        ]
    );
}

/// The built-in strategies against zapret2's function list and Flowseal 1.10.3's files.
#[test]
fn every_built_in_strategy_passes_the_static_check_and_names_files_that_exist() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("zapret2");
    common::fake_engine(&root, "", &common::lib_with_functions());
    let engine = Engine::open(&root).unwrap();
    let flowseal_files: Vec<&str> = include_str!("fixtures/flowseal-1.10.3.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let folder = common::fake_flowseal(&dir.path().join("zapret"), &flowseal_files);
    let data = tempfile::tempdir().unwrap();
    let builtin: Vec<_> = strategies::list(data.path())
        .unwrap()
        .into_iter()
        .filter(|s| s.builtin)
        .collect();
    assert!((1..=3).contains(&builtin.len()));
    for strategy in builtin {
        let tail = tail(&engine, Some(&folder), &strategy.text).unwrap();
        check(&engine, &tail).unwrap_or_else(|err| panic!("{}: {err:#}", strategy.name));
        let paths: Vec<&str> = tail
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|quoted| quoted.contains(":\\"))
            .collect();
        assert!(paths.len() > 10, "{}: {paths:?}", strategy.name);
        for path in paths {
            assert!(
                Path::new(path).exists(),
                "{} names {path}, which is not there",
                strategy.name
            );
        }
        assert!(
            strategy.text.contains("bol-van") && strategy.text.contains("MIT"),
            "{}: the source and its license are named",
            strategy.name
        );
    }
}

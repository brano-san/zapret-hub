use zapret::hosts::{merge, needs_update, plan};

#[test]
fn needs_update_compares_the_first_and_the_last_line() {
    let flowseal = "1.2.3.4 a.com\r\n\r\n5.6.7.8 b.com\r\n";
    assert!(needs_update("127.0.0.1 localhost\r\n", flowseal));
    assert!(needs_update("1.2.3.4 a.com\r\n", flowseal));
    assert!(!needs_update(
        "x\r\n1.2.3.4 a.com\r\n5.6.7.8 b.com\r\n",
        flowseal
    ));
}

#[test]
fn merge_replaces_only_its_own_block() {
    let first = merge("127.0.0.1 localhost\r\n", "1.2.3.4 a.com");
    assert_eq!(
        first,
        "127.0.0.1 localhost\r\n\r\n# zapret-hub begin\r\n1.2.3.4 a.com\r\n# zapret-hub end\r\n"
    );
    let second = merge(&first, "5.6.7.8 b.com\r\n");
    assert_eq!(
        second,
        "127.0.0.1 localhost\r\n\r\n# zapret-hub begin\r\n5.6.7.8 b.com\r\n# zapret-hub end\r\n"
    );
    assert_eq!(
        merge("", "x"),
        "# zapret-hub begin\r\nx\r\n# zapret-hub end\r\n"
    );
}

#[test]
fn plan_lists_what_comes_and_what_goes() {
    let current = merge("x\r\n", "1.1.1.1 old.com\r\n2.2.2.2 keep.com");
    let p = plan(&current, "2.2.2.2 keep.com\r\n3.3.3.3 new.com\r\n");
    assert!(p.needs_update);
    assert_eq!(p.added, ["3.3.3.3 new.com"]);
    assert_eq!(p.removed, ["1.1.1.1 old.com"]);
}

#[test]
fn merge_keeps_the_lines_after_an_unterminated_begin() {
    let current = "127.0.0.1 localhost\r\n# zapret-hub begin\r\n1.2.3.4 mine.com\r\n";
    let merged = merge(current, "5.6.7.8 b.com");
    assert_eq!(
        merged,
        "127.0.0.1 localhost\r\n1.2.3.4 mine.com\r\n\r\n# zapret-hub begin\r\n5.6.7.8 b.com\r\n# zapret-hub end\r\n"
    );
    assert_eq!(
        merge(&merged, "5.6.7.8 b.com"),
        merged,
        "the stray marker must not come back"
    );

    // A stray begin in front of a complete block: the marker goes, the lines up to the block stay.
    let current =
        "a\r\n# zapret-hub begin\r\nx\r\n# zapret-hub begin\r\nold\r\n# zapret-hub end\r\ny\r\n";
    assert_eq!(
        merge(current, "new"),
        "a\r\nx\r\ny\r\n\r\n# zapret-hub begin\r\nnew\r\n# zapret-hub end\r\n"
    );
}

#[test]
fn plan_does_not_count_the_lines_of_an_unterminated_begin_as_its_own() {
    let current = "127.0.0.1 localhost\r\n# zapret-hub begin\r\n1.2.3.4 mine.com\r\n";
    let p = plan(current, "5.6.7.8 b.com\r\n");
    assert_eq!(p.added, ["5.6.7.8 b.com"]);
    assert!(p.removed.is_empty(), "{:?}", p.removed);
}

#[test]
#[ignore = "needs the network"]
fn live_flowseal_version_is_readable() {
    let version = zapret::updates::latest_version(None).unwrap();
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "{version}"
    );
}

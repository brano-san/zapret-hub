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
#[ignore = "needs the network"]
fn live_flowseal_version_is_readable() {
    let version = zapret::updates::latest_version(None).unwrap();
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "{version}"
    );
}

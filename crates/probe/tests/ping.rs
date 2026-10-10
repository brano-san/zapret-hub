#[test]
fn pings_loopback() {
    assert!(probe::ping::ping("127.0.0.1", 1000).is_some());
}

#[test]
fn an_unresolvable_name_is_a_timeout() {
    assert_eq!(probe::ping::ping("no-such-host.invalid", 300), None);
}

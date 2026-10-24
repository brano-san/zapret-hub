#[test]
fn pings_loopback() {
    assert!(probe::ping::ping("127.0.0.1", 1000).is_some());
}

#[test]
fn an_unresolvable_name_is_a_timeout() {
    assert_eq!(probe::ping::ping("no-such-host.invalid", 300), None);
}

#[test]
fn rtt_comes_from_the_reply_line_of_linux_ping() {
    let out = "PING 1.1.1.1 (1.1.1.1) 56(84) bytes of data.\n\
               64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=4.56 ms\n\n\
               --- 1.1.1.1 ping statistics ---\n\
               1 packets transmitted, 1 received, 0% packet loss, time 0ms\n\
               rtt min/avg/max/mdev = 4.560/4.560/4.560/0.000 ms\n";
    assert_eq!(probe::ping::parse_rtt(out), Some(5));
    assert_eq!(
        probe::ping::parse_rtt("64 bytes from ::1: time=0.031 ms"),
        Some(0)
    );
    assert_eq!(
        probe::ping::parse_rtt("1 packets transmitted, 0 received"),
        None
    );
}

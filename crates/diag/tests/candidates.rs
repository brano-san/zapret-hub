use diag::candidates::candidates;
use diag::cover::{cover, covers, strategy_covers};
use diag::culprits::{Fault, Group};
use router::rules::Protocol;

const ALT11: &str = include_str!("fixtures/ALT11.txt");
const FLOWSEAL: &str = include_str!("fixtures/flowseal-ALT11.args");

fn group(key: &str, protocol: Protocol, port: u16) -> Group {
    Group {
        key: key.into(),
        protocol,
        ports: vec![port],
        members: vec![],
        representatives: vec![],
        fault: Fault::ResetEarly,
    }
}

fn strategies(text: &str) -> Vec<(String, String)> {
    vec![("zapret2/ALT11".into(), text.into())]
}

#[test]
fn cover_reads_ports_and_l7() {
    let c =
        cover("--filter-tcp=80,443,8000-8100 --filter-udp=443 --filter-l7=tls,quic --hostlist=x");
    assert_eq!(c.tcp, [(80, 80), (443, 443), (8000, 8100)]);
    assert_eq!(c.udp, [(443, 443)]);
    assert_eq!(c.l7, ["tls", "quic"]);
    assert!(c.lists);
    assert!(covers(&c, Protocol::Tcp, 8050));
    assert!(!covers(&c, Protocol::Udp, 80));
    assert!(!covers(&cover("--filter-tcp=443"), Protocol::Udp, 443));
}

#[test]
fn game_filter_covers_high_ports() {
    let c = cover("--filter-tcp=%GameFilterTCP% --filter-udp=%GameFilter%");
    assert!(covers(&c, Protocol::Tcp, 30000));
    assert!(covers(&c, Protocol::Udp, 1024));
    assert!(!covers(&c, Protocol::Udp, 1023));
}

#[test]
fn alt11_covers_udp_19300() {
    let args = zapret2::strategies::args(ALT11);
    assert!(strategy_covers(&args, Protocol::Udp, 19300));
    assert!(!strategy_covers(&args, Protocol::Udp, 100));
    assert!(strategy_covers(FLOWSEAL, Protocol::Udp, 19300));
    assert!(strategy_covers(FLOWSEAL, Protocol::Tcp, 443));
}

#[test]
fn candidate_replaces_lists_with_group_ip() {
    let found = candidates(
        &strategies(ALT11),
        &group("104.29.149.0/24", Protocol::Udp, 443),
    );
    assert!(!found.is_empty());
    for c in &found {
        assert!(!c.text.contains("--hostlist"), "{}", c.text);
        assert!(!c.text.contains("--ipset-exclude"), "{}", c.text);
        assert_eq!(c.text.matches("--ipset-ip=104.29.149.0/24").count(), 1);
        assert!(c.text.contains("--lua-desync="));
        assert!(c.text.contains("--lua-init="));
        assert!(!c.text.contains("--new"));
    }
}

#[test]
fn candidate_by_domain_uses_hostlist_domains() {
    let found = candidates(
        &strategies(ALT11),
        &group("example.com", Protocol::Tcp, 443),
    );
    assert!(!found.is_empty());
    for c in &found {
        assert_eq!(c.text.matches("--hostlist-domains=example.com").count(), 1);
        assert!(!c.text.contains("--ipset"));
    }
}

#[test]
fn identical_profiles_deduplicated() {
    let text = "--filter-tcp=443 --hostlist=a.txt --lua-desync=fake\n--new\n--filter-tcp=443 --hostlist=b.txt --lua-desync=fake";
    let found = candidates(&strategies(text), &group("x.com", Protocol::Tcp, 443));
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].text,
        "--filter-tcp=443 --hostlist-domains=x.com --lua-desync=fake"
    );
}

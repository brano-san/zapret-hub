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

/// The kept candidates only.
fn candidates(strategies: &[(String, String)], group: &Group) -> Vec<diag::candidates::Candidate> {
    diag::candidates::candidates(strategies, group).0
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

#[test]
fn the_same_profile_on_other_blobs_is_another_candidate() {
    let on = |blob: &str| {
        format!(
            "--blob=fake:@{blob}.bin\n--filter-tcp=443 --hostlist=a.txt --lua-desync=fake:blob=fake"
        )
    };
    let g = group("x.com", Protocol::Tcp, 443);
    let two = vec![
        ("zapret2/A".to_owned(), on("a")),
        ("zapret2/B".to_owned(), on("b")),
    ];
    assert_eq!(candidates(&two, &g).len(), 2);
    let same = vec![
        ("zapret2/A".to_owned(), on("a")),
        ("zapret2/C".to_owned(), on("a")),
    ];
    assert_eq!(candidates(&same, &g).len(), 1);
}

#[test]
fn filters_in_every_form() {
    // Flowseal's final args put the value after a space.
    let c = cover("--filter-tcp 80,443 --hostlist list-general.txt --dpi-desync=fake");
    assert_eq!(c.tcp, [(80, 80), (443, 443)]);
    assert!(c.lists);
    assert!(covers(&c, Protocol::Both, 443), "either protocol");
    assert!(!covers(&cover("--filter-udp=443"), Protocol::Tcp, 443));
    assert!(covers(&cover("--filter-udp=443"), Protocol::Both, 443));
    // No filter: the profile catches nothing to narrow.
    let g = group("x.com", Protocol::Tcp, 443);
    assert!(candidates(&strategies("--hostlist=a.txt --lua-desync=fake"), &g).is_empty());
    // No list: the culprit's goes right after the filters.
    let found = candidates(
        &strategies("--filter-tcp=443 --filter-l7=tls --lua-desync=fake"),
        &g,
    );
    assert_eq!(
        found[0].text,
        "--filter-tcp=443 --filter-l7=tls --hostlist-domains=x.com --lua-desync=fake"
    );
}

#[test]
fn copies_that_differ_in_unused_blobs_are_one_trick() {
    // Live 2026-10-24: Flowseal copies with the same Discord profile, their globals listing other
    // blobs too, were tried 9 times.
    let with = |extra: &str| {
        format!(
            "--wf-udp-out=50000-50100 --blob=d:@d.bin {extra}\n--filter-udp=50000-50100 --hostlist=a.txt --lua-desync=fake:blob=d"
        )
    };
    let g = group("35.217.19.0/24", Protocol::Udp, 50004);
    let copies = vec![
        ("zapret2:general (ALT)".to_owned(), with("--blob=q:@q.bin")),
        ("zapret2:general (ALT2)".to_owned(), with("--blob=t:@t.bin")),
        // Another file behind the blob it uses: another trick.
        (
            "zapret2:ALT11".to_owned(),
            with("").replace("@d.bin", "@other.bin"),
        ),
    ];
    let (kept, twins) = diag::candidates::candidates(&copies, &g);
    let sources: Vec<&str> = kept.iter().map(|c| c.source.as_str()).collect();
    assert_eq!(sources, ["zapret2:general (ALT)", "zapret2:ALT11"]);
    assert_eq!(
        twins,
        [
            "Приём zapret2: zapret2:general (ALT2), профиль 1: то же, что zapret2:general (ALT), профиль 1"
        ]
    );
}

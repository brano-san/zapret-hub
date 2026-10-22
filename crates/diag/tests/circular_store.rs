use diag::candidates::candidates;
use diag::circular_store::{GroupTricks, Store, Trick, add, generate, load, save};
use diag::culprits::{Fault, Group};
use router::rules::Protocol;
use zapret2::circular::analyze;
use zapret2::strategies::{args, split};

const ALT11: &str = include_str!("fixtures/ALT11.txt");
const VOICE: &str = "--filter-udp=50000-50100";

fn trick(text: &str, ok: u32, median_ms: Option<u32>) -> Trick {
    Trick {
        source: "zapret2/ALT11".into(),
        text: text.into(),
        found: "2026-10-22".into(),
        median_ms,
        ok,
        total: 3,
    }
}

/// ALT11's tricks for Discord voice: its voice profile and its games-over-UDP one.
fn voice_tricks() -> Vec<Trick> {
    let group = Group {
        key: "162.159.130.0/24".into(),
        protocol: Protocol::Udp,
        ports: vec![50001],
        members: vec![],
        representatives: vec![],
        fault: Fault::ResetEarly,
    };
    let found = candidates(&[("zapret2/ALT11".into(), ALT11.into())], &group);
    assert!(found.len() >= 2, "{found:?}");
    found.iter().map(|c| trick(&c.text, 3, Some(40))).collect()
}

fn store(groups: Vec<(&str, Vec<Trick>)>) -> Store {
    Store {
        groups: groups
            .into_iter()
            .map(|(filter, tricks)| GroupTricks {
                filter: filter.into(),
                list: String::new(),
                tricks,
            })
            .collect(),
    }
}

#[test]
fn add_reorders_by_ok_then_median() {
    let mut s = Store::default();
    add(
        &mut s,
        VOICE,
        vec![
            trick("a", 2, Some(50)),
            trick("b", 3, Some(90)),
            trick("c", 3, Some(40)),
            trick("d", 3, None),
        ],
    );
    let order =
        |s: &Store| -> Vec<String> { s.groups[0].tricks.iter().map(|t| t.text.clone()).collect() };
    assert_eq!(order(&s), ["c", "b", "d", "a"]);
    add(&mut s, VOICE, vec![trick("a", 3, Some(10))]);
    assert_eq!(order(&s), ["a", "c", "b", "d"]);
    add(&mut s, "--filter-tcp=443", vec![trick("e", 1, None)]);
    assert_eq!(s.groups.len(), 2);
}

#[test]
fn tricks_for_other_addresses_stay_apart() {
    let voice = |subnet: &str| {
        trick(
            &format!("{VOICE} --ipset-ip={subnet} --lua-desync=fake:blob=0x00"),
            3,
            Some(40),
        )
    };
    let mut s = Store::default();
    add(&mut s, VOICE, vec![voice("104.29.152.0/24")]);
    add(&mut s, VOICE, vec![voice("104.29.160.0/24")]);
    add(&mut s, VOICE, vec![voice("104.29.160.0/24")]);
    assert_eq!(s.groups.len(), 2);
    let out = generate("", &s).unwrap();
    let (_, profiles) = split(&args(&out));
    assert_eq!(profiles.len(), 2, "{out}");
    assert!(profiles[0].contains("--ipset-ip=104.29.152.0/24"), "{out}");
    assert!(profiles[1].contains("--ipset-ip=104.29.160.0/24"), "{out}");
}

#[test]
fn a_store_without_lists_loads() {
    let s: Store =
        toml::from_str("[[groups]]\nfilter = \"--filter-tcp=443\"\ntricks = []\n").unwrap();
    assert_eq!(s.groups[0].list, "");
}

#[test]
fn load_missing_is_empty_and_save_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diag").join("circular.toml");
    assert_eq!(load(&path).unwrap(), Store::default());
    let s = store(vec![(
        VOICE,
        vec![trick("x", 2, None), trick("y", 1, Some(7))],
    )]);
    save(&path, &s).unwrap();
    assert_eq!(load(&path).unwrap(), s);
}

#[test]
fn generate_puts_groups_first() {
    let out = generate(ALT11, &store(vec![(VOICE, voice_tricks())])).unwrap();
    let (_, profiles) = split(&args(&out));
    let (_, current) = split(&args(ALT11));
    assert_eq!(profiles.len(), current.len() + 1);
    assert!(profiles[0].starts_with(VOICE), "{}", profiles[0]);
    assert!(
        profiles[0].contains("--lua-desync=circular:"),
        "{}",
        profiles[0]
    );
    assert_eq!(profiles.last(), current.last());
}

#[test]
fn generate_single_trick_no_circular() {
    let tricks = voice_tricks();
    let out = generate(ALT11, &store(vec![(VOICE, tricks[..1].to_vec())])).unwrap();
    assert!(!out.contains("--lua-desync=circular"), "{out}");
    let (_, profiles) = split(&args(&out));
    let (_, trick) = split(&args(&tricks[0].text));
    assert_eq!(profiles[0], trick[0]);
}

#[test]
fn generate_parses_clean() {
    let tcp = [
        "--wf-tcp-out=2053 --lua-init=@\"%LUA%zapret-lib.lua\" --blob=mine:0x00 --filter-tcp=2053 --hostlist-domains=example.com --lua-desync=fake:blob=mine",
        "--wf-tcp-out=2053 --lua-init=@\"%LUA%zapret-lib.lua\" --filter-tcp=2053,2083 --hostlist-domains=example.com --lua-desync=multisplit",
    ];
    let s = store(vec![
        (VOICE, voice_tricks()),
        (
            "--filter-tcp=2053",
            tcp.iter().map(|t| trick(t, 3, None)).collect(),
        ),
    ]);
    let out = generate(ALT11, &s).unwrap();
    let analysis = analyze(&args(&out));
    assert!(analysis.errors.is_empty(), "{:?}\n{out}", analysis.errors);
    let tokens: Vec<&str> = args(&out).leak().split(' ').collect();
    // Defined once, the voice ports intercepted whole both ways, 2053 already was.
    assert_eq!(
        tokens
            .iter()
            .filter(|t| t.starts_with("--lua-init=@\"%LUA%zapret-lib.lua\""))
            .count(),
        1
    );
    assert!(tokens.contains(&"--blob=mine:0x00"), "{out}");
    assert!(tokens.contains(&"--wf-udp-in=50000-50100"), "{out}");
    assert!(
        tokens.contains(&"--wf-udp-out=%GameFilterUDP%,50000-50100"),
        "{out}"
    );
    assert_eq!(
        tokens
            .iter()
            .filter(|t| t.starts_with("--wf-tcp-out="))
            .count(),
        1
    );
    let (_, profiles) = split(&args(&out));
    assert!(
        profiles[1].starts_with("--filter-tcp=2053 "),
        "{}",
        profiles[1]
    );
    assert!(profiles[1].contains("circular"), "{}", profiles[1]);
}

use router::dns::{self, Answer, MAX_TTL, MIN_TTL, Names};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

fn name(n: &str) -> Vec<u8> {
    let mut out: Vec<u8> = n
        .split('.')
        .flat_map(|l| [&[l.len() as u8][..], l.as_bytes()].concat())
        .collect();
    out.push(0);
    out
}
fn rr(owner: &[u8], kind: u16, ttl: u32, data: &[u8]) -> Vec<u8> {
    [
        owner,
        &kind.to_be_bytes(),
        &[0, 1],
        &ttl.to_be_bytes(),
        &(data.len() as u16).to_be_bytes(),
        data,
    ]
    .concat()
}
fn message(q: &str, answers: &[Vec<u8>]) -> Vec<u8> {
    [
        &[
            0x12,
            0x34,
            0x81,
            0x80,
            0,
            1,
            0,
            answers.len() as u8,
            0,
            0,
            0,
            0,
        ][..],
        &name(q),
        &[0, 1, 0, 1],
        &answers.concat(),
    ]
    .concat()
}
fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

#[test]
fn a_records_and_the_end_of_a_cname_chain_go_under_the_asked_name() {
    let edge = name("cdn.discordapp.com.cdn.cloudflare.net");
    let m = message(
        "Cdn.DiscordApp.com",
        &[
            rr(&name("cdn.discordapp.com"), 5, 300, &edge),
            rr(&edge, 1, 60, &[162, 159, 130, 233]),
            rr(&edge, 1, 60, &[162, 159, 129, 233]),
            rr(&name("unrelated.example"), 1, 60, &[6, 6, 6, 6]),
        ],
    );
    let a = dns::parse(&m).unwrap();
    assert_eq!(a.name, "cdn.discordapp.com");
    assert_eq!(
        a.ips,
        [(ip("162.159.130.233"), 60), (ip("162.159.129.233"), 60)]
    );
}

#[test]
fn compressed_names_are_followed() {
    // question "a.discord.media" at 12..29 ("discord" label at 14), type/class to 33;
    // answer 1 at 33: owner -> 12, CNAME rdata at 45 = "x" + pointer -> 14 ("x.discord.media");
    // answer 2: owner -> 45, A 35.214.1.1, TTL 30.
    let mut m = message("a.discord.media", &[]);
    m[7] = 2;
    m.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 1, 44, 0, 4, 1, b'x', 0xC0, 14]);
    m.extend_from_slice(&[0xC0, 45, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 35, 214, 1, 1]);
    assert_eq!(
        dns::parse(&m).unwrap(),
        Answer {
            name: "a.discord.media".into(),
            ips: vec![(ip("35.214.1.1"), 30)]
        }
    );
}

#[test]
fn a_cname_loop_ends() {
    let m = message(
        "a.com",
        &[
            rr(&name("a.com"), 5, 60, &name("b.com")),
            rr(&name("b.com"), 5, 60, &name("a.com")),
            rr(&name("b.com"), 1, 60, &[1, 1, 1, 1]),
        ],
    );
    assert_eq!(dns::parse(&m).unwrap().ips, [(ip("1.1.1.1"), 60)]);
}

#[test]
fn hostile_and_broken_messages_are_dropped() {
    let ok = message(
        "discord.com",
        &[rr(&name("discord.com"), 1, 60, &[1, 2, 3, 4])],
    );
    assert!(dns::parse(&ok).is_some());
    let at = 12 + name("discord.com").len() + 4; // the answer's owner
    let (mut query, mut nxdomain, mut looped, mut forward, mut long) =
        (ok.clone(), ok.clone(), ok.clone(), ok.clone(), ok.clone());
    query[2] &= !0x80;
    nxdomain[3] = 0x83;
    looped[at..at + 2].copy_from_slice(&[0xC0, at as u8]); // points at itself
    forward[12..14].copy_from_slice(&[0xC0, 40]);
    let n = long.len();
    long[n - 5] = 9; // rdlength past the end
    for bad in [
        query,
        nxdomain,
        looped,
        forward,
        long,
        ok[..ok.len() - 1].to_vec(),
        vec![],
        vec![0; 12],
    ] {
        assert!(dns::parse(&bad).is_none(), "{bad:?}");
    }
}

#[test]
fn names_live_their_ttl_but_at_least_5_minutes() {
    let t0 = Instant::now();
    let mut names = Names::default();
    let short = Answer {
        name: "discord.com".into(),
        ips: vec![(ip("1.1.1.1"), 60)],
    };
    let long = Answer {
        name: "discord.gg".into(),
        ips: vec![(ip("2.2.2.2"), 3600)],
    };
    assert!(names.record(&short, t0) && names.record(&long, t0));
    assert!(
        !names.record(&short, t0 + Duration::from_secs(10)),
        "a known IP changes no filter"
    );
    assert_eq!(
        names.name_of(ip("1.1.1.1"), t0 + Duration::from_secs(309)),
        Some("discord.com")
    );
    assert!(names.expire(t0 + Duration::from_secs(310), &[]));
    assert_eq!(
        names.name_of(ip("1.1.1.1"), t0 + Duration::from_secs(310)),
        None
    );
    assert_eq!(names.entries(), [(ip("2.2.2.2"), "discord.gg".to_string())]);
    assert!(!names.expire(t0 + Duration::from_secs(3599), &[]));
    assert!(names.expire(t0 + MIN_TTL * 12, &[]));
}

#[test]
fn names_over_253_bytes_are_rejected() {
    let label = "a".repeat(63);
    let long = format!("{label}.{label}.{label}.{label}");
    assert!(dns::parse(&message(&long, &[])).is_none());
    let ok = format!("{label}.{label}.{label}.{}", "b".repeat(61));
    assert_eq!(dns::parse(&message(&ok, &[])).unwrap().name.len(), 253);
}

#[test]
fn a_ttl_is_clamped_to_a_day_and_a_top_bit_one_reads_as_zero() {
    let m = message(
        "discord.com",
        &[
            rr(&name("discord.com"), 1, u32::MAX, &[1, 1, 1, 1]),
            rr(&name("discord.com"), 1, 0x7FFF_FFFF, &[2, 2, 2, 2]),
            rr(&name("discord.com"), 1, 3600, &[3, 3, 3, 3]),
        ],
    );
    assert_eq!(
        dns::parse(&m).unwrap().ips,
        [
            (ip("1.1.1.1"), 0),
            (ip("2.2.2.2"), MAX_TTL),
            (ip("3.3.3.3"), 3600)
        ]
    );
}

#[test]
fn a_name_in_use_outlives_its_ttl() {
    let t0 = Instant::now();
    let mut names = Names::default();
    let answer = Answer {
        name: "discord.media".into(),
        ips: vec![(ip("1.1.1.1"), 60), (ip("2.2.2.2"), 60)],
    };
    names.record(&answer, t0);
    let late = t0 + MIN_TTL + Duration::from_secs(1);
    assert!(names.expire(late, &[ip("1.1.1.1")]), "the unused one goes");
    assert_eq!(
        names.entries(),
        [(ip("1.1.1.1"), "discord.media".to_string())]
    );
    assert_eq!(names.name_of(ip("1.1.1.1"), late), Some("discord.media"));
    assert!(
        !names.expire(late + MIN_TTL - Duration::from_secs(1), &[]),
        "a used name lives another MIN_TTL after its last use"
    );
    assert!(names.expire(late + MIN_TTL, &[]));
}

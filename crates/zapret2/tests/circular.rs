mod common;

use zapret2::Engine;
use zapret2::circular::{
    CircularEdit, CircularSettings, CircularTarget, NoteKind, analyze, apply, merge, parse_variant,
};
use zapret2::strategies::{args, check};

/// As the release has them, with `\n` line ends whatever Git checked out.
fn alt11() -> String {
    include_str!("fixtures/ALT11.txt").replace("\r\n", "\n")
}

fn alt11_plus() -> String {
    include_str!("fixtures/ALT11 plus.txt").replace("\r\n", "\n")
}

/// The manual's example of a circular profile, after a plain one.
const MANUAL: &str = "\
--wf-tcp-out=80,443
--blob=x:0x00
--filter-udp=443 --filter-l7=quic
--payload=quic_initial --lua-desync=fake:blob=fake_default_quic:repeats=11
--new
# rotating
--filter-tcp=80,443 --filter-l7=http,tls
--out-range=-s34228
--in-range=-s5556 --lua-desync=circular:fails=4:retrans=2:maxseq=16384
--in-range=x
--payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:badsum:strategy=1
--lua-desync=multidisorder:strategy=2:final
--payload=http_req
--lua-desync=fake:blob=fake_default_http:badsum:strategy=1
--lua-desync=multisplit:strategy=2
";

fn edit(variants: &[&str], stop: bool) -> CircularEdit {
    CircularEdit {
        settings: CircularSettings::default(),
        variants: variants.iter().map(|v| (*v).to_owned()).collect(),
        stop,
    }
}

#[test]
fn a_circular_profile_is_read_as_numbered_variants() {
    let analysis = analyze(&args(MANUAL));
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);
    assert!(analysis.warnings.is_empty(), "{:?}", analysis.warnings);
    let [plain, rotating] = &analysis.profiles[..] else {
        panic!("{:?}", analysis.profiles)
    };
    assert_eq!(plain.circular, None);
    assert_eq!(
        plain.plain,
        "--payload=quic_initial\n--lua-desync=fake:blob=fake_default_quic:repeats=11"
    );
    assert_eq!((rotating.index, rotating.number), (1, 2));
    assert_eq!(rotating.filter, "--filter-tcp=80,443 --filter-l7=http,tls");
    assert_eq!(
        rotating.circular,
        Some(CircularSettings {
            fails: Some(4),
            retrans: Some(2),
            extra: vec!["maxseq=16384".into()],
        })
    );
    let variants: Vec<(u32, &str, bool)> = rotating
        .variants
        .iter()
        .map(|v| (v.number, v.text.as_str(), v.is_final))
        .collect();
    assert_eq!(
        variants,
        [
            (
                1,
                "--payload=tls_client_hello\n--lua-desync=fake:blob=fake_default_tls:badsum\n--payload=http_req\n--lua-desync=fake:blob=fake_default_http:badsum",
                false
            ),
            (
                2,
                "--payload=tls_client_hello\n--lua-desync=multidisorder\n--payload=http_req\n--lua-desync=multisplit",
                true
            ),
        ]
    );
}

#[test]
fn alt11_has_no_circular_profile_and_nothing_wrong() {
    let analysis = analyze(&args(&alt11()));
    assert!(analysis.errors.is_empty() && analysis.warnings.is_empty());
    assert_eq!(analysis.profiles.len(), 9);
    assert!(analysis.profiles.iter().all(|p| p.circular.is_none()));
    // The global options are not part of the first profile's filter.
    assert_eq!(
        analysis.profiles[0].filter,
        "--filter-udp=443 --filter-l7=quic --hostlist=\"%LISTS%list-general.txt\" --hostlist=\"%LISTS%list-general-user.txt\" --hostlist-exclude=\"%LISTS%list-exclude.txt\" --hostlist-exclude=\"%LISTS%list-exclude-user.txt\" --ipset-exclude=\"%LISTS%ipset-exclude.txt\" --ipset-exclude=\"%LISTS%ipset-exclude-user.txt\""
    );
}

#[test]
fn the_checker_names_each_circular_mistake() {
    let errors = |text: &str| analyze(text).errors;
    let warnings = |text: &str| analyze(text).warnings;
    assert_eq!(
        errors(
            "--filter-tcp=443 --in-range=-s5556 --lua-desync=circular --lua-desync=fake:strategy=1 --lua-desync=multisplit:strategy=3"
        ),
        ["profile 1: strategy numbers must run 1, 2, 3... without gaps, found 1, 3"]
    );
    assert_eq!(
        errors(
            "--filter-tcp=443 --lua-desync=fake:strategy=1 --new --filter-tcp=80 --lua-desync=x"
        ),
        [
            "profile 1: 1 instance(s) with strategy= come before any --lua-desync=circular, so nothing rotates them and they all run at once"
        ]
    );
    assert_eq!(
        errors("--filter-tcp=443 --in-range=-s5556 --lua-desync=circular --lua-desync=fake"),
        ["profile 1: --lua-desync=circular has no instances with strategy=1, 2, ... after it"]
    );
    assert_eq!(
        errors(
            "--filter-tcp=443 --in-range=-s5556 --lua-desync=circular --lua-desync=fake:strategy=one"
        ),
        ["profile 1: strategy=one is not a number from 1"]
    );
    assert_eq!(
        warnings(
            "--filter-tcp=443 --lua-desync=circular --lua-desync=fake:strategy=1 --lua-desync=split"
        ),
        [
            "profile 1: no --in-range before --lua-desync=circular: circular needs incoming packets to count failures (put --in-range=-s5556 right before it)",
            "profile 1: 1 instance(s) after --lua-desync=circular have no strategy= and never run",
        ]
    );
    // `--in-range=x` before circular opens nothing.
    assert_eq!(
        warnings(
            "--filter-tcp=443 --in-range=x --lua-desync=circular --lua-desync=fake:strategy=1"
        )
        .len(),
        1
    );
}

#[test]
fn the_static_check_refuses_a_circular_error() {
    let tmp = tempfile::tempdir().unwrap();
    common::fake_engine(tmp.path(), "", &common::lib_with_functions());
    let engine = Engine::open(tmp.path()).unwrap();
    check(&engine, &args(MANUAL)).unwrap();
    let gap =
        "--filter-tcp=443 --in-range=-s5556 --lua-desync=circular --lua-desync=fake:strategy=2";
    let err = check(&engine, gap).unwrap_err().to_string();
    assert!(err.contains("without gaps, found 2"), "{err}");
}

#[test]
fn the_builder_turns_a_plain_profile_into_a_circular_one_and_reads_it_back() {
    // Google / YouTube in ALT11: its instances become variant 1, multidisorder variant 2.
    let google = analyze(&args(&alt11())).profiles[3].clone();
    let variant1 = google.plain.clone();
    let text = apply(
        &alt11(),
        &CircularTarget::Profile(google.index),
        &CircularEdit {
            settings: CircularSettings {
                fails: Some(2),
                retrans: None,
                extra: Vec::new(),
            },
            variants: vec![
                variant1.clone(),
                "--lua-desync=multidisorder:pos=midsld".into(),
            ],
            stop: true,
        },
    )
    .unwrap();
    let expected_profile = "\
# Google / YouTube
--filter-tcp=443 --filter-l7=tls --hostlist=\"%LISTS%list-google.txt\"
--out-range=-s34228
--in-range=-s5556 --lua-desync=circular:fails=2
--in-range=x
--payload=tls_client_hello
--lua-desync=fake:blob=tls_google:tcp_ts=-600000:repeats=8:ip_id=zero:strategy=1
--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google:ip_id=zero:strategy=1
--payload=all
--lua-desync=multidisorder:pos=midsld:strategy=2:final
--new
";
    assert!(text.contains(expected_profile), "{text}");
    // Everything else stays as written.
    let without = |t: &str| t.replace(expected_profile, "").replace("\r\n", "\n");
    let original_profile = "\
# Google / YouTube
--filter-tcp=443 --filter-l7=tls --hostlist=\"%LISTS%list-google.txt\"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=tls_google:tcp_ts=-600000:repeats=8:ip_id=zero
--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google:ip_id=zero
--new
";
    assert_eq!(
        without(&text),
        without(&alt11()).replace(original_profile, "")
    );
    let back = analyze(&args(&text));
    assert!(
        back.errors.is_empty() && back.warnings.is_empty(),
        "{back:?}"
    );
    let profile = &back.profiles[3];
    assert_eq!(profile.variants.len(), 2);
    assert_eq!(profile.variants[0].text, variant1);
    assert!(profile.variants[1].is_final);
    // Writing what was read back changes nothing.
    let again = apply(
        &text,
        &CircularTarget::Profile(3),
        &CircularEdit {
            settings: profile.circular.clone().unwrap(),
            variants: profile.variants.iter().map(|v| v.text.clone()).collect(),
            stop: true,
        },
    )
    .unwrap();
    assert_eq!(again, text);
}

#[test]
fn a_new_profile_goes_after_the_chosen_one_and_brings_its_blobs() {
    let text = "--wf-tcp-out=443\n--lua-init=@\"%LUA%zapret-lib.lua\"\n--filter-tcp=80 --lua-desync=fake\n--new\n--filter-udp=443 --lua-desync=fake\n";
    let target = CircularTarget::New {
        after: 0,
        filter: "--filter-tcp=443 --filter-l7=tls --hostlist-domains=example.com".into(),
    };
    let variants = [
        "--lua-desync=fake:blob=tls_google:repeats=8",
        "--lua-desync=fake:blob=stun2:repeats=8",
    ];
    let out = apply(text, &target, &edit(&variants, false)).unwrap();
    assert_eq!(
        out,
        "\
--wf-tcp-out=443
--lua-init=@\"%LUA%zapret-lib.lua\"
--blob=tls_google:@\"%BIN%tls_clienthello_www_google_com.bin\"
--blob=stun2:@\"%BIN%stun2.bin\"
--filter-tcp=80 --lua-desync=fake
--new

# circular
--filter-tcp=443 --filter-l7=tls --hostlist-domains=example.com
--out-range=-s34228
--in-range=-s5556 --lua-desync=circular
--in-range=x
--lua-desync=fake:blob=tls_google:repeats=8:strategy=1
--lua-desync=fake:blob=stun2:repeats=8:strategy=2
--new
--filter-udp=443 --lua-desync=fake
"
    );
    // After the last profile: at the end.
    let last = CircularTarget::New {
        after: 1,
        filter: "--filter-tcp=443".into(),
    };
    let out = apply(text, &last, &edit(&["--lua-desync=split"], false)).unwrap();
    assert!(out.ends_with("--filter-udp=443 --lua-desync=fake\n--new\n\n# circular\n--filter-tcp=443\n--out-range=-s34228\n--in-range=-s5556 --lua-desync=circular\n--in-range=x\n--lua-desync=split:strategy=1\n"), "{out}");
}

#[test]
fn the_builder_refuses_what_it_cannot_write() {
    let text = "--filter-tcp=443 --lua-desync=fake\n";
    let err = |target: CircularTarget, variants: &[&str]| {
        apply(text, &target, &edit(variants, false))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        err(CircularTarget::Profile(1), &["--lua-desync=x"]),
        "the strategy has no profile 2"
    );
    assert_eq!(
        err(
            CircularTarget::Profile(0),
            &["--lua-desync=x", "--filter-tcp=80"]
        ),
        "variant 2: a variant has only --payload= and --lua-desync= options, not --filter-tcp=80"
    );
    assert_eq!(
        err(CircularTarget::Profile(0), &["# nothing"]),
        "variant 1: a variant has no --lua-desync="
    );
    assert_eq!(
        err(
            CircularTarget::New {
                after: 0,
                filter: "--filter-tcp=443 --new".into()
            },
            &["--lua-desync=x"]
        ),
        "--new does not belong in a profile's filter"
    );
    // A pasted tag is the builder's to write.
    let pasted = parse_variant("--lua-desync=fake:strategy=7:final").unwrap();
    assert_eq!(pasted[0].call, "fake");
}

#[test]
fn a_line_with_the_new_option_is_split_around_the_inserted_profile() {
    let text = "--filter-tcp=443 --lua-desync=fake --new --filter-tcp=80 --lua-desync=split\n";
    let out = apply(
        text,
        &CircularTarget::Profile(0),
        &edit(&["--lua-desync=x", "--lua-desync=y"], false),
    )
    .unwrap();
    assert_eq!(
        out,
        "--filter-tcp=443\n--out-range=-s34228\n--in-range=-s5556 --lua-desync=circular\n--in-range=x\n--lua-desync=x:strategy=1\n--lua-desync=y:strategy=2\n--new --filter-tcp=80 --lua-desync=split\n"
    );
}

#[test]
fn alt11_and_alt11_plus_share_every_profile_and_differ_in_none() {
    let merged = merge(&[
        ("ALT11".into(), alt11()),
        ("ALT11 plus".into(), alt11_plus()),
    ])
    .unwrap();
    let kinds: Vec<(NoteKind, u32)> = merged
        .notes
        .iter()
        .map(|n| (n.kind.clone(), n.number))
        .collect();
    let mut expected: Vec<(NoteKind, u32)> = (1..=9).map(|n| (NoteKind::Same, n)).collect();
    // ALT11 plus's pass-through profile for game services.
    expected.push((NoteKind::LeftOut, 5));
    assert_eq!(kinds, expected);
    assert!(
        merged.text.ends_with(&alt11()),
        "nothing rotates: ALT11 as it was"
    );
}

#[test]
fn merging_rotates_shared_tcp_profiles_and_explains_the_rest() {
    // ALT11 with Google / YouTube on multidisorder, QUIC on another blob, discord.media gone and
    // a blob of its own.
    let other = alt11()
        .replace(
            "--lua-desync=fake:blob=tls_google:tcp_ts=-600000:repeats=8:ip_id=zero\n--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google:ip_id=zero",
            "--lua-desync=multidisorder:pos=midsld",
        )
        .replace(
            "--payload=quic_initial --lua-desync=fake:blob=quic_google:repeats=11\n--new\n\n# Discord voice",
            "--payload=quic_initial --lua-desync=fake:blob=quic_vk:repeats=6\n--new\n\n# Discord voice",
        )
        .replace("--hostlist-domains=discord.media", "--hostlist-domains=discord.gg")
        .replace(
            "--blob=stun2:@\"%BIN%stun2.bin\"",
            "--blob=stun2:@\"%BIN%stun2.bin\"\n--blob=quic_vk:@\"%BIN%quic_vk.bin\"\n--blob=tls_max:@\"%BIN%other.bin\"",
        );
    assert_ne!(other, alt11());
    let merged = merge(&[("ALT11".into(), alt11()), ("mine".into(), other)]).unwrap();
    let notes: Vec<(NoteKind, u32, Vec<String>, u32)> = merged
        .notes
        .iter()
        .map(|n| (n.kind.clone(), n.number, n.names.clone(), n.variants))
        .collect();
    let names = |list: &[&str]| list.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        notes,
        [
            (NoteKind::NotTcp, 1, vec![], 0),
            (NoteKind::Same, 2, vec![], 1),
            (NoteKind::Missing, 3, names(&["mine"]), 0),
            (NoteKind::Rotates, 4, names(&["ALT11", "mine"]), 2),
            (NoteKind::Same, 5, vec![], 1),
            (NoteKind::Same, 6, vec![], 1),
            (NoteKind::Same, 7, vec![], 1),
            (NoteKind::Same, 8, vec![], 1),
            (NoteKind::Same, 9, vec![], 1),
            (NoteKind::LeftOut, 3, names(&["mine"]), 0),
            (NoteKind::BlobConflict, 0, names(&["mine"]), 0),
        ]
    );
    assert_eq!(merged.notes[10].filter, "tls_max");
    let text = &merged.text;
    assert!(text.starts_with("# circular over ALT11, mine: the profiles they share rotate between their variants\n--blob=quic_vk:@\"%BIN%quic_vk.bin\"\n# Port of Flowseal general to zapret2\n"), "{text}");
    let analysis = analyze(&args(text));
    assert!(
        analysis.errors.is_empty() && analysis.warnings.is_empty(),
        "{analysis:?}"
    );
    let google = &analysis.profiles[3];
    assert_eq!(google.circular, Some(CircularSettings::default()));
    let variants: Vec<&str> = google.variants.iter().map(|v| v.text.as_str()).collect();
    assert_eq!(
        variants,
        [
            "--payload=tls_client_hello\n--lua-desync=fake:blob=tls_google:tcp_ts=-600000:repeats=8:ip_id=zero\n--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google:ip_id=zero",
            "--payload=tls_client_hello\n--lua-desync=multidisorder:pos=midsld",
        ]
    );
}

#[test]
fn merging_takes_two_to_five() {
    let one = [("a".to_owned(), "--filter-tcp=443".to_owned())];
    assert_eq!(
        merge(&one).unwrap_err().to_string(),
        "pick 2 to 5 strategies to merge, not 1"
    );
}

#[test]
fn a_text_with_crlf_line_ends_keeps_them() {
    let text = "--filter-tcp=443\r\n--lua-desync=fake\r\n";
    let out = apply(
        text,
        &CircularTarget::Profile(0),
        &edit(&["--lua-desync=fake"], false),
    )
    .unwrap();
    assert_eq!(out.matches("\r\n").count(), out.matches('\n').count());
    assert!(out.ends_with("--lua-desync=fake:strategy=1\r\n"), "{out}");
}

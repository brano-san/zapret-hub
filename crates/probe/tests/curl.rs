use probe::curl::{
    CheckKind, CheckStatus, DpiStatus, Via, classify, dpi_classify, https_args,
    parse_code_and_time, parse_doh_json,
};

#[test]
fn classifies_like_test_zapret_ps1() {
    assert_eq!(classify(Some(0), ""), CheckStatus::Ok);
    assert_eq!(
        classify(
            Some(28),
            "curl: (28) Connection timed out after 2001 milliseconds"
        ),
        CheckStatus::Error
    );
    assert_eq!(
        classify(Some(6), "curl: (6) Could not resolve host: discord.com"),
        CheckStatus::Ssl
    );
    assert_eq!(
        classify(
            Some(60),
            "curl: (60) SSL certificate problem: self-signed certificate"
        ),
        CheckStatus::Ssl
    );
    assert_eq!(
        classify(Some(35), "curl: (35) Recv failure: Connection was reset"),
        CheckStatus::Unsup
    );
    assert_eq!(classify(Some(35), ""), CheckStatus::Unsup);
    assert_eq!(
        classify(
            Some(4),
            "curl: option --tlsv1.3: the installed libcurl version does not support this"
        ),
        CheckStatus::Unsup
    );
    assert_eq!(classify(None, ""), CheckStatus::Error);
}

#[test]
fn reads_code_and_time_from_w() {
    assert_eq!(
        parse_code_and_time("200 0.153000"),
        ("200".to_owned(), Some(153))
    );
    assert_eq!(
        parse_code_and_time("000 2.004"),
        ("000".to_owned(), Some(2004))
    );
    assert_eq!(parse_code_and_time(""), ("000".to_owned(), None));
}

#[test]
fn dpi_verdicts_follow_invoke_dpi_suite() {
    let blocked = dpi_classify(CheckKind::Tls12, Some(28), "000 65536 0 5.004", 5);
    assert_eq!(blocked.status, DpiStatus::LikelyBlocked);
    assert_eq!((blocked.up_bytes, blocked.down_bytes), (65536, 0));

    assert_eq!(
        dpi_classify(CheckKind::Http, Some(0), "200 65536 65536 0.512", 5).status,
        DpiStatus::Ok
    );
    assert_eq!(
        dpi_classify(CheckKind::Http, Some(28), "000 0 0 3.001", 5).status,
        DpiStatus::Fail
    );
    let unsup = dpi_classify(CheckKind::Tls13, Some(35), "", 5);
    assert_eq!(
        (unsup.status, unsup.code.as_str()),
        (DpiStatus::Unsupported, "UNSUP")
    );
    let err = dpi_classify(CheckKind::Http, Some(7), "garbage", 5);
    assert_eq!((err.status, err.code.as_str()), (DpiStatus::Fail, "ERR"));
}

#[test]
fn payload_is_64_kib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.bin");
    probe::curl::write_payload(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 65536);
}

#[test]
#[ignore = "needs the network"]
fn live_check_reaches_a_site() {
    let result = probe::curl::check(
        "https://www.gstatic.com/generate_204",
        CheckKind::Tls12,
        None,
    );
    assert_eq!(result.status, CheckStatus::Ok, "{result:?}");
}

fn has_pair(args: &[String], a: &str, b: &str) -> bool {
    args.windows(2).any(|w| w[0] == a && w[1] == b)
}

#[test]
fn https_args_resolve_and_socks() {
    let args = https_args(
        "discord.com",
        Some("1.2.3.4".parse().unwrap()),
        &Via::Socks5("127.0.0.1:2080".into()),
    );
    assert!(has_pair(&args, "--resolve", "discord.com:443:1.2.3.4"));
    assert!(has_pair(&args, "--socks5-hostname", "127.0.0.1:2080"));
    assert!(!args.contains(&"--noproxy".to_owned()));
    assert_eq!(args.last().unwrap(), "https://discord.com");
}

#[test]
fn https_args_direct_has_noproxy() {
    let args = https_args("discord.com", None, &Via::Direct);
    assert!(has_pair(&args, "--noproxy", "*"));
    assert!(!args.contains(&"--resolve".to_owned()));
}

#[test]
fn doh_json_parsed() {
    let json = r#"{"Status":0,"Answer":[
        {"name":"d.com","type":5,"TTL":1,"data":"x.d.com."},
        {"name":"x.d.com","type":1,"TTL":1,"data":"162.159.137.232"},
        {"name":"x.d.com","type":1,"TTL":1,"data":"162.159.136.232"}]}"#;
    assert_eq!(
        parse_doh_json(json),
        vec![
            "162.159.137.232".parse::<std::net::Ipv4Addr>().unwrap(),
            "162.159.136.232".parse().unwrap()
        ]
    );
    assert!(parse_doh_json(r#"{"Status":3}"#).is_empty());
    assert!(parse_doh_json("not json").is_empty());
}

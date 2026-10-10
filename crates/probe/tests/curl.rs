use probe::curl::{CheckKind, CheckStatus, DpiStatus, classify, dpi_classify, parse_code_and_time};

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

use zapret::batparse::{GameFilterVars, parse};

fn all_ports() -> GameFilterVars {
    GameFilterVars {
        all: "1024-65535".into(),
        tcp: "1024-65535".into(),
        udp: "12".into(),
    }
}

#[test]
fn parses_a_strategy_like_service_bat() {
    let bat = concat!(
        "@echo off\r\n",
        "set \"BIN=%~dp0bin\\\"\r\n",
        "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443,%GameFilterTCP% --wf-udp=443 ^\r\n",
        "--filter-udp=443 --hostlist=\"%LISTS%list-general.txt\" --dpi-desync=fake --dpi-desync-fake-tls-mod=rnd,sni=www.google.com --new ^\r\n",
        "--filter-tcp=80 --dpi-desync-fake-tls=\"@bin\\x.bin\" --hostlist=\"lists\\y.txt\"\r\n",
    );
    let parsed = parse(bat, "E:\\Zapret\\", &all_ports()).unwrap();
    assert_eq!(
        parsed.final_args,
        concat!(
            "  --wf-tcp 80,443,1024-65535 --wf-udp 443",
            "  --filter-udp 443 --hostlist \\\"E:\\Zapret\\lists\\list-general.txt\\\" --dpi-desync fake",
            " --dpi-desync-fake-tls-mod rnd,sni=www.google.com --new",
            "  --filter-tcp 80 --dpi-desync-fake-tls \\\"@E:\\Zapret\\bin\\x.bin\\\" --hostlist \\\"E:\\Zapret\\lists\\y.txt\\\"",
        )
    );
    assert_eq!(
        parsed.command_tail(),
        parsed.final_args.replace("\\\"", "\"")
    );
    assert_eq!(
        parsed.bin_path(std::path::Path::new(r"E:\Zapret\bin\winws.exe")),
        format!("\"E:\\Zapret\\bin\\winws.exe\" {}", parsed.command_tail())
    );
    assert_eq!(parsed.sha256().len(), 64);
}

#[test]
fn keeps_a_cyrillic_root_intact() {
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --hostlist=\"%LISTS%a.txt\"\r\n";
    let parsed = parse(bat, "C:\\Запрет\\", &GameFilterVars::disabled()).unwrap();
    assert_eq!(
        parsed.final_args,
        "  --hostlist \\\"C:\\Запрет\\lists\\a.txt\\\""
    );
}

#[test]
fn disabled_game_filter_puts_port_12() {
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --wf-udp=443,%GameFilterUDP% ^\r\n--filter-tcp=%GameFilter%\r\n";
    let parsed = parse(bat, "C:\\z\\", &GameFilterVars::disabled()).unwrap();
    assert_eq!(parsed.final_args, "  --wf-udp 443,12  --filter-tcp 12");
}

#[test]
fn a_caret_before_an_exclamation_mark_is_an_escape() {
    // general (FAKE TLS AUTO).bat: service.bat passes `!` to winws, not `^!`.
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --filter-tcp=443 --dpi-desync-fake-tls=^! ^\r\n--new\r\n";
    let parsed = parse(bat, "C:\\z\\", &GameFilterVars::disabled()).unwrap();
    assert!(
        parsed.final_args.contains("--dpi-desync-fake-tls !"),
        "{}",
        parsed.final_args
    );
    assert!(!parsed.final_args.contains('^'), "{}", parsed.final_args);
}

#[test]
fn a_file_without_winws_is_not_a_strategy() {
    assert!(
        parse(
            "@echo off\r\necho hi\r\n",
            "C:\\z\\",
            &GameFilterVars::disabled()
        )
        .is_err()
    );
}

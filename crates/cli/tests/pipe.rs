//! The pipe in one process, without elevation: a pipe made by the current user changes nothing
//! in the system and goes away with the test.

use std::os::windows::io::AsRawHandle as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use cli::pipe::{self, Server};

const BUSY: Duration = Duration::from_secs(10);

/// Tests run in parallel: a name each.
fn unique() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        r"\\.\pipe\zapret-hub-cli-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

fn serve(name: &str, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) {
    let server = Server::bind(name).unwrap();
    std::thread::spawn(move || server.serve(handler));
}

#[test]
fn a_client_gets_the_answer_to_its_line() {
    let name = unique();
    serve(&name, |line| String::from_utf8_lossy(line).to_uppercase());
    assert_eq!(pipe::request(&name, b"hello", BUSY).unwrap(), b"HELLO");
    assert_eq!(
        pipe::request(&name, "ещё".as_bytes(), BUSY).unwrap(),
        "ЕЩЁ".as_bytes(),
        "one connection per request"
    );
}

#[test]
fn a_second_server_cannot_take_the_name() {
    let name = unique();
    let _first = Server::bind(&name).unwrap();
    let Err(second) = Server::bind(&name) else {
        panic!("FILE_FLAG_FIRST_PIPE_INSTANCE let a second server in");
    };
    assert_eq!(second.raw_os_error(), Some(5), "{second}");
}

#[test]
fn no_server_means_file_not_found() {
    let err = pipe::request(&unique(), b"{}", BUSY).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(2), "{err}");
}

#[test]
fn a_slow_client_does_not_hold_up_the_others() {
    let name = unique();
    serve(&name, |line| {
        if line == b"slow" {
            std::thread::sleep(Duration::from_secs(2));
        }
        String::from_utf8_lossy(line).into_owned()
    });
    let slow = {
        let name = name.clone();
        std::thread::spawn(move || pipe::request(&name, b"slow", BUSY).unwrap())
    };
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(pipe::request(&name, b"fast", BUSY).unwrap(), b"fast");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(slow.join().unwrap(), b"slow");
}

#[test]
fn a_client_that_leaves_before_its_answer_does_not_stop_the_server() {
    use std::io::Write as _;

    let name = unique();
    serve(&name, |line| {
        std::thread::sleep(Duration::from_millis(300));
        String::from_utf8_lossy(line).into_owned()
    });
    let mut gone = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    gone.write_all(b"bye\n").unwrap();
    drop(gone);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        pipe::request(&name, b"still here", BUSY).unwrap(),
        b"still here"
    );
}

#[test]
fn a_line_its_client_never_finished_reaches_no_handler() {
    use std::io::Write as _;
    use std::sync::mpsc;

    let name = unique();
    let (ran, lines) = mpsc::channel();
    serve(&name, move |line| {
        let _ = ran.send(line.to_vec());
        String::new()
    });
    // The server waits in ConnectNamedPipe by now, so it does read what the client wrote.
    std::thread::sleep(Duration::from_millis(300));
    let mut gone = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    gone.write_all(b"x").unwrap();
    drop(gone);
    assert_eq!(
        lines.recv_timeout(Duration::from_millis(500)),
        Err(mpsc::RecvTimeoutError::Timeout),
        "a line without its \\n ran"
    );
    pipe::request(&name, b"y", BUSY).unwrap();
    assert_eq!(lines.recv().unwrap(), b"y", "a whole line still runs");
}

#[test]
fn an_app_that_closes_without_an_answer_is_an_unexpected_eof() {
    let name = unique();
    serve(&name, |_| panic!("the handler fails"));
    let err = pipe::request(&name, b"{}", BUSY).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{err}");
}

#[test]
fn an_oversized_line_is_cut_and_still_answered() {
    let name = unique();
    serve(&name, |line| line.len().to_string());
    let line = vec![b'a'; cli::MAX_REQUEST + 100_000];
    assert_eq!(
        pipe::request(&name, &line, BUSY).unwrap(),
        (cli::MAX_REQUEST + 1).to_string().as_bytes()
    );
}

#[test]
fn the_descriptor_names_this_user() {
    let sid = pipe::current_user_sid().unwrap();
    assert!(sid.starts_with("S-1-5-"), "{sid}");
    assert_eq!(
        pipe::sddl(&sid),
        format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{sid})S:(ML;;NWNR;;;ME)")
    );
}

#[test]
fn the_pipe_lets_in_system_administrators_and_this_user_only() {
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
        SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    use windows::core::PWSTR;

    let name = unique();
    serve(&name, |_| String::new());
    let client = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    let what = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    // SAFETY: plain Win32 calls on an open handle; both buffers are freed.
    let sddl = unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            HANDLE(client.as_raw_handle()),
            SE_KERNEL_OBJECT,
            what,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .unwrap();
        let mut text = PWSTR::null();
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            what,
            &mut text,
            None,
        )
        .unwrap();
        let sddl = text.to_string().unwrap();
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
        sddl
    };
    let sid = pipe::current_user_sid().unwrap();
    // SDDL writes the built-in Administrator (RID 500, the CI runner's user) as `LA`, not its SID.
    let user = if sid.ends_with("-500") {
        "LA".to_owned()
    } else {
        sid
    };
    assert!(sddl.contains(&format!(";;;{user})")), "{sddl}");
    assert!(sddl.contains(";;;SY)") && sddl.contains(";;;BA)"), "{sddl}");
    assert_eq!(sddl.matches("(A;").count(), 3, "nobody else: {sddl}");
    assert!(sddl.contains("(ML;;NWNR;;;ME)"), "{sddl}");
}

#[test]
fn the_app_can_identify_the_cli_but_not_act_as_it() {
    use std::fs::File;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::windows::io::{FromRawHandle as _, OwnedHandle};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{
        GetTokenInformation, RevertToSelf, SECURITY_IMPERSONATION_LEVEL, SecurityIdentification,
        TOKEN_QUERY, TokenImpersonationLevel,
    };
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, ImpersonateNamedPipeClient, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
    use windows::core::HSTRING;

    let name = unique();
    // SAFETY: a plain pipe with the default descriptor; the `File` owns and closes it.
    let pipe = unsafe {
        let raw = CreateNamedPipeW(
            &HSTRING::from(&name),
            PIPE_ACCESS_DUPLEX,
            PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            None,
        );
        assert!(!raw.is_invalid(), "{}", std::io::Error::last_os_error());
        File::from(OwnedHandle::from_raw_handle(raw.0))
    };
    let app = std::thread::spawn(move || {
        let handle = HANDLE(pipe.as_raw_handle());
        // ERROR_PIPE_CONNECTED when the client came first; a real failure fails the read below.
        // SAFETY: a valid pipe handle, used synchronously.
        let _ = unsafe { ConnectNamedPipe(handle, None) };
        // Impersonating needs a read first.
        BufReader::new(&pipe)
            .read_until(b'\n', &mut Vec::new())
            .unwrap();
        // SAFETY: this thread impersonates only to open its token and reverts before anything
        // else; the token is closed by its `OwnedHandle`.
        let level = unsafe {
            ImpersonateNamedPipeClient(handle).unwrap();
            let mut token = HANDLE::default();
            // As self: an identification token cannot open itself.
            let opened = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token);
            RevertToSelf().unwrap();
            opened.unwrap();
            let token = OwnedHandle::from_raw_handle(token.0);
            let mut level = SECURITY_IMPERSONATION_LEVEL::default();
            let mut size = 0;
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenImpersonationLevel,
                Some((&raw mut level).cast()),
                size_of_val(&level) as u32,
                &mut size,
            )
            .unwrap();
            level
        };
        (&pipe).write_all(b"\n").unwrap();
        pipe.sync_all().unwrap();
        level
    });
    pipe::request(&name, b"who", BUSY).unwrap();
    assert_eq!(app.join().unwrap(), SecurityIdentification);
}

//! TG WS Proxy: `tg-ws-proxy` of valnesfjord/tg-ws-proxy-rs, a local MTProto proxy that carries
//! Telegram over WebSocket. The latest release is downloaded on the user's request into the data
//! folder. Linux runs it as a `systemd --user` unit (no root anywhere), Windows as the app's own
//! child process in a job object: it starts with the app when on and ends with it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context as _;
use tauri::AppHandle;

use crate::actions::{self, changed};
use crate::dto::TgWsStatus;
use crate::settings::Settings;
use crate::state::{Paths, lock};

const API_URL: &str = "https://api.github.com/repos/valnesfjord/tg-ws-proxy-rs/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/valnesfjord/tg-ws-proxy-rs/releases/download/";

/// This system's build in a release: a static musl binary in a `.tar.gz` for Linux, a zip for
/// Windows.
#[cfg(windows)]
const ASSET: &str = "tg-ws-proxy-x86_64-pc-windows-gnu.zip";
#[cfg(not(windows))]
const ASSET: &str = "tg-ws-proxy-x86_64-unknown-linux-musl.tar.gz";

#[cfg(windows)]
const IMAGE: &str = "tg-ws-proxy.exe";
#[cfg(not(windows))]
const IMAGE: &str = "tg-ws-proxy";

/// Why the latest release has nothing to install here, once a download found out.
static UNAVAILABLE: Mutex<Option<String>> = Mutex::new(None);

/// `<data>/tg-ws-proxy`: the binary and `version.txt`.
fn dir(paths: &Paths) -> PathBuf {
    paths.data().join("tg-ws-proxy")
}

fn bin(paths: &Paths) -> PathBuf {
    dir(paths).join(IMAGE)
}

fn version(paths: &Paths) -> Option<String> {
    std::fs::read_to_string(dir(paths).join("version.txt"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|tag| !tag.is_empty())
}

/// A secret is made once, while `tg_ws_secret` is empty: 16 random bytes as 32 hex characters.
/// `true` when made here.
pub fn ensure_secret(settings: &mut Settings) -> bool {
    if !settings.tg_ws_secret.is_empty() {
        return false;
    }
    let mut bytes = [0u8; 16];
    if let Err(err) = getrandom::fill(&mut bytes) {
        log::warn!("no random bytes for the TG WS Proxy secret: {err}");
        return false;
    }
    settings.tg_ws_secret = bytes.iter().map(|b| format!("{b:02x}")).collect();
    true
}

/// What Telegram adds the proxy from: `dd` + the secret, the padded MTProto form a local
/// listener takes.
pub fn link(settings: &Settings) -> String {
    format!(
        "tg://proxy?server=127.0.0.1&port={}&secret=dd{}",
        settings.tg_ws_port, settings.tg_ws_secret
    )
}

/// A port, and a secret of exactly 32 hex characters: refused before anything is saved.
fn validate(settings: &Settings) -> anyhow::Result<()> {
    anyhow::ensure!(
        settings.tg_ws_port != 0,
        "port 0 does not exist: use 1-65535"
    );
    anyhow::ensure!(
        settings.tg_ws_secret.len() == 32
            && settings.tg_ws_secret.bytes().all(|b| b.is_ascii_hexdigit()),
        "the secret must be exactly 32 hex characters"
    );
    Ok(())
}

/// The command line: listening on 127.0.0.1 only, then the user's extra flags, split at spaces.
fn argv(bin: &Path, settings: &Settings) -> Vec<String> {
    let mut argv = vec![
        bin.display().to_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        settings.tg_ws_port.to_string(),
        "--secret".into(),
        settings.tg_ws_secret.clone(),
    ];
    argv.extend(settings.tg_ws_args.split_whitespace().map(str::to_owned));
    argv
}

/// Where the asset's SHA-256 comes from: the release's `SHA256SUMS` (its URL), else the
/// `digest` GitHub computed for the asset.
#[derive(Debug, PartialEq)]
enum Sum {
    File(String),
    Digest(String),
}

/// A `sha256:<64 hex>` digest as lowercase hex.
fn digest_hex(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// The asset's hash as `SHA256SUMS` lists it.
fn listed(sums: &str) -> Option<String> {
    zapret::release::parse_sums(sums)
        .into_iter()
        .find(|(_, path)| path.rsplit('/').next() == Some(ASSET))
        .map(|(hash, _)| hash)
}

/// The latest release's tag and this system's asset URL with its checksum source, `None` when
/// the release has no build for it. A build with no checksum is refused. Each URL must be a
/// download of the same repository; nothing the answer carries becomes a command.
fn select(json: &str) -> anyhow::Result<(String, Option<(String, Sum)>)> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("GitHub answered with something other than JSON")?;
    let tag = value["tag_name"]
        .as_str()
        .context("the GitHub answer has no tag_name")?;
    anyhow::ensure!(
        !tag.is_empty()
            && tag.len() <= 64
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "unexpected release tag {tag:?}"
    );
    let asset = |name: &str| {
        value["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|asset| asset["name"].as_str() == Some(name))
    };
    let url = |asset: &serde_json::Value| {
        let url = asset["browser_download_url"].as_str().unwrap_or_default();
        anyhow::ensure!(
            url.starts_with(DOWNLOAD_PREFIX),
            "{} points outside tg-ws-proxy-rs: {url}",
            asset["name"].as_str().unwrap_or_default()
        );
        Ok(url.to_owned())
    };
    let Some(build) = asset(ASSET) else {
        return Ok((tag.to_owned(), None));
    };
    let sum = match asset("SHA256SUMS") {
        Some(sums) => Sum::File(url(sums)?),
        None => Sum::Digest(
            build["digest"]
                .as_str()
                .and_then(digest_hex)
                .with_context(|| format!("release {tag} has no SHA256SUMS or digest for {ASSET}: not installed unchecked"))?,
        ),
    };
    Ok((tag.to_owned(), Some((url(build)?, sum))))
}

/// The one `tg-ws-proxy` file in the unpacked archive, wherever it keeps it.
fn find_binary(root: &Path) -> anyhow::Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() && entry.file_name() == IMAGE {
                return Ok(entry.path());
            }
        }
    }
    anyhow::bail!("{ASSET} has no {IMAGE}")
}

#[cfg(windows)]
fn unpack_archive(archive: &Path, to: &Path, cancel: &AtomicBool) -> anyhow::Result<()> {
    zapret::release::expand(archive, to, cancel)
}

#[cfg(not(windows))]
fn unpack_archive(archive: &Path, to: &Path, cancel: &AtomicBool) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    let mut tar = std::process::Command::new("tar");
    tar.arg("-xzf").arg(archive).arg("-C").arg(to);
    let out = exec::run(&mut tar, Duration::from_secs(300), Some(cancel))?;
    anyhow::ensure!(!out.canceled, "canceled");
    out.check()?;
    Ok(())
}

/// The latest release over `dir`: downloaded and unpacked in a staging folder, the binary moved
/// out (executable), the tag into `version.txt`. On a failure the old files stay.
fn download(dir: &Path, socks5: Option<&str>, cancel: &AtomicBool) -> anyhow::Result<()> {
    let (tag, url) = select(&probe::curl::fetch_text(API_URL, socks5)?)?;
    let Some((url, sum)) = url else {
        let why = format!("release {tag} of tg-ws-proxy-rs has no build for this system ({ASSET})");
        *lock(&UNAVAILABLE) = Some(why.clone());
        anyhow::bail!(why);
    };
    *lock(&UNAVAILABLE) = None;
    let staging = dir.join(".zapret-hub-download");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("cannot create {}", staging.display()))?;
    let result = (|| {
        let archive = staging.join(ASSET);
        probe::curl::with_fallback(socks5, |proxy| {
            anyhow::ensure!(!cancel.load(Ordering::Relaxed), "canceled");
            let mut cmd = probe::curl::get(&url, proxy);
            cmd.args(["-m", "900", "-o"]).arg(&archive);
            exec::run(&mut cmd, Duration::from_secs(910), Some(cancel))?.check()?;
            Ok(())
        })?;
        anyhow::ensure!(!cancel.load(Ordering::Relaxed), "canceled");
        let expected = match sum {
            Sum::Digest(hash) => hash,
            Sum::File(sums) => listed(&probe::curl::fetch_text(&sums, socks5)?)
                .with_context(|| format!("SHA256SUMS of {tag} does not list {ASSET}"))?,
        };
        let sha256 = zapret::sha256_hex(&std::fs::read(&archive)?);
        anyhow::ensure!(
            sha256 == expected,
            "{ASSET} {tag} SHA-256 is {sha256}, the release lists {expected}: not installed"
        );
        log::info!("{ASSET} {tag} SHA-256 {sha256} checked");
        let raw = staging.join("x");
        unpack_archive(&archive, &raw, cancel)?;
        let to = dir.join(IMAGE);
        std::fs::copy(find_binary(&raw)?, &to)
            .with_context(|| format!("cannot write {}", to.display()))?;
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::write(dir.join("version.txt"), format!("{tag}\n"))?;
        log::info!("tg-ws-proxy {tag} installed: {}", to.display());
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(not(windows))]
mod run {
    //! `systemd --user`: the unit in `~/.config/systemd/user`, enabled with the setting, so it
    //! also starts at login without the app.

    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    use anyhow::Context as _;

    use crate::state::Paths;

    pub const UNIT: &str = "zapret-hub-tg-ws";

    fn unit_path() -> PathBuf {
        zapret::config_home()
            .join("systemd/user")
            .join(format!("{UNIT}.service"))
    }

    /// The unit that runs `argv` (quoted for systemd: no variable or specifier expansion).
    pub fn unit_text(argv: &[String]) -> String {
        format!(
            "[Unit]\n\
             Description=Zapret Hub: TG WS Proxy (Telegram over WebSocket)\n\
             After=network-online.target\n\
             \n\
             [Service]\n\
             ExecStart={}\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            zapret::service::exec_line(argv)
        )
    }

    fn systemctl(args: &[&str]) -> anyhow::Result<exec::Output> {
        let mut cmd = Command::new("systemctl");
        cmd.arg("--user").args(args);
        exec::run(&mut cmd, Duration::from_secs(30), None)
            .with_context(|| format!("systemctl --user {} did not run", args.join(" ")))
    }

    fn check(args: &[&str]) -> anyhow::Result<()> {
        systemctl(args)?.check()?;
        Ok(())
    }

    /// The unit written for `argv`, enabled, and (re)started into it.
    pub fn start(_paths: &Paths, argv: &[String]) -> anyhow::Result<()> {
        let path = unit_path();
        anyhow::ensure!(
            path.is_absolute(),
            "HOME is not set: there is no systemd user folder"
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, unit_text(argv))
            .with_context(|| format!("cannot write {}", path.display()))?;
        check(&["daemon-reload"])?;
        check(&["enable", UNIT])?;
        check(&["restart", UNIT])
    }

    /// Stopped and disabled; a unit that was never made is no error.
    pub fn stop() {
        if let Err(err) = systemctl(&["disable", "--now", UNIT]) {
            log::warn!("{err:#}");
        }
    }

    pub fn running() -> bool {
        systemctl(&["is-active", "--quiet", UNIT]).is_ok_and(|out| out.success())
    }

    pub fn logs(_paths: &Paths) -> String {
        let mut cmd = Command::new("journalctl");
        cmd.args([
            "--user",
            "-u",
            UNIT,
            "-n",
            "30",
            "--no-pager",
            "--no-hostname",
        ]);
        match exec::run(&mut cmd, Duration::from_secs(10), None) {
            Ok(out) if out.success() => out.stdout.trim().to_owned(),
            _ => String::new(),
        }
    }
}

#[cfg(windows)]
mod run {
    //! The app's own child, ended with the app (`exec::Owned`'s job object), its log in
    //! `<data>\logs\tg-ws-proxy.log`.

    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;

    use crate::state::{Paths, lock};

    static CHILD: Mutex<Option<exec::Owned>> = Mutex::new(None);

    fn log_file(paths: &Paths) -> PathBuf {
        paths.logs().join("tg-ws-proxy.log")
    }

    /// The running one ended, then `argv` started with its log file fresh.
    pub fn start(paths: &Paths, argv: &[String]) -> anyhow::Result<()> {
        let mut child = lock(&CHILD);
        *child = None;
        let log = log_file(paths);
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let _ = std::fs::remove_file(&log);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .arg("--log-file")
            .arg(&log)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        *child = Some(exec::Owned::spawn(&mut cmd)?);
        Ok(())
    }

    pub fn stop() {
        *lock(&CHILD) = None;
    }

    pub fn running() -> bool {
        lock(&CHILD).as_mut().is_some_and(exec::Owned::running)
    }

    /// The last 30 lines of the log file.
    pub fn logs(paths: &Paths) -> String {
        let text = std::fs::read_to_string(log_file(paths)).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// Everything the card shows: the disk, the process and the log in one go.
pub fn status(app: &AppHandle) -> TgWsStatus {
    let state = actions::state(app);
    let settings = state.settings();
    TgWsStatus {
        installed: bin(&state.paths).is_file(),
        version: version(&state.paths),
        enabled: settings.tg_ws_enabled,
        running: run::running(),
        unavailable: lock(&UNAVAILABLE).clone(),
        port: settings.tg_ws_port,
        link: link(&settings),
        secret: settings.tg_ws_secret,
        args: settings.tg_ws_args,
        logs: run::logs(&state.paths),
    }
}

/// Started (or restarted) with the settings in force; the binary must be there.
fn start(paths: &Paths, settings: &Settings) -> anyhow::Result<()> {
    validate(settings)?;
    let bin = bin(paths);
    anyhow::ensure!(
        bin.is_file(),
        "TG WS Proxy is not downloaded yet: download it first"
    );
    run::start(paths, &argv(&bin, settings))
}

/// The settings changed by `with`, saved in the settings' own turn, and announced.
fn save_settings(app: &AppHandle, with: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
    let state = actions::state(app);
    let _saving = state.saving();
    let mut settings = state.settings();
    with(&mut settings);
    crate::settings::save(&state.paths.settings(), &settings)?;
    state.set_settings(settings);
    changed(app);
    Ok(())
}

/// The only writer of `tg_ws_enabled`: on starts it (it must be downloaded), off stops it.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> anyhow::Result<TgWsStatus> {
    let state = actions::state(app);
    if enabled {
        let mut settings = state.settings();
        ensure_secret(&mut settings);
        start(&state.paths, &settings)?;
        save_settings(app, |saved| {
            saved.tg_ws_enabled = true;
            saved.tg_ws_secret = settings.tg_ws_secret;
        })?;
    } else {
        run::stop();
        save_settings(app, |saved| saved.tg_ws_enabled = false)?;
    }
    Ok(status(app))
}

/// The port, the secret and the extra flags; a running proxy restarts into them.
pub fn save_config(
    app: &AppHandle,
    port: u16,
    secret: String,
    args: String,
) -> anyhow::Result<TgWsStatus> {
    let state = actions::state(app);
    let settings = Settings {
        tg_ws_port: port,
        tg_ws_secret: secret.trim().to_ascii_lowercase(),
        tg_ws_args: args.trim().to_owned(),
        ..state.settings()
    };
    validate(&settings)?;
    if settings.tg_ws_enabled && bin(&state.paths).is_file() {
        start(&state.paths, &settings)?;
    }
    save_settings(app, |saved| {
        saved.tg_ws_port = settings.tg_ws_port;
        saved.tg_ws_secret = settings.tg_ws_secret;
        saved.tg_ws_args = settings.tg_ws_args;
    })?;
    Ok(status(app))
}

/// The CLI pipe changes no flags on Windows: there the proxy runs as a child of the elevated app
/// and any local program can reach the pipe, the boundary `settings::to_save` keeps for the
/// zapret2 folder. Linux runs it as the user's own unit, so the pipe may.
pub fn check_pipe_args(current: &str, sent: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !cfg!(windows) || sent.split_whitespace().eq(current.split_whitespace()),
        "the TG WS Proxy flags can be changed only in the Zapret Hub window on Windows"
    );
    Ok(())
}

/// The latest release over the installed one, one download at a time with the engine's; a proxy
/// that is on starts again from it.
pub fn install(app: &AppHandle) -> anyhow::Result<TgWsStatus> {
    let state = actions::state(app);
    let socks5 = state.socks5();
    let dir = dir(&state.paths);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    // Windows cannot replace a running exe.
    run::stop();
    let downloaded = state.download(|cancel| download(&dir, socks5.as_deref(), cancel));
    let settings = state.settings();
    if settings.tg_ws_enabled && bin(&state.paths).is_file() {
        start(&state.paths, &settings)?;
    }
    downloaded?;
    changed(app);
    Ok(status(app))
}

/// At the app's start: a proxy that is on but not running comes up (on Windows, every start).
pub fn raise(app: &AppHandle) -> anyhow::Result<()> {
    let state = actions::state(app);
    let settings = state.settings();
    if settings.tg_ws_enabled && !run::running() {
        start(&state.paths, &settings)?;
    }
    Ok(())
}

/// The `tg://proxy` link to the system's handler: Telegram Desktop adds the proxy.
pub fn open_link(app: &AppHandle) -> anyhow::Result<()> {
    crate::shell::open(std::ffi::OsStr::new(&link(&actions::state(app).settings())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[(&str, &str)]) -> String {
        let assets: Vec<_> = assets
            .iter()
            .map(|(name, url)| serde_json::json!({"name": name, "browser_download_url": url}))
            .collect();
        serde_json::json!({"tag_name": tag, "assets": assets}).to_string()
    }

    /// The asset names of the real release v2.5.6 (abridged).
    const NAMES: [&str; 4] = [
        "SHA256SUMS",
        "tg-ws-proxy-x86_64-unknown-linux-musl-upx.tar.gz",
        "tg-ws-proxy-x86_64-unknown-linux-musl.tar.gz",
        "tg-ws-proxy-x86_64-pc-windows-gnu.zip",
    ];

    fn url(name: &str) -> String {
        format!("{DOWNLOAD_PREFIX}v2.5.6/{name}")
    }

    #[test]
    fn picks_this_systems_build_of_the_same_repository() {
        let assets: Vec<(&str, String)> = NAMES.iter().map(|n| (*n, url(n))).collect();
        let assets: Vec<(&str, &str)> = assets.iter().map(|(n, u)| (*n, u.as_str())).collect();
        let (tag, picked) = select(&release("v2.5.6", &assets)).unwrap();
        assert_eq!(tag, "v2.5.6");
        let expected = if cfg!(windows) {
            "tg-ws-proxy-x86_64-pc-windows-gnu.zip"
        } else {
            "tg-ws-proxy-x86_64-unknown-linux-musl.tar.gz"
        };
        assert_eq!(picked, Some((url(expected), Sum::File(url("SHA256SUMS")))));
    }

    #[test]
    fn without_sha256sums_the_asset_digest_checks_it_and_without_both_it_is_refused() {
        let hash = "AB".repeat(32);
        let json = serde_json::json!({"tag_name": "v1", "assets": [
            {"name": ASSET, "browser_download_url": url(ASSET), "digest": format!("sha256:{hash}")}
        ]});
        let (_, picked) = select(&json.to_string()).unwrap();
        assert_eq!(picked, Some((url(ASSET), Sum::Digest(hash.to_lowercase()))));
        let bare = select(&release("v1", &[(ASSET, &url(ASSET))])).unwrap_err();
        assert!(
            bare.to_string().contains("no SHA256SUMS or digest"),
            "{bare}"
        );
        let odd = serde_json::json!({"tag_name": "v1", "assets": [
            {"name": ASSET, "browser_download_url": url(ASSET), "digest": "md5:abc"}
        ]});
        assert!(select(&odd.to_string()).is_err());
    }

    #[test]
    fn sha256sums_lists_the_asset_by_its_file_name() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let sums =
            format!("{a}  tg-ws-proxy-x86_64-unknown-linux-musl-upx.tar.gz\n{b} *dist/{ASSET}\n");
        assert_eq!(listed(&sums), Some(b));
        assert_eq!(listed(&format!("{a}  other.zip\n")), None);
    }

    #[test]
    fn a_release_without_this_systems_build_says_so() {
        let other = "tg-ws-proxy-aarch64-apple-darwin.tar.gz";
        let (tag, picked) = select(&release("v9", &[(other, &url(other))])).unwrap();
        assert_eq!((tag.as_str(), picked), ("v9", None));
    }

    #[test]
    fn a_foreign_url_an_odd_tag_or_not_json_is_refused() {
        let foreign = format!("https://example.com/{ASSET}");
        assert!(select(&release("v1", &[(ASSET, &foreign)])).is_err());
        let foreign_sums = "https://example.com/SHA256SUMS";
        let with_foreign_sums = [(ASSET, url(ASSET)), ("SHA256SUMS", foreign_sums.into())];
        let with_foreign_sums: Vec<(&str, &str)> = with_foreign_sums
            .iter()
            .map(|(n, u)| (*n, u.as_str()))
            .collect();
        assert!(select(&release("v1", &with_foreign_sums)).is_err());
        let odd = select(&release("../..", &[])).unwrap_err();
        assert!(odd.to_string().contains("unexpected release tag"), "{odd}");
        assert!(select("<html>").is_err());
    }

    #[test]
    fn the_pipe_changes_the_flags_only_where_the_proxy_is_not_elevated() {
        assert!(check_pipe_args("--dc-ip 2:1.2.3.4  -v", " --dc-ip 2:1.2.3.4 -v").is_ok());
        assert!(check_pipe_args("", "").is_ok());
        assert_eq!(check_pipe_args("-v", "-v --evil x").is_err(), cfg!(windows));
        assert_eq!(check_pipe_args("-v", "").is_err(), cfg!(windows));
    }

    #[test]
    fn a_secret_is_32_hex_characters_made_once() {
        let mut settings = Settings::default();
        assert!(ensure_secret(&mut settings));
        let secret = settings.tg_ws_secret.clone();
        assert_eq!(secret.len(), 32);
        assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()), "{secret}");
        assert!(validate(&settings).is_ok());
        assert!(!ensure_secret(&mut settings), "made once");
        assert_eq!(settings.tg_ws_secret, secret);
        let mut other = Settings::default();
        ensure_secret(&mut other);
        assert_ne!(other.tg_ws_secret, secret, "random");
    }

    #[test]
    fn the_link_has_the_dd_secret_of_the_local_listener() {
        let settings = Settings {
            tg_ws_port: 1443,
            tg_ws_secret: "ab".repeat(16),
            ..Settings::default()
        };
        assert_eq!(
            link(&settings),
            format!(
                "tg://proxy?server=127.0.0.1&port=1443&secret=dd{}",
                "ab".repeat(16)
            )
        );
    }

    #[test]
    fn a_bad_port_or_secret_is_refused() {
        let good = Settings {
            tg_ws_secret: "AB".repeat(16),
            ..Settings::default()
        };
        assert!(validate(&good).is_ok());
        for bad in [
            Settings {
                tg_ws_port: 0,
                ..good.clone()
            },
            Settings {
                tg_ws_secret: "ab".repeat(15),
                ..good.clone()
            },
            Settings {
                tg_ws_secret: "g".repeat(32),
                ..good.clone()
            },
        ] {
            assert!(validate(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn it_listens_on_localhost_with_the_extra_flags_after() {
        let settings = Settings {
            tg_ws_port: 1443,
            tg_ws_secret: "ab".repeat(16),
            tg_ws_args: " --dc-ip 2:149.154.167.220  -v ".into(),
            ..Settings::default()
        };
        let secret = "ab".repeat(16);
        assert_eq!(
            argv(Path::new("/x/tg-ws-proxy"), &settings),
            [
                "/x/tg-ws-proxy",
                "--host",
                "127.0.0.1",
                "--port",
                "1443",
                "--secret",
                secret.as_str(),
                "--dc-ip",
                "2:149.154.167.220",
                "-v"
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn the_user_unit_runs_the_argv_as_given() {
        let argv: Vec<String> = [
            "/home/me/.local/share/zapret-hub/tg-ws-proxy/tg-ws-proxy",
            "--x",
            "a$b%c",
        ]
        .map(str::to_owned)
        .to_vec();
        let text = run::unit_text(&argv);
        assert!(
            text.contains(
                "ExecStart=\"/home/me/.local/share/zapret-hub/tg-ws-proxy/tg-ws-proxy\" \"--x\" \"a$$b%%c\"\n"
            ),
            "{text}"
        );
        assert!(text.contains("Restart=on-failure\n"), "{text}");
        assert!(text.contains("WantedBy=default.target\n"), "{text}");
    }
}

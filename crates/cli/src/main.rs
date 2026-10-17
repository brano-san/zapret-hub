//! `zapret-hub-cli`: one command to the running Zapret Hub, one JSON document on stdout, fixed
//! exit codes (CLI spec 6, 7).

use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use cli::{Live, Outcome, PROTOCOL, Request, Response, TestsStatus};
use probe::curl::{CheckStatus, DpiStatus};
use router::rules::{Preset, parse_rule_file};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tester::catalog::StrategyRow;
use tester::model::{Pass, Suite, TestRun};
use tester::run::Request as TestRequest;
use zapret::checks::CheckAction;
use zapret::custom::CustomStrategy;
use zapret::fakes::FakeKind;
use zapret::game_filter::{GameFilter, GameFilterMode};
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

/// The app ran the command and it failed, or the test run failed.
const FAILED: u8 = 1;
/// Bad arguments, or `--yes` missing.
const USAGE: u8 = 2;
/// The app is not running, refused (the CLI is off, another protocol) or cannot be read: fix the
/// setup, not the command.
const UNAVAILABLE: u8 = 3;
/// `tests wait` or `tests run --wait` reached `--timeout`.
const TIMED_OUT: u8 = 4;

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_PIPE_BUSY: i32 = 231;

/// How long a busy pipe is tried again (spec 7).
const BUSY: Duration = Duration::from_secs(10);
/// How long `start` waits for the pipe after the task ran (spec 4).
const START_WAIT: Duration = Duration::from_secs(30);
/// How often `tests wait` asks (spec 7.1).
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Parser)]
#[command(
    name = "zapret-hub-cli",
    about = "Controls the running Zapret Hub. Prints one JSON document.",
    after_help = "Exit codes: 0 done; 1 the command failed; 2 bad arguments or no --yes; \
                  3 Zapret Hub not running, the CLI off in its Settings, or another protocol; \
                  4 --timeout reached."
)]
struct Cli {
    /// Confirm what the window asks to confirm: custom delete, zapret2 delete, hosts apply, checks
    /// fix remove-windivert|remove-conflicts, discord clear-cache, service remove, routes remove.
    #[arg(long, global = true)]
    yes: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Service, strategy, last quick check and test run.
    Status,
    /// Versions of this CLI, of its protocol and of the running app.
    Version,
    /// Start Zapret Hub through its autostart task and wait up to 30 s for it.
    Start,
    /// zapret-hub.toml.
    #[command(subcommand)]
    Settings(SettingsCommand),
    /// SOCKS5 proxy.
    #[command(subcommand)]
    Proxy(ProxyCommand),
    /// Zapret folders and SOCKS5 proxies found on this computer.
    Candidates,
    /// Flowseal's strategies, then zapret2's.
    #[command(subcommand)]
    Strategies(StrategiesCommand),
    /// Install a strategy as its engine's service (zapret or zapret2), then run a quick check.
    Apply { name: String },
    /// Apply the best strategy of the newest standard test run.
    ApplyBest,
    /// Restart the active service (zapret or zapret2), then run a quick check.
    Restart,
    /// Check that zapret works now.
    QuickCheck,
    /// Custom strategies.
    #[command(subcommand)]
    Custom(CustomCommand),
    /// zapret2: its folder and your own strategies (zapret2:<name>).
    #[command(subcommand)]
    Zapret2(Zapret2Command),
    /// Strategy test runs.
    #[command(subcommand)]
    Tests(TestsCommand),
    /// Flowseal's settings, lists and fakes.
    Flowseal {
        #[command(subcommand)]
        command: Option<FlowsealCommand>,
    },
    /// Set the game filter; ports not given stay as they are.
    GameFilter {
        mode: FilterMode,
        /// TCP ports, e.g. 1024-65535.
        #[arg(long)]
        tcp: Option<String>,
        /// UDP ports, e.g. 1024-65535.
        #[arg(long)]
        udp: Option<String>,
    },
    /// The ipset-all list.
    #[command(subcommand)]
    Ipset(IpsetCommand),
    /// Flowseal's own update check.
    CheckUpdates { state: Switch },
    /// Make a file of bin\ (its name without .bin) the active fake.
    Fake { kind: Fake, name: String },
    /// The user lists.
    #[command(subcommand)]
    Lists(ListsCommand),
    /// The zapret-hub block in hosts.
    #[command(subcommand)]
    Hosts(HostsCommand),
    /// service.bat's diagnostics.
    Checks {
        #[command(subcommand)]
        command: Option<ChecksCommand>,
    },
    /// Discord.
    #[command(subcommand)]
    Discord(DiscordCommand),
    /// The zapret service.
    #[command(subcommand)]
    Service(ServiceCommand),
    /// Route UDP through the local SOCKS5 proxy.
    #[command(subcommand)]
    Router(RouterCommand),
    /// The routing rules of data\routes.toml.
    #[command(subcommand)]
    Routes(RoutesCommand),
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    /// All settings.
    Get,
    /// Change one setting; an empty value clears socks5. The zapret and zapret2 folders are
    /// changed only in the Zapret Hub window.
    Set { key: Key, value: String },
}

#[derive(Debug, Subcommand)]
enum ProxyCommand {
    /// Check an address, or the socks5 setting when none is given.
    Check { addr: Option<String> },
}

#[derive(Debug, Subcommand)]
enum StrategiesCommand {
    /// Every strategy in service.bat's order with its last test result.
    List,
    /// Arguments, last result and, for a custom strategy, its record.
    Show { name: String },
}

#[derive(Debug, Subcommand)]
enum CustomCommand {
    /// Copy a strategy as "general (HUB <name>)" and print its record.
    Clone { source: String, name: String },
    /// Check a CustomStrategy JSON (a file, or - for stdin) without saving it.
    Validate { file: PathBuf },
    /// Save a CustomStrategy JSON (a file, or - for stdin).
    Save {
        file: PathBuf,
        /// A new strategy rather than a change to one.
        #[arg(long)]
        create: bool,
    },
    /// Delete a custom strategy by its own name: mine for "general (HUB mine)" (needs --yes).
    Delete { name: String },
}

#[derive(Debug, Subcommand)]
enum Zapret2Command {
    /// The zapret2 folder, its version and Lua compatibility, or why it cannot be used.
    Info,
    /// Check a strategy's text (a file, or - for stdin) with winws2, without saving it.
    Validate { file: PathBuf },
    /// Save a strategy's text (a file, or - for stdin) as zapret2:<name>.
    Save {
        name: String,
        file: PathBuf,
        /// A new strategy rather than a change to one.
        #[arg(long)]
        create: bool,
    },
    /// Delete one of your zapret2 strategies by its own name: mine for zapret2:mine (needs --yes).
    Delete { name: String },
}

#[derive(Debug, Subcommand)]
enum TestsCommand {
    /// Start a run of the named strategies, or of every one that parses.
    Run {
        /// The DPI suite instead of the standard one.
        #[arg(long)]
        dpi: bool,
        /// Wait for the end and print the ranking.
        #[arg(long)]
        wait: bool,
        /// Stop waiting after this many seconds (exit code 4); the run goes on.
        #[arg(long, requires = "wait")]
        timeout: Option<u64>,
        /// Passes over the list (1-5): each strategy is measured this many times and ranked by
        /// the aggregate.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=5))]
        repeats: u32,
        /// Strategy names in the order to run them; none: every strategy that parses.
        names: Vec<String>,
    },
    /// Whether a run is going, its progress and how the last one ended.
    Status,
    /// Wait for the run to end and print its ranking.
    Wait {
        /// Stop waiting after this many seconds (exit code 4); the run goes on.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Cancel the run.
    Cancel,
    /// The saved runs.
    List,
    /// A run's ranking; --full prints every check.
    Show {
        id: String,
        #[arg(long)]
        full: bool,
        /// Instead: every check that did not pass, over all passes, counted per strategy.
        #[arg(long, conflicts_with = "full")]
        failures: bool,
    },
    /// A run as test zapret.ps1 writes it (text).
    Report { id: String },
}

#[derive(Debug, Subcommand)]
enum FlowsealCommand {
    /// Look for a newer Flowseal version.
    CheckUpdate,
}

#[derive(Debug, Subcommand)]
enum IpsetCommand {
    /// Switch the mode as service.bat does.
    Mode { mode: Ipset },
    /// Download ipset-all.txt again.
    Update,
}

#[derive(Debug, Subcommand)]
enum ListsCommand {
    Add { list: List, entry: String },
    Remove { list: List, entry: String },
}

#[derive(Debug, Subcommand)]
enum HostsCommand {
    /// What applying would change; nothing is written.
    Plan,
    /// Write the zapret-hub block into hosts (needs --yes).
    Apply,
}

#[derive(Debug, Subcommand)]
enum ChecksCommand {
    /// Apply a fix; remove-windivert and remove-conflicts need --yes.
    Fix { action: Fix },
}

#[derive(Debug, Subcommand)]
enum DiscordCommand {
    /// Close Discord and clear its caches (needs --yes).
    ClearCache,
}

#[derive(Debug, Subcommand)]
enum RouterCommand {
    /// Start routing.
    On,
    /// Stop routing.
    Off,
    /// Whether it runs, the proxy and the counters.
    Status,
}

#[derive(Debug, Subcommand)]
enum RoutesCommand {
    /// The rules, their state and what is wrong with the file.
    List,
    /// One rule.
    Show {
        name: String,
    },
    /// Add a rule from a TOML file with a [rule] table (- is stdin).
    Add {
        name: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Replace a rule from a TOML file with a [rule] table (- is stdin).
    Edit {
        name: String,
        #[arg(long)]
        file: PathBuf,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    /// Delete a rule (needs --yes).
    Remove {
        name: String,
    },
    /// Add a ready-made rule.
    Preset {
        preset: PresetArg,
    },
    /// The open flows and those closed in the last 30 s; --all adds flows without a rule (seen for app rules).
    Flows {
        #[arg(long)]
        all: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PresetArg {
    DiscordVoice,
    Discord,
    Dbd,
}

impl From<PresetArg> for Preset {
    fn from(preset: PresetArg) -> Self {
        match preset {
            PresetArg::DiscordVoice => Preset::DiscordVoice,
            PresetArg::Discord => Preset::Discord,
            PresetArg::Dbd => Preset::Dbd,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Remove the zapret or zapret2 service and WinDivert (needs --yes).
    Remove,
}

/// The keys of zapret-hub.toml the CLI may change (spec 7.2). Not `zapret_dir` or `zapret2_dir`:
/// the SYSTEM service runs that folder's winws.exe or winws2.exe, so the app takes a new one only
/// from its window (spec 5; zapret2 spec 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
enum Key {
    Socks5,
    Autostart,
    StartMinimized,
    Theme,
    CliEnabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FilterMode {
    Disabled,
    All,
    Tcp,
    Udp,
}

impl From<FilterMode> for GameFilterMode {
    fn from(mode: FilterMode) -> Self {
        match mode {
            FilterMode::Disabled => Self::Disabled,
            FilterMode::All => Self::All,
            FilterMode::Tcp => Self::Tcp,
            FilterMode::Udp => Self::Udp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Ipset {
    Loaded,
    None,
    Any,
}

impl From<Ipset> for IpsetMode {
    fn from(mode: Ipset) -> Self {
        match mode {
            Ipset::Loaded => Self::Loaded,
            Ipset::None => Self::None,
            Ipset::Any => Self::Any,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Fake {
    Discord,
    Game,
}

impl From<Fake> for FakeKind {
    fn from(kind: Fake) -> Self {
        match kind {
            Fake::Discord => Self::Discord,
            Fake::Game => Self::Game,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum List {
    General,
    Exclude,
    IpsetExclude,
}

impl From<List> for UserList {
    fn from(list: List) -> Self {
        match list {
            List::General => Self::General,
            List::Exclude => Self::Exclude,
            List::IpsetExclude => Self::IpsetExclude,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Fix {
    EnableTcpTimestamps,
    RemoveWindivert,
    RemoveConflicts,
}

impl From<Fix> for CheckAction {
    fn from(fix: Fix) -> Self {
        match fix {
            Fix::EnableTcpTimestamps => Self::EnableTcpTimestamps,
            Fix::RemoveWindivert => Self::RemoveWinDivert,
            Fix::RemoveConflicts => Self::RemoveConflicts,
        }
    }
}

/// What goes to stdout on success.
#[derive(Debug, PartialEq)]
enum Done {
    Json(Value),
    /// `tests report`: the text as the app sent it.
    Text(String),
}

/// `{"error": …}` on stdout, and the exit code.
#[derive(Debug, PartialEq)]
struct Fail {
    code: u8,
    error: String,
}

impl Fail {
    fn new(code: u8, error: impl Into<String>) -> Self {
        Self {
            code,
            error: error.into(),
        }
    }
}

/// One request to the app and its answer's `data`.
type Call<'a> = &'a mut dyn FnMut(Request) -> Result<Value, Fail>;
/// Where progress lines go: stderr, so that stdout stays one JSON document.
type Note<'a> = &'a mut dyn FnMut(&str);

fn main() -> ExitCode {
    let args = match Cli::try_parse() {
        Ok(args) => args,
        // --help, as clap prints it.
        Err(err) if !err.use_stderr() => err.exit(),
        Err(err) => return finish(Err(Fail::new(USAGE, err.render().to_string()))),
    };
    finish(run(
        args,
        &mut |request| answer(send(&request)),
        &mut |line| {
            let _ = writeln!(io::stderr(), "{line}");
        },
    ))
}

fn send(request: &Request) -> io::Result<Vec<u8>> {
    cli::pipe::request(cli::pipe::NAME, cli::encode(request).as_bytes(), BUSY)
}

/// Prints the outcome and turns it into the exit code (spec 7).
fn finish(outcome: Result<Done, Fail>) -> ExitCode {
    let (text, code) = match outcome {
        Ok(Done::Json(value)) => (pretty(&value), 0),
        Ok(Done::Text(text)) => (text, 0),
        Err(fail) => (pretty(&json!({ "error": fail.error })), fail.code),
    };
    // A closed stdout is the caller's business, not a panic.
    let _ = io::stdout().lock().write_all(text.as_bytes());
    ExitCode::from(code)
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("a JSON value prints") + "\n"
}

/// The app's answer line → its `data`, or the exit code it means (spec 6, 7).
fn answer(line: io::Result<Vec<u8>>) -> Result<Value, Fail> {
    let line = line.map_err(|err| Fail::new(UNAVAILABLE, unavailable(&err)))?;
    let response: Response = serde_json::from_slice(&line).map_err(foreign)?;
    match response {
        Response { ok: true, data, .. } => Ok(data),
        Response { refused, error, .. } => Err(Fail::new(
            if refused { UNAVAILABLE } else { FAILED },
            error.unwrap_or_default(),
        )),
    }
}

fn unavailable(err: &io::Error) -> String {
    match err.raw_os_error() {
        // Also an app whose pipe stopped: `start` would only show its window.
        Some(ERROR_FILE_NOT_FOUND) => {
            "Zapret Hub is not running: start it with `zapret-hub-cli start` or run zapret-hub.exe \
             (if it is in the tray, its CLI pipe is closed: quit it from the tray and start it again)"
                .to_owned()
        }
        Some(ERROR_ACCESS_DENIED) => "Zapret Hub's pipe did not let this program in: it answers \
                                       only the Windows user who started Zapret Hub, outside \
                                       sandboxes"
            .to_owned(),
        Some(ERROR_PIPE_BUSY) => "Zapret Hub's pipe stayed busy for 10 s: try again".to_owned(),
        // `pipe::request` had sent the whole line: the app may have run it before it stopped.
        None if err.kind() == io::ErrorKind::UnexpectedEof => {
            "Zapret Hub closed the connection without a full answer, so the command may have run: \
             check with `zapret-hub-cli status`"
                .to_owned()
        }
        _ => format!("cannot reach Zapret Hub: {err}"),
    }
}

/// An answer of another shape: the CLI and the app do not match.
fn foreign(err: serde_json::Error) -> Fail {
    Fail::new(
        UNAVAILABLE,
        format!("Zapret Hub sent an answer this CLI does not read: {err}"),
    )
}

fn read<T: DeserializeOwned>(value: Value) -> Result<T, Fail> {
    serde_json::from_value(value).map_err(foreign)
}

/// What the window confirms before it runs it (spec 6).
fn needs_yes(command: &Command) -> bool {
    matches!(
        command,
        Command::Custom(CustomCommand::Delete { .. })
            | Command::Zapret2(Zapret2Command::Delete { .. })
            | Command::Hosts(HostsCommand::Apply)
            | Command::Checks {
                command: Some(ChecksCommand::Fix {
                    action: Fix::RemoveWindivert | Fix::RemoveConflicts
                })
            }
            | Command::Discord(DiscordCommand::ClearCache)
            | Command::Service(ServiceCommand::Remove)
            | Command::Routes(RoutesCommand::Remove { .. })
    )
}

fn run(args: Cli, call: Call, note: Note) -> Result<Done, Fail> {
    if needs_yes(&args.command) && !args.yes {
        return Err(Fail::new(
            USAGE,
            "the window asks to confirm this: add --yes",
        ));
    }
    let request = match args.command {
        Command::Version => {
            return Ok(Done::Json(json!({
                "cli": env!("CARGO_PKG_VERSION"),
                "protocol": PROTOCOL,
                "app": call(Request::AppVersion).ok(),
            })));
        }
        Command::Start => return start(),
        Command::Settings(SettingsCommand::Set { key, value }) => {
            let value = setting(key, &value)?;
            let mut settings = call(Request::GetSettings)?;
            settings
                .as_object_mut()
                .ok_or_else(|| {
                    Fail::new(
                        UNAVAILABLE,
                        "Zapret Hub sent settings that are not an object",
                    )
                })?
                .insert(key_name(key), value);
            Request::SaveSettings { settings }
        }
        Command::Proxy(ProxyCommand::Check { addr: None }) => {
            let addr = call(Request::GetSettings)?["socks5"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| {
                    Fail::new(USAGE, "no address given and no socks5 in the settings")
                })?;
            Request::CheckProxy { addr }
        }
        Command::GameFilter { mode, tcp, udp } => {
            let (tcp, udp) = match (tcp, udp) {
                (Some(tcp), Some(udp)) => (tcp, udp),
                (tcp, udp) => {
                    let now: GameFilter = read(call(Request::GetFlowseal)?["game_filter"].clone())?;
                    (tcp.unwrap_or(now.tcp), udp.unwrap_or(now.udp))
                }
            };
            Request::SetGameFilter {
                filter: GameFilter {
                    mode: mode.into(),
                    tcp,
                    udp,
                },
            }
        }
        Command::Custom(CustomCommand::Validate { file }) => Request::ValidateCustom {
            strategy: read_strategy(&file)?,
        },
        Command::Custom(CustomCommand::Save { file, create }) => Request::SaveCustom {
            strategy: read_strategy(&file)?,
            create,
        },
        Command::Zapret2(Zapret2Command::Validate { file }) => Request::ValidateZapret2 {
            text: read_text(&file)?,
        },
        Command::Zapret2(Zapret2Command::Save { name, file, create }) => Request::SaveZapret2 {
            name,
            text: read_text(&file)?,
            create,
        },
        Command::Tests(TestsCommand::Run {
            dpi,
            wait,
            timeout,
            repeats,
            names,
        }) => {
            let strategies = if names.is_empty() {
                runnable(call(Request::ListStrategies)?)?
            } else {
                names
            };
            let suite = if dpi { Suite::Dpi } else { Suite::Standard };
            let count = strategies.len();
            call(Request::StartTests {
                request: TestRequest {
                    suite,
                    strategies,
                    repeats,
                },
            })?;
            if wait {
                return wait_for_run(call, timeout, note);
            }
            return Ok(Done::Json(json!({
                "started": true,
                "suite": suite_name(suite),
                "strategies": count,
                "repeats": repeats,
            })));
        }
        Command::Tests(TestsCommand::Wait { timeout }) => {
            return wait_for_run(call, timeout, note);
        }
        Command::Tests(TestsCommand::Show { id, full, failures }) => {
            return show(call, id, full, failures);
        }
        Command::Tests(TestsCommand::Report { id }) => {
            return Ok(Done::Text(read(call(Request::RunReport { id })?)?));
        }
        // One command, one request: the table of spec 6.
        Command::Status => Request::GetOverview,
        Command::Settings(SettingsCommand::Get) => Request::GetSettings,
        Command::Proxy(ProxyCommand::Check { addr: Some(addr) }) => Request::CheckProxy { addr },
        Command::Candidates => Request::WizardCandidates,
        Command::Strategies(StrategiesCommand::List) => Request::ListStrategies,
        Command::Strategies(StrategiesCommand::Show { name }) => Request::GetStrategy { name },
        Command::Apply { name } => Request::ApplyStrategy { name },
        Command::ApplyBest => Request::ApplyBest,
        Command::Restart => Request::RestartZapret,
        Command::QuickCheck => Request::QuickCheck,
        Command::Custom(CustomCommand::Clone { source, name }) => {
            Request::CloneStrategy { source, name }
        }
        Command::Custom(CustomCommand::Delete { name }) => Request::DeleteCustom { name },
        Command::Zapret2(Zapret2Command::Info) => Request::Zapret2Info,
        Command::Zapret2(Zapret2Command::Delete { name }) => Request::DeleteZapret2 { name },
        Command::Tests(TestsCommand::Status) => Request::TestsStatus,
        Command::Tests(TestsCommand::Cancel) => Request::CancelTests,
        Command::Tests(TestsCommand::List) => Request::ListRuns,
        Command::Flowseal { command: None } => Request::GetFlowseal,
        Command::Flowseal {
            command: Some(FlowsealCommand::CheckUpdate),
        } => Request::CheckFlowsealUpdate,
        Command::Ipset(IpsetCommand::Mode { mode }) => Request::SetIpsetMode { mode: mode.into() },
        Command::Ipset(IpsetCommand::Update) => Request::UpdateIpset,
        Command::CheckUpdates { state } => Request::SetCheckUpdates {
            enabled: state == Switch::On,
        },
        Command::Fake { kind, name } => Request::ReplaceFake {
            kind: kind.into(),
            source: name,
        },
        Command::Lists(ListsCommand::Add { list, entry }) => Request::AddListEntry {
            list: list.into(),
            entry,
        },
        Command::Lists(ListsCommand::Remove { list, entry }) => Request::RemoveListEntry {
            list: list.into(),
            entry,
        },
        Command::Hosts(HostsCommand::Plan) => Request::HostsPlan,
        Command::Hosts(HostsCommand::Apply) => Request::ApplyHosts,
        Command::Checks { command: None } => Request::SystemChecks,
        Command::Checks {
            command: Some(ChecksCommand::Fix { action }),
        } => Request::FixCheck {
            action: action.into(),
        },
        Command::Discord(DiscordCommand::ClearCache) => Request::ClearDiscordCache,
        Command::Service(ServiceCommand::Remove) => Request::RemoveService,
        Command::Router(RouterCommand::On) => Request::SetRouterEnabled { enabled: true },
        Command::Router(RouterCommand::Off) => Request::SetRouterEnabled { enabled: false },
        Command::Router(RouterCommand::Status) => Request::RouterStatus,
        Command::Routes(RoutesCommand::List) => Request::ListRoutes,
        Command::Routes(RoutesCommand::Show { name }) => Request::GetRoute { name },
        Command::Routes(RoutesCommand::Add { name, file }) => Request::SaveRoute {
            rule: read_rule(&name, &file)?,
            create: true,
        },
        Command::Routes(RoutesCommand::Edit { name, file }) => Request::SaveRoute {
            rule: read_rule(&name, &file)?,
            create: false,
        },
        Command::Routes(RoutesCommand::Enable { name }) => Request::SetRouteEnabled {
            name,
            enabled: true,
        },
        Command::Routes(RoutesCommand::Disable { name }) => Request::SetRouteEnabled {
            name,
            enabled: false,
        },
        Command::Routes(RoutesCommand::Remove { name }) => Request::DeleteRoute { name },
        Command::Routes(RoutesCommand::Preset { preset }) => Request::AddRoutePreset {
            preset: preset.into(),
        },
        Command::Routes(RoutesCommand::Flows { all }) => Request::RouteFlows { all },
    };
    // Only `flowseal check-update` has a `null` that means something (no update).
    let silent = !matches!(request, Request::CheckFlowsealUpdate);
    call(request).map(|data| {
        Done::Json(if silent && data.is_null() {
            json!({ "ok": true })
        } else {
            data
        })
    })
}

fn key_name(key: Key) -> String {
    key.to_possible_value()
        .expect("no key is skipped")
        .get_name()
        .to_owned()
}

/// A value for `settings set`, checked before anything is sent (spec 7.2).
fn setting(key: Key, value: &str) -> Result<Value, Fail> {
    let bad = |what: &str| {
        Fail::new(
            USAGE,
            format!("{} takes {what}, not {value:?}", key_name(key)),
        )
    };
    match key {
        Key::Socks5 if value.trim().is_empty() => Ok(Value::Null),
        Key::Socks5 => Ok(value.into()),
        Key::Autostart | Key::StartMinimized | Key::CliEnabled => value
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|_| bad("true or false")),
        Key::Theme if ["system", "light", "dark"].contains(&value) => Ok(value.into()),
        Key::Theme => Err(bad("system, light or dark")),
    }
}

/// A file, or stdin for `-` (spec 7.3). PowerShell 5.1 and Notepad start the file with a BOM: a
/// UTF-8 one is dropped, and a UTF-16LE one (`>`, `Out-File`) is decoded.
fn read_text(file: &Path) -> Result<String, Fail> {
    let fail = |err: &dyn std::fmt::Display| {
        Fail::new(USAGE, format!("cannot read {}: {err}", file.display()))
    };
    let mut bytes = Vec::new();
    if file == Path::new("-") {
        io::Read::read_to_end(&mut io::stdin(), &mut bytes).map_err(|err| fail(&err))?;
    } else {
        bytes = std::fs::read(file).map_err(|err| fail(&err))?;
    }
    let text = if let Some(utf16) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units = utf16
            .chunks(2)
            .map(|pair| match pair {
                [lo, hi] => Ok(u16::from_le_bytes([*lo, *hi])),
                _ => Err(fail(&"truncated UTF-16 text")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        String::from_utf16(&units).map_err(|err| fail(&err))?
    } else {
        String::from_utf8(bytes).map_err(|err| fail(&err))?
    };
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// One rule from a file with a `[rule]` table, or stdin for `-`; checked before anything is sent.
fn read_rule(name: &str, file: &Path) -> Result<router::rules::Rule, Fail> {
    parse_rule_file(name, &read_text(file)?).map_err(|err| Fail::new(USAGE, err.to_string()))
}

/// A `CustomStrategy` JSON from a file, or from stdin for `-` (spec 7.3).
fn read_strategy(file: &Path) -> Result<CustomStrategy, Fail> {
    serde_json::from_str(&read_text(file)?).map_err(|err| {
        Fail::new(
            USAGE,
            format!(
                "{} is not a CustomStrategy (name, cloned_from, wf, blocks): {err}",
                file.display()
            ),
        )
    })
}

/// `tests run` without names: every strategy that parses, in service.bat's order (spec 7.1), but not
/// the built-in copies of Flowseal's: those run when named.
fn runnable(rows: Value) -> Result<Vec<String>, Fail> {
    let rows: Vec<StrategyRow> = read(rows)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.error.is_none() && !row.flowseal_copy)
        .map(|row| row.name)
        .collect())
}

fn suite_name(suite: Suite) -> &'static str {
    match suite {
        Suite::Standard => "standard",
        Suite::Dpi => "dpi",
    }
}

/// Asks `tests_status` every 2 s until the run is over; the progress goes to stderr (spec 7.1).
fn wait_for_run(call: Call, timeout: Option<u64>, note: Note) -> Result<Done, Fail> {
    // A deadline the clock cannot reach (a `--timeout` near u64::MAX) is no deadline.
    let deadline = timeout.and_then(|secs| Instant::now().checked_add(Duration::from_secs(secs)));
    let mut shown = None;
    loop {
        let status: TestsStatus = read(call(Request::TestsStatus)?)?;
        if !status.testing {
            return match status.last {
                None => Ok(Done::Json(json!({ "last": null }))),
                Some(Outcome::Failed { error }) => Err(Fail::new(FAILED, error)),
                Some(Outcome::Run { run_id }) => show(call, run_id, false, false),
            };
        }
        if let Some(line) = status
            .live
            .as_ref()
            .and_then(|live| progress(live, &mut shown))
        {
            note(&line);
        }
        let left = deadline.map_or(POLL, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if left.is_zero() {
            return Err(Fail::new(
                TIMED_OUT,
                format!(
                    "the test run is still going after {} s: wait again with `zapret-hub-cli tests wait`",
                    timeout.unwrap_or_default()
                ),
            ));
        }
        std::thread::sleep(left.min(POLL));
    }
}

/// `3/33 general (ALT2)`, once per strategy and pass; with repeats `pass 2/3: 3/33 general (ALT2)`.
fn progress(live: &Live, shown: &mut Option<(u32, u32)>) -> Option<String> {
    let index = live.current?;
    let at = (live.pass, index);
    if *shown == Some(at) {
        return None;
    }
    *shown = Some(at);
    let names = &live.request.strategies;
    let pass = if live.request.repeats > 1 {
        format!("pass {}/{}: ", live.pass + 1, live.request.repeats)
    } else {
        String::new()
    };
    Some(format!(
        "{pass}{}/{} {}",
        index + 1,
        names.len(),
        names.get(index as usize)?
    ))
}

fn show(call: Call, id: String, full: bool, failures: bool) -> Result<Done, Fail> {
    let run = call(Request::GetRun { id })?;
    if full {
        return Ok(Done::Json(run));
    }
    if failures {
        return Ok(Done::Json(failures_of(&read(run)?)));
    }
    Ok(Done::Json(compact(&read(run)?)))
}

/// `{strategy: {"<target> <kind> <status>": {count, sample_error}}}` for every check that did not
/// pass, over all passes; `sample_error` is the first line curl wrote (a DPI check has none: its
/// code). A launch failure is the key `launch_error`. Strategies without failures are left out.
fn failures_of(run: &TestRun) -> Value {
    let mut out = serde_json::Map::new();
    for strategy in &run.strategies {
        let mut groups: BTreeMap<String, (u32, Option<String>)> = BTreeMap::new();
        let mut add = |key: String, error: Option<String>| {
            let entry = groups.entry(key).or_default();
            entry.0 += 1;
            entry.1 = entry.1.take().or(error);
        };
        // A single pass keeps its checks in the strategy itself.
        let single;
        let passes: Vec<&Pass> = if strategy.passes.is_empty() {
            single = Pass {
                launch_error: strategy.launch_error.clone(),
                targets: strategy.targets.clone(),
                dpi: strategy.dpi.clone(),
                score: strategy.score.clone(),
            };
            vec![&single]
        } else {
            strategy.passes.iter().collect()
        };
        for pass in passes {
            if let Some(error) = &pass.launch_error {
                add("launch_error".into(), Some(error.clone()));
            }
            for target in &pass.targets {
                for check in target.checks.iter().filter(|c| c.status != CheckStatus::Ok) {
                    let error = check
                        .stderr
                        .lines()
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .map(str::to_owned);
                    add(
                        format!("{} {:?} {:?}", target.name, check.kind, check.status),
                        error,
                    );
                }
            }
            for target in &pass.dpi {
                for check in target.checks.iter().filter(|c| c.status != DpiStatus::Ok) {
                    add(
                        format!("{} {:?} {:?}", target.id, check.kind, check.status),
                        Some(check.code.clone()),
                    );
                }
            }
        }
        if !groups.is_empty() {
            let found: serde_json::Map<String, Value> = groups
                .into_iter()
                .map(|(key, (count, sample))| {
                    (key, json!({ "count": count, "sample_error": sample }))
                })
                .collect();
            out.insert(strategy.name.clone(), Value::Object(found));
        }
    }
    Value::Object(out)
}

/// The ranking without the checks (spec 7.1); a saved run is ranked already.
fn compact(run: &TestRun) -> Value {
    let ranking: Vec<Value> = run
        .strategies
        .iter()
        .enumerate()
        .map(|(i, strategy)| {
            json!({
                "place": i + 1,
                "name": strategy.name,
                "score": strategy.score,
                "launch_error": strategy.launch_error,
            })
        })
        .collect();
    json!({
        "id": run.id,
        "suite": suite_name(run.suite),
        "canceled": run.canceled,
        "restore_error": run.restore_error,
        "ranking": ranking,
    })
}

/// `{"app": <version>}` once the app answers; when it is not running, its autostart task starts
/// it first (spec 4).
fn start() -> Result<Done, Fail> {
    let started = |version| Done::Json(json!({ "app": version }));
    if let Some(version) = running()? {
        return Ok(started(version));
    }
    let ran = exec::run(
        Process::new("schtasks.exe").args(["/Run", "/TN", cli::AUTOSTART_TASK]),
        START_WAIT,
        None,
    );
    if let Err(err) = ran
        .map_err(anyhow::Error::from)
        .and_then(exec::Output::check)
    {
        return Err(Fail::new(
            UNAVAILABLE,
            format!(
                "the autostart task «{}» did not run: start zapret-hub.exe yourself or turn \
                 autostart on in its Settings\n\n{err:#}",
                cli::AUTOSTART_TASK
            ),
        ));
    }
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(version) = running()? {
            return Ok(started(version));
        }
    }
    Err(Fail::new(
        UNAVAILABLE,
        "Zapret Hub did not answer within 30 s of its task starting",
    ))
}

/// The app's version; `None` while there is no pipe.
fn running() -> Result<Option<Value>, Fail> {
    match send(&Request::AppVersion) {
        Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => Ok(None),
        exchanged => answer(exchanged).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;
    use tester::model::{Score, StrategyResult};

    use super::*;

    /// Runs `args` against a fake app that gives `answers` in turn (then `null`); returns the
    /// outcome and what the app was asked.
    fn drive(
        args: &[&str],
        answers: Vec<Result<Value, Fail>>,
    ) -> (Result<Done, Fail>, Vec<Request>) {
        let (outcome, asked, _) = drive_noted(args, answers);
        (outcome, asked)
    }

    /// `drive`, and the progress lines the CLI wrote besides its JSON.
    fn drive_noted(
        args: &[&str],
        answers: Vec<Result<Value, Fail>>,
    ) -> (Result<Done, Fail>, Vec<Request>, Vec<String>) {
        let mut notes = Vec::new();
        let args =
            Cli::try_parse_from(std::iter::once("zapret-hub-cli").chain(args.iter().copied()))
                .unwrap();
        let mut asked = Vec::new();
        let mut answers = answers.into_iter();
        let outcome = run(
            args,
            &mut |request| {
                asked.push(request);
                answers.next().unwrap_or(Ok(Value::Null))
            },
            &mut |line| notes.push(line.to_owned()),
        );
        (outcome, asked, notes)
    }

    fn code(outcome: Result<Done, Fail>) -> u8 {
        outcome.map_or_else(|fail| fail.code, |_| 0)
    }

    #[test]
    fn the_arguments_are_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_sends_its_window_command() {
        let general = || "general (ALT11)".to_owned();
        let table: Vec<(&[&str], Request)> = vec![
            (&["status"], Request::GetOverview),
            (
                &["router", "on"],
                Request::SetRouterEnabled { enabled: true },
            ),
            (
                &["router", "off"],
                Request::SetRouterEnabled { enabled: false },
            ),
            (&["router", "status"], Request::RouterStatus),
            (&["routes", "list"], Request::ListRoutes),
            (
                &["routes", "show", "Discord голос"],
                Request::GetRoute {
                    name: "Discord голос".into(),
                },
            ),
            (
                &["routes", "enable", "v"],
                Request::SetRouteEnabled {
                    name: "v".into(),
                    enabled: true,
                },
            ),
            (
                &["routes", "disable", "v"],
                Request::SetRouteEnabled {
                    name: "v".into(),
                    enabled: false,
                },
            ),
            (
                &["routes", "remove", "v", "--yes"],
                Request::DeleteRoute { name: "v".into() },
            ),
            (
                &["routes", "preset", "discord-voice"],
                Request::AddRoutePreset {
                    preset: Preset::DiscordVoice,
                },
            ),
            (
                &["routes", "preset", "discord"],
                Request::AddRoutePreset {
                    preset: Preset::Discord,
                },
            ),
            (
                &["routes", "preset", "dbd"],
                Request::AddRoutePreset {
                    preset: Preset::Dbd,
                },
            ),
            (&["routes", "flows"], Request::RouteFlows { all: false }),
            (
                &["routes", "flows", "--all"],
                Request::RouteFlows { all: true },
            ),
            (&["settings", "get"], Request::GetSettings),
            (
                &["proxy", "check", "127.0.0.1:2080"],
                Request::CheckProxy {
                    addr: "127.0.0.1:2080".into(),
                },
            ),
            (&["candidates"], Request::WizardCandidates),
            (&["strategies", "list"], Request::ListStrategies),
            (
                &["strategies", "show", "general (ALT11)"],
                Request::GetStrategy { name: general() },
            ),
            (
                &["apply", "general (ALT11)"],
                Request::ApplyStrategy { name: general() },
            ),
            (&["apply-best"], Request::ApplyBest),
            (&["restart"], Request::RestartZapret),
            (&["quick-check"], Request::QuickCheck),
            (
                &["custom", "clone", "general (ALT11)", "mine"],
                Request::CloneStrategy {
                    source: general(),
                    name: "mine".into(),
                },
            ),
            (
                &["custom", "delete", "mine", "--yes"],
                Request::DeleteCustom {
                    name: "mine".into(),
                },
            ),
            (&["zapret2", "info"], Request::Zapret2Info),
            (
                &["zapret2", "delete", "mine", "--yes"],
                Request::DeleteZapret2 {
                    name: "mine".into(),
                },
            ),
            (&["tests", "status"], Request::TestsStatus),
            (&["tests", "cancel"], Request::CancelTests),
            (&["tests", "list"], Request::ListRuns),
            (
                &["tests", "show", "2026-10-10_05-03-34", "--full"],
                Request::GetRun {
                    id: "2026-10-10_05-03-34".into(),
                },
            ),
            (
                &["tests", "report", "2026-10-10_05-03-34"],
                Request::RunReport {
                    id: "2026-10-10_05-03-34".into(),
                },
            ),
            (&["flowseal"], Request::GetFlowseal),
            (&["flowseal", "check-update"], Request::CheckFlowsealUpdate),
            (
                &[
                    "game-filter",
                    "tcp",
                    "--tcp",
                    "80,443",
                    "--udp",
                    "1024-65535",
                ],
                Request::SetGameFilter {
                    filter: GameFilter {
                        mode: GameFilterMode::Tcp,
                        tcp: "80,443".into(),
                        udp: "1024-65535".into(),
                    },
                },
            ),
            (
                &["ipset", "mode", "none"],
                Request::SetIpsetMode {
                    mode: IpsetMode::None,
                },
            ),
            (&["ipset", "update"], Request::UpdateIpset),
            (
                &["check-updates", "off"],
                Request::SetCheckUpdates { enabled: false },
            ),
            (
                &["fake", "game", "quic_initial_www_google_com"],
                Request::ReplaceFake {
                    kind: FakeKind::Game,
                    source: "quic_initial_www_google_com".into(),
                },
            ),
            (
                &["lists", "add", "ipset-exclude", "10.0.0.0/8"],
                Request::AddListEntry {
                    list: UserList::IpsetExclude,
                    entry: "10.0.0.0/8".into(),
                },
            ),
            (
                &["lists", "remove", "general", "example.com"],
                Request::RemoveListEntry {
                    list: UserList::General,
                    entry: "example.com".into(),
                },
            ),
            (&["hosts", "plan"], Request::HostsPlan),
            (&["hosts", "apply", "--yes"], Request::ApplyHosts),
            (&["checks"], Request::SystemChecks),
            (
                &["checks", "fix", "enable-tcp-timestamps"],
                Request::FixCheck {
                    action: CheckAction::EnableTcpTimestamps,
                },
            ),
            (
                &["--yes", "checks", "fix", "remove-windivert"],
                Request::FixCheck {
                    action: CheckAction::RemoveWinDivert,
                },
            ),
            (
                &["checks", "fix", "remove-conflicts", "--yes"],
                Request::FixCheck {
                    action: CheckAction::RemoveConflicts,
                },
            ),
            (
                &["discord", "clear-cache", "--yes"],
                Request::ClearDiscordCache,
            ),
            (&["service", "remove", "--yes"], Request::RemoveService),
        ];
        for (args, expected) in table {
            let (_, asked) = drive(args, Vec::new());
            assert_eq!(asked, [expected], "{args:?}");
        }
    }

    #[test]
    fn what_the_window_confirms_needs_yes() {
        for args in [
            &["custom", "delete", "mine"][..],
            &["zapret2", "delete", "mine"],
            &["hosts", "apply"],
            &["checks", "fix", "remove-windivert"],
            &["checks", "fix", "remove-conflicts"],
            &["discord", "clear-cache"],
            &["service", "remove"],
            &["routes", "remove", "v"],
        ] {
            let (outcome, asked) = drive(args, Vec::new());
            assert_eq!(code(outcome), USAGE, "{args:?}");
            assert!(asked.is_empty(), "{args:?} reached the app");
        }
    }

    #[test]
    fn routes_add_and_edit_read_one_rule_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rule.toml");
        std::fs::write(
            &file,
            "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n",
        )
        .unwrap();
        let path = file.to_str().unwrap();
        let (_, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
        let rule = parse_rule_file(
            "v",
            "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n",
        )
        .unwrap();
        assert_eq!(
            asked,
            [Request::SaveRoute {
                rule: rule.clone(),
                create: true
            }]
        );
        let (_, asked) = drive(&["routes", "edit", "v", "--file", path], Vec::new());
        assert_eq!(
            asked,
            [Request::SaveRoute {
                rule,
                create: false
            }]
        );
        std::fs::write(
            &file,
            "[rule]\nprotocol = \"udp\"\napps = [\"Discord.exe\"]\n",
        )
        .unwrap();
        let (_, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
        let [Request::SaveRoute { rule, create: true }] = asked.as_slice() else {
            panic!("{asked:?}")
        };
        assert_eq!(rule.apps, ["Discord.exe"]);
        std::fs::write(
            &file,
            "[rule]\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\napps = [\"Discord\"]\n",
        )
        .unwrap();
        let (outcome, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
        assert!(asked.is_empty());
        let fail = outcome.err().unwrap();
        assert_eq!(fail.code, 2);
        assert!(fail.error.contains("is not a program"), "{}", fail.error);
        std::fs::write(
            &file,
            "[rule]\nprotocol = \"tcp\"\ndomains = [\"discord.com\"]\n",
        )
        .unwrap();
        let (_, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
        let [Request::SaveRoute { rule, create: true }] = asked.as_slice() else {
            panic!("{asked:?}")
        };
        assert_eq!(rule.domains, ["discord.com"]);
    }

    #[test]
    fn answers_become_exit_codes() {
        let line = |text: &str| Ok(text.as_bytes().to_vec());
        assert_eq!(
            answer(line(r#"{"ok":true,"data":{"app":"0.2.0"}}"#)),
            Ok(json!({ "app": "0.2.0" }))
        );
        assert_eq!(answer(line(r#"{"ok":true}"#)), Ok(Value::Null));
        assert_eq!(
            answer(line(r#"{"ok":false,"error":"no test results yet"}"#)),
            Err(Fail::new(FAILED, "no test results yet"))
        );
        assert_eq!(
            answer(line(
                r#"{"ok":false,"refused":true,"error":"the CLI is off"}"#
            )),
            Err(Fail::new(UNAVAILABLE, "the CLI is off"))
        );
        assert_eq!(code(answer(line("<html>")).map(Done::Json)), UNAVAILABLE);
        let absent = answer(Err(io::Error::from_raw_os_error(ERROR_FILE_NOT_FOUND))).unwrap_err();
        assert_eq!(absent.code, UNAVAILABLE);
        assert!(absent.error.contains("not running"), "{}", absent.error);
        // An app whose pipe stopped is not running for the CLI, and `start` only shows its window.
        assert!(
            absent.error.contains("quit it from the tray"),
            "{}",
            absent.error
        );
        let busy = answer(Err(io::Error::from_raw_os_error(ERROR_PIPE_BUSY))).unwrap_err();
        assert!(busy.error.contains("busy"), "{}", busy.error);
        let denied = answer(Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED))).unwrap_err();
        assert_eq!(denied.code, UNAVAILABLE);
        assert!(denied.error.contains("Windows user"), "{}", denied.error);
        // The pipe's "closed without a full answer" is no os error, and the app may have run the
        // command by then: the text says how to find out.
        let closed = answer(Err(io::ErrorKind::UnexpectedEof.into())).unwrap_err();
        assert_eq!(
            closed,
            Fail::new(
                UNAVAILABLE,
                "Zapret Hub closed the connection without a full answer, so the command may have \
                 run: check with `zapret-hub-cli status`"
            )
        );
    }

    #[test]
    fn bad_arguments_are_refused_by_clap() {
        for args in [
            &["settings", "set", "colour", "red"][..],
            // Only the window moves the folders whose winws.exe and winws2.exe SYSTEM runs.
            &["settings", "set", "zapret_dir", r"C:\Users\me\winws"],
            &["settings", "set", "zapret2_dir", r"D:\zapret2"],
            &["lists", "add", "everything", "x"],
            &["tests", "run", "--timeout", "5"],
            &["frobnicate"],
        ] {
            let parsed =
                Cli::try_parse_from(std::iter::once("zapret-hub-cli").chain(args.iter().copied()));
            assert!(parsed.is_err(), "{args:?}");
        }
        let err = Cli::try_parse_from(["zapret-hub-cli", "settings", "set", "colour", "red"])
            .unwrap_err()
            .render()
            .to_string();
        assert!(
            err.contains("socks5")
                && err.contains("cli_enabled")
                && !err.contains("zapret_dir")
                && !err.contains("zapret2_dir")
                && !err.contains('\u{1b}'),
            "{err}"
        );
    }

    fn settings() -> Value {
        json!({
            "zapret_dir": "E:\\Zapret",
            "zapret2_dir": "D:\\zapret2",
            "socks5": "127.0.0.1:2080",
            "autostart": false,
            "start_minimized": true,
            "theme": "system",
            "wizard_done": true,
            "cli_enabled": true,
        })
    }

    #[test]
    fn settings_set_changes_one_key_and_sends_all_of_them() {
        for (key, value, expected) in [
            ("socks5", "", Value::Null),
            ("socks5", "  ", Value::Null),
            ("theme", "dark", json!("dark")),
            ("cli_enabled", "false", json!(false)),
            ("start_minimized", "false", json!(false)),
        ] {
            let (outcome, asked) = drive(&["settings", "set", key, value], vec![Ok(settings())]);
            assert_eq!(outcome, Ok(Done::Json(json!({ "ok": true }))), "{key}");
            let mut changed = settings();
            changed[key] = expected;
            assert_eq!(
                asked,
                [
                    Request::GetSettings,
                    Request::SaveSettings { settings: changed }
                ]
            );
        }
    }

    #[test]
    fn settings_set_checks_the_value_before_asking() {
        for (key, value) in [
            ("theme", "blue"),
            ("autostart", "yes"),
            ("cli_enabled", "1"),
        ] {
            let (outcome, asked) = drive(&["settings", "set", key, value], vec![Ok(settings())]);
            assert_eq!(code(outcome), USAGE, "{key} {value}");
            assert!(asked.is_empty());
        }
    }

    #[test]
    fn proxy_check_without_an_address_takes_the_setting() {
        let (_, asked) = drive(&["proxy", "check"], vec![Ok(settings())]);
        assert_eq!(
            asked,
            [
                Request::GetSettings,
                Request::CheckProxy {
                    addr: "127.0.0.1:2080".into()
                }
            ]
        );
        let (outcome, asked) = drive(&["proxy", "check"], vec![Ok(json!({ "socks5": null }))]);
        assert_eq!(code(outcome), USAGE);
        assert_eq!(asked, [Request::GetSettings]);
    }

    #[test]
    fn game_filter_keeps_the_ports_not_given() {
        let flowseal = json!({
            "game_filter": { "mode": "all", "tcp": "1024-65535", "udp": "50000-50100" },
            "ipset": "loaded",
        });
        let (_, asked) = drive(&["game-filter", "tcp", "--tcp", "80"], vec![Ok(flowseal)]);
        assert_eq!(
            asked,
            [
                Request::GetFlowseal,
                Request::SetGameFilter {
                    filter: GameFilter {
                        mode: GameFilterMode::Tcp,
                        tcp: "80".into(),
                        udp: "50000-50100".into(),
                    }
                }
            ]
        );
    }

    #[test]
    fn custom_save_reads_the_strategy_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.json");
        let strategy = CustomStrategy {
            name: "mine".into(),
            cloned_from: "general (ALT11)".into(),
            wf: "--wf-tcp=80,443".into(),
            blocks: vec!["--filter-tcp=443 --dpi-desync=fake".into()],
        };
        let json = serde_json::to_string(&strategy).unwrap();
        std::fs::write(&file, format!("\u{feff}{json}")).unwrap();
        let path = file.to_str().unwrap();
        let (_, asked) = drive(&["custom", "save", path, "--create"], Vec::new());
        assert_eq!(
            asked,
            [Request::SaveCustom {
                strategy,
                create: true
            }]
        );
        std::fs::write(&file, "{\"name\": 1}").unwrap();
        let (outcome, asked) = drive(&["custom", "validate", path], Vec::new());
        assert_eq!(code(outcome), USAGE);
        assert!(asked.is_empty());
    }

    #[test]
    fn zapret2_save_and_validate_send_the_text_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.txt");
        let text = "# mine\n--wf-tcp-out=443\n--lua-desync=fake:blob=fake_default_tls\n";
        std::fs::write(&file, format!("\u{feff}{text}")).unwrap();
        let path = file.to_str().unwrap();
        let (_, asked) = drive(&["zapret2", "save", "mine", path, "--create"], Vec::new());
        assert_eq!(
            asked,
            [Request::SaveZapret2 {
                name: "mine".into(),
                text: text.into(),
                create: true
            }]
        );
        let (_, asked) = drive(&["zapret2", "validate", path], Vec::new());
        assert_eq!(asked, [Request::ValidateZapret2 { text: text.into() }]);
        // PowerShell 5.1 `>` / `Out-File`: UTF-16LE with an FF FE BOM.
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(&file, utf16).unwrap();
        let (_, asked) = drive(&["zapret2", "validate", path], Vec::new());
        assert_eq!(asked, [Request::ValidateZapret2 { text: text.into() }]);
        let missing = dir.path().join("none.txt");
        let (outcome, asked) = drive(
            &["zapret2", "validate", missing.to_str().unwrap()],
            Vec::new(),
        );
        assert_eq!(code(outcome), USAGE);
        assert!(asked.is_empty());
    }

    fn rows() -> Value {
        json!([
            { "name": "general", "custom": false, "active": true, "error": null, "last": null, "flowseal_copy": false },
            { "name": "general (broken)", "custom": false, "active": false, "error": "no winws line", "last": null, "flowseal_copy": false },
            { "name": "general (ALT11)", "custom": false, "active": false, "error": null, "last": null, "flowseal_copy": false },
            { "name": "zapret2:general (ALT11)", "custom": false, "active": false, "error": null, "last": null, "flowseal_copy": true },
        ])
    }

    #[test]
    fn tests_run_without_names_takes_every_strategy_that_parses_except_flowseal_copies() {
        let (outcome, asked) = drive(&["tests", "run"], vec![Ok(rows())]);
        assert_eq!(
            outcome,
            Ok(Done::Json(
                json!({ "started": true, "suite": "standard", "strategies": 2, "repeats": 1 })
            ))
        );
        assert_eq!(
            asked,
            [
                Request::ListStrategies,
                Request::StartTests {
                    request: TestRequest {
                        suite: Suite::Standard,
                        strategies: vec!["general".into(), "general (ALT11)".into()],
                        repeats: 1,
                    }
                }
            ]
        );
    }

    fn result(name: &str, launch_error: Option<&str>) -> StrategyResult {
        StrategyResult {
            name: name.into(),
            args_sha256: String::new(),
            launch_error: launch_error.map(str::to_owned),
            targets: Vec::new(),
            dpi: Vec::new(),
            score: Score {
                ok: 35,
                total: 36,
                ..Score::default()
            },
            passes: Vec::new(),
        }
    }

    fn test_run() -> TestRun {
        TestRun {
            id: "2026-10-11_10-00-00".into(),
            suite: Suite::Dpi,
            flowseal_version: Some("1.10.3".into()),
            zapret2_version: None,
            canceled: false,
            restore_error: None,
            strategies: vec![
                result("general (ALT11)", None),
                result("general", Some("exit 1")),
            ],
        }
    }

    #[test]
    fn tests_run_wait_ends_with_the_ranking() {
        let finished =
            json!({ "testing": false, "live": null, "last": { "run_id": "2026-10-11_10-00-00" } });
        let (outcome, asked) = drive(
            &[
                "tests",
                "run",
                "--dpi",
                "--wait",
                "--repeats",
                "3",
                "general (ALT11)",
                "general",
            ],
            vec![
                Ok(Value::Null),
                Ok(finished),
                Ok(serde_json::to_value(test_run()).unwrap()),
            ],
        );
        assert_eq!(
            asked,
            [
                Request::StartTests {
                    request: TestRequest {
                        suite: Suite::Dpi,
                        strategies: vec!["general (ALT11)".into(), "general".into()],
                        repeats: 3,
                    }
                },
                Request::TestsStatus,
                Request::GetRun {
                    id: "2026-10-11_10-00-00".into()
                },
            ]
        );
        assert_eq!(outcome, Ok(Done::Json(compact(&test_run()))));
    }

    #[test]
    fn the_compact_ranking_keeps_places_scores_and_launch_errors() {
        let compact = compact(&test_run());
        assert_eq!(compact["suite"], "dpi");
        assert_eq!(compact["ranking"][0]["place"], 1);
        assert_eq!(compact["ranking"][0]["name"], "general (ALT11)");
        assert_eq!(compact["ranking"][0]["score"]["ok"], 35);
        assert_eq!(compact["ranking"][1]["launch_error"], "exit 1");
        assert!(compact["ranking"][0].get("targets").is_none());
    }

    #[test]
    fn tests_wait_reports_how_the_run_ended() {
        let idle = |last: Value| json!({ "testing": false, "live": null, "last": last });
        let (outcome, _) = drive(
            &["tests", "wait"],
            vec![Ok(idle(json!({ "error": "curl.exe was not found" })))],
        );
        assert_eq!(outcome, Err(Fail::new(FAILED, "curl.exe was not found")));
        let (outcome, _) = drive(&["tests", "wait"], vec![Ok(idle(Value::Null))]);
        assert_eq!(outcome, Ok(Done::Json(json!({ "last": null }))));
        let going = json!({ "testing": true, "live": null, "last": null });
        let (outcome, asked) = drive(&["tests", "wait", "--timeout", "0"], vec![Ok(going)]);
        assert_eq!(code(outcome), TIMED_OUT);
        assert_eq!(asked, [Request::TestsStatus]);
    }

    #[test]
    fn a_timeout_past_the_end_of_the_clock_means_no_limit() {
        // `Instant + Duration` panics there; only u64::MAX seconds gets that far.
        let huge = u64::MAX.to_string();
        let ended = json!({ "testing": false, "live": null, "last": null });
        let (outcome, asked) = drive(
            &["tests", "wait", "--timeout", &huge],
            vec![Ok(ended.clone())],
        );
        assert_eq!(outcome, Ok(Done::Json(json!({ "last": null }))));
        assert_eq!(asked, [Request::TestsStatus]);
        // The run has started by then.
        let (outcome, asked) = drive(
            &["tests", "run", "--wait", "--timeout", &huge, "general"],
            vec![Ok(Value::Null), Ok(ended)],
        );
        assert_eq!(outcome, Ok(Done::Json(json!({ "last": null }))));
        assert_eq!(
            asked,
            [
                Request::StartTests {
                    request: TestRequest {
                        suite: Suite::Standard,
                        strategies: vec!["general".into()],
                        repeats: 1,
                    }
                },
                Request::TestsStatus
            ]
        );
    }

    #[test]
    fn progress_goes_to_notes_and_never_into_the_json() {
        let live = json!({ "request": { "suite": "Standard", "strategies": ["a", "b"], "repeats": 3 },
            "current": 1, "restoring": false, "pass": 0, "results": [] });
        let going = json!({ "testing": true, "live": live, "last": null });
        let ended = json!({ "testing": false, "live": null, "last": null });
        let (outcome, _, notes) = drive_noted(
            &["tests", "run", "--wait", "a", "b"],
            vec![Ok(Value::Null), Ok(going), Ok(ended)],
        );
        assert_eq!(notes, ["pass 1/3: 2/2 b"]);
        assert_eq!(outcome, Ok(Done::Json(json!({ "last": null }))));
    }

    #[test]
    fn silent_commands_print_ok_but_an_empty_update_check_stays_null() {
        for args in [&["tests", "cancel"][..], &["ipset", "update"], &["restart"]] {
            let (outcome, _) = drive(args, Vec::new());
            assert_eq!(outcome, Ok(Done::Json(json!({ "ok": true }))), "{args:?}");
        }
        let (outcome, _) = drive(&["flowseal", "check-update"], Vec::new());
        assert_eq!(outcome, Ok(Done::Json(Value::Null)));
    }

    #[test]
    fn failures_count_every_bad_check_over_all_passes() {
        use probe::curl::{CheckKind, CheckResult};
        use tester::model::TargetResult;
        let check = |kind, status, stderr: &str| CheckResult {
            kind,
            status,
            http_code: "000".into(),
            time_ms: None,
            stderr: stderr.into(),
        };
        let pass = |checks| Pass {
            launch_error: None,
            targets: vec![TargetResult {
                name: "YouTube".into(),
                url: None,
                extra: false,
                checks,
                ping_ms: None,
            }],
            dpi: Vec::new(),
            score: Score::default(),
        };
        let bad = pass(vec![
            check(CheckKind::Http, CheckStatus::Ok, ""),
            check(
                CheckKind::Tls12,
                CheckStatus::Error,
                "
curl: (28) timeout
more",
            ),
        ]);
        let mut worse = bad.clone();
        worse.targets[0].checks[1].stderr = "curl: (35) reset".into();
        let mut strategy = result("general", None);
        strategy.passes = vec![bad, worse];
        let mut run = test_run();
        run.strategies = vec![strategy, result("clean", None)];
        assert_eq!(
            failures_of(&run),
            json!({ "general": { "YouTube Tls12 Error":
                { "count": 2, "sample_error": "curl: (28) timeout" } } })
        );
    }

    #[test]
    fn progress_names_each_strategy_once() {
        let live = Live {
            request: TestRequest {
                suite: Suite::Standard,
                strategies: vec!["general".into(), "general (ALT2)".into()],
                repeats: 1,
            },
            current: Some(1),
            restoring: false,
            pass: 0,
        };
        let mut shown = None;
        assert_eq!(
            progress(&live, &mut shown).as_deref(),
            Some("2/2 general (ALT2)")
        );
        assert_eq!(progress(&live, &mut shown), None);
    }

    #[test]
    fn progress_names_each_pass_of_one_strategy() {
        let mut live = Live {
            request: TestRequest {
                suite: Suite::Standard,
                strategies: vec!["general".into()],
                repeats: 3,
            },
            current: Some(0),
            restoring: false,
            pass: 0,
        };
        let mut shown = None;
        assert_eq!(
            progress(&live, &mut shown).as_deref(),
            Some("pass 1/3: 1/1 general")
        );
        live.pass = 1;
        assert_eq!(
            progress(&live, &mut shown).as_deref(),
            Some("pass 2/3: 1/1 general")
        );
        assert_eq!(progress(&live, &mut shown), None);
    }

    #[test]
    fn version_answers_without_the_app() {
        let (outcome, _) = drive(
            &["version"],
            vec![Err(Fail::new(UNAVAILABLE, "Zapret Hub is not running"))],
        );
        assert_eq!(
            outcome,
            Ok(Done::Json(json!({
                "cli": env!("CARGO_PKG_VERSION"),
                "protocol": 4,
                "app": null,
            })))
        );
    }
}

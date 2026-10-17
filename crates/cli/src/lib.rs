//! The protocol between `zapret-hub-cli` and the running app (CLI spec 6): one JSON line each way
//! per connection. `cmd` and `args` are the window's IPC commands, so the app runs the same code
//! for both.

pub mod pipe;

use router::rules::{Preset, Rule};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tester::run::Request as TestRequest;
use zapret::checks::CheckAction;
use zapret::custom::CustomStrategy;
use zapret::fakes::FakeKind;
use zapret::game_filter::GameFilter;
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

/// Grows only with an incompatible change; a new command is not one.
pub const PROTOCOL: u32 = 4;

/// A longer request is refused (spec 5).
pub const MAX_REQUEST: usize = 1024 * 1024;

/// The Task Scheduler entry the app's autostart makes; `zapret-hub-cli start` runs it.
pub const AUTOSTART_TASK: &str = "Zapret Hub";

/// The answer to everything while «Управление из командной строки» is off (spec 5).
pub const OFF: &str = "the CLI is off: enable it in Zapret Hub → Settings";

/// One command: the variant in snake_case and its fields are an IPC command of the window and its
/// arguments (`bindings.ts`). `tests_status` is the only one the window lacks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Request {
    AppVersion,
    GetSettings,
    /// The app's `Settings` as JSON: the type lives in the app, which checks it on arrival.
    SaveSettings {
        settings: Value,
    },
    GetOverview,
    CheckProxy {
        addr: String,
    },
    WizardCandidates,
    ListStrategies,
    GetStrategy {
        name: String,
    },
    ApplyStrategy {
        name: String,
    },
    ApplyBest,
    RestartZapret,
    CloneStrategy {
        source: String,
        name: String,
    },
    ValidateCustom {
        strategy: CustomStrategy,
    },
    SaveCustom {
        strategy: CustomStrategy,
        create: bool,
    },
    DeleteCustom {
        name: String,
    },
    Zapret2Info,
    ValidateZapret2 {
        text: String,
    },
    /// `name` without the prefix: `mine` for `zapret2:mine`.
    SaveZapret2 {
        name: String,
        text: String,
        create: bool,
    },
    DeleteZapret2 {
        name: String,
    },
    StartTests {
        request: TestRequest,
    },
    TestsStatus,
    CancelTests,
    ListRuns,
    GetRun {
        id: String,
    },
    RunReport {
        id: String,
    },
    GetFlowseal,
    SetGameFilter {
        filter: GameFilter,
    },
    SetIpsetMode {
        mode: IpsetMode,
    },
    SetCheckUpdates {
        enabled: bool,
    },
    ReplaceFake {
        kind: FakeKind,
        source: String,
    },
    AddListEntry {
        list: UserList,
        entry: String,
    },
    RemoveListEntry {
        list: UserList,
        entry: String,
    },
    UpdateIpset,
    HostsPlan,
    ApplyHosts,
    CheckFlowsealUpdate,
    SystemChecks,
    FixCheck {
        action: CheckAction,
    },
    ClearDiscordCache,
    RemoveService,
    QuickCheck,
    SetRouterEnabled {
        enabled: bool,
    },
    RouterStatus,
    ListRoutes,
    GetRoute {
        name: String,
    },
    SaveRoute {
        rule: Rule,
        create: bool,
    },
    SetRouteEnabled {
        name: String,
        enabled: bool,
    },
    DeleteRoute {
        name: String,
    },
    AddRoutePreset {
        preset: Preset,
    },
    RouteFlows {
        all: bool,
    },
}

impl Request {
    /// The `cmd` it travels as: the journal names it.
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value["cmd"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Serialize)]
struct Outgoing<'a> {
    v: u32,
    #[serde(flatten)]
    request: &'a Request,
}

/// The line the CLI sends, without its `\n`: `{"v":4,"cmd":"apply_strategy","args":{"name":"…"}}`.
pub fn encode(request: &Request) -> String {
    serde_json::to_string(&Outgoing {
        v: PROTOCOL,
        request,
    })
    .expect("a request is plain data")
}

/// `{"ok":true,"data":…}`, `{"ok":false,"error":"…"}`, or `{"ok":false,"refused":true,"error":"…"}`
/// when the app ran nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            refused: false,
            data,
            error: None,
        }
    }

    /// The command ran and failed: the whole chain of causes.
    pub fn error(error: String) -> Self {
        Self {
            ok: false,
            refused: false,
            data: Value::Null,
            error: Some(error),
        }
    }

    pub fn refused(error: String) -> Self {
        Self {
            refused: true,
            ..Self::error(error)
        }
    }
}

/// What the app does with a request line before running anything (spec 5, 6): refused while the
/// CLI is off, over 1 MiB, unparsable (an unknown command included) or of another protocol version.
pub fn gate(line: &[u8], enabled: bool) -> Result<Request, Response> {
    if !enabled {
        return Err(Response::refused(OFF.to_owned()));
    }
    if line.len() > MAX_REQUEST {
        return Err(Response::refused(
            "the request is longer than 1 MiB".to_owned(),
        ));
    }
    #[derive(Deserialize)]
    struct Version {
        v: u32,
    }
    // serde quotes the request (an unknown `cmd` as sent): one line, so the journal's
    // `cli: refused: …` cannot carry a forged line of its own.
    let unparsable = |err: serde_json::Error| {
        Response::refused(
            format!("the request does not parse: {err}").replace(char::is_control, " "),
        )
    };
    // The version first: a newer CLI's unknown command must read as a version mismatch.
    let Version { v } = serde_json::from_slice(line).map_err(unparsable)?;
    if v != PROTOCOL {
        return Err(Response::refused(format!(
            "protocol v{v} is not supported: update zapret-hub-cli.exe and Zapret Hub together"
        )));
    }
    serde_json::from_slice(line).map_err(unparsable)
}

/// `tests_status` (spec 7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestsStatus {
    pub testing: bool,
    pub live: Option<Live>,
    pub last: Option<Outcome>,
}

/// What the CLI reads of `test_live`'s run in progress; the app sends all of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Live {
    pub request: TestRequest,
    /// The strategy under test, from 0; `None` before the first and while restoring.
    pub current: Option<u32>,
    pub restoring: bool,
    /// The pass under way, from 0, of `request.repeats`; an app from before repeats sends none.
    #[serde(default)]
    pub pass: u32,
}

/// How the newest run since the app started ended: saved as `run_id`, or failed before a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Run { run_id: String },
    Failed { error: String },
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tester::model::Suite;
    use zapret::game_filter::GameFilterMode;

    use super::*;

    fn strategy() -> CustomStrategy {
        CustomStrategy {
            name: "mine".into(),
            cloned_from: "general (ALT11)".into(),
            wf: "--wf-tcp=80,443".into(),
            blocks: vec!["--filter-tcp=443 --dpi-desync=fake".into()],
        }
    }

    /// One of each request.
    fn samples() -> Vec<Request> {
        vec![
            Request::AppVersion,
            Request::GetSettings,
            Request::SaveSettings {
                settings: json!({ "zapret_dir": null, "cli_enabled": true }),
            },
            Request::GetOverview,
            Request::CheckProxy {
                addr: "127.0.0.1:2080".into(),
            },
            Request::WizardCandidates,
            Request::ListStrategies,
            Request::GetStrategy {
                name: "general (ALT11)".into(),
            },
            Request::ApplyStrategy {
                name: "general (ALT11)".into(),
            },
            Request::ApplyBest,
            Request::RestartZapret,
            Request::CloneStrategy {
                source: "general (ALT11)".into(),
                name: "mine".into(),
            },
            Request::ValidateCustom {
                strategy: strategy(),
            },
            Request::SaveCustom {
                strategy: strategy(),
                create: true,
            },
            Request::DeleteCustom {
                name: "mine".into(),
            },
            Request::Zapret2Info,
            Request::ValidateZapret2 {
                text: "--wf-tcp-out=443".into(),
            },
            Request::SaveZapret2 {
                name: "mine".into(),
                text: "--wf-tcp-out=443".into(),
                create: true,
            },
            Request::DeleteZapret2 {
                name: "mine".into(),
            },
            Request::StartTests {
                request: TestRequest {
                    suite: Suite::Standard,
                    strategies: vec!["general".into()],
                    repeats: 1,
                },
            },
            Request::TestsStatus,
            Request::CancelTests,
            Request::ListRuns,
            Request::GetRun {
                id: "2026-10-10_05-03-34".into(),
            },
            Request::RunReport {
                id: "2026-10-10_05-03-34".into(),
            },
            Request::GetFlowseal,
            Request::SetGameFilter {
                filter: GameFilter {
                    mode: GameFilterMode::All,
                    tcp: "1024-65535".into(),
                    udp: "1024-65535".into(),
                },
            },
            Request::SetIpsetMode {
                mode: IpsetMode::Any,
            },
            Request::SetCheckUpdates { enabled: false },
            Request::ReplaceFake {
                kind: FakeKind::Discord,
                source: "quic_initial_www_google_com".into(),
            },
            Request::AddListEntry {
                list: UserList::General,
                entry: "example.com".into(),
            },
            Request::RemoveListEntry {
                list: UserList::IpsetExclude,
                entry: "203.0.113.0/24".into(),
            },
            Request::UpdateIpset,
            Request::HostsPlan,
            Request::ApplyHosts,
            Request::CheckFlowsealUpdate,
            Request::SystemChecks,
            Request::FixCheck {
                action: CheckAction::RemoveWinDivert,
            },
            Request::ClearDiscordCache,
            Request::RemoveService,
            Request::QuickCheck,
            Request::SetRouterEnabled { enabled: true },
            Request::RouterStatus,
            Request::ListRoutes,
            Request::GetRoute {
                name: "Discord голос".into(),
            },
            Request::SaveRoute {
                rule: router::rules::preset(Preset::DiscordVoice),
                create: true,
            },
            Request::SetRouteEnabled {
                name: "Discord голос".into(),
                enabled: false,
            },
            Request::DeleteRoute {
                name: "Discord голос".into(),
            },
            Request::AddRoutePreset {
                preset: Preset::DiscordVoice,
            },
            Request::RouteFlows { all: true },
        ]
    }

    /// Window commands the CLI leaves out (spec 6).
    const WINDOW_ONLY: [&str; 16] = [
        "detect_folders",
        "check_folder",
        "check_zapret2_folder",
        "pick_folder",
        // Downloads make a new engine folder: window only, like the folders (cli.md).
        "default_download_parent",
        "download_engine",
        "cancel_download",
        "open_url",
        "open_folder",
        "test_live",
        "take_notices",
        // The circular builder and merge shape the editor's text (zapret2 spec 5.4).
        "zapret2_profiles",
        "zapret2_circular",
        "zapret2_merge",
        // The editor checks a rule as it is typed and fills its IP list from a pasted text.
        "validate_route",
        "import_route_lines",
    ];

    /// `("cmd", { a, b })` → the argument names in `bindings.ts`, sorted.
    fn ipc_arguments(bindings: &str, cmd: &str) -> Option<Vec<String>> {
        let call = format!("(\"{cmd}\"");
        let rest = &bindings[bindings.find(&call)? + call.len()..];
        let rest = &rest[..rest.find(')')?];
        let mut args: Vec<String> = rest
            .trim_start_matches(',')
            .trim()
            .trim_matches(['{', '}'])
            .split(',')
            .map(|arg| arg.trim().to_owned())
            .filter(|arg| !arg.is_empty())
            .collect();
        args.sort();
        Some(args)
    }

    #[test]
    fn every_request_is_a_window_command_with_its_arguments() {
        let bindings = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/src/lib/ipc/bindings.ts"
        ))
        .unwrap();
        let mut names = Vec::new();
        for request in samples() {
            let value = serde_json::to_value(&request).unwrap();
            let cmd = value["cmd"].as_str().unwrap().to_owned();
            assert_eq!(request.name(), cmd);
            if cmd != "tests_status" {
                let mut args: Vec<String> = value
                    .get("args")
                    .and_then(Value::as_object)
                    .map(|args| args.keys().cloned().collect())
                    .unwrap_or_default();
                args.sort();
                assert_eq!(
                    ipc_arguments(&bindings, &cmd),
                    Some(args),
                    "{cmd} in bindings.ts"
                );
            }
            names.push(cmd);
        }
        // And the other way: a new window command needs a request or a place in WINDOW_ONLY.
        // `__TAURI_INVOKE("cmd"` or `__TAURI_INVOKE<T>("cmd"`; the import line does not count.
        for chunk in bindings
            .split("__TAURI_INVOKE")
            .filter(|chunk| chunk.starts_with(['(', '<']))
        {
            let start = chunk.find("(\"").unwrap() + 2;
            let cmd = &chunk[start..start + chunk[start..].find('"').unwrap()];
            assert!(
                names.iter().any(|name| name == cmd) || WINDOW_ONLY.contains(&cmd),
                "{cmd} has no request"
            );
        }
    }

    #[test]
    fn a_request_line_carries_the_version_first() {
        assert_eq!(
            encode(&Request::ApplyStrategy {
                name: "general (ALT11)".into()
            }),
            r#"{"v":4,"cmd":"apply_strategy","args":{"name":"general (ALT11)"}}"#
        );
        assert_eq!(encode(&Request::ApplyBest), r#"{"v":4,"cmd":"apply_best"}"#);
    }

    #[test]
    fn every_request_comes_through_the_gate_as_it_left() {
        for request in samples() {
            assert_eq!(gate(encode(&request).as_bytes(), true), Ok(request));
        }
    }

    fn refusal(line: &[u8], enabled: bool) -> String {
        let response = gate(line, enabled).unwrap_err();
        assert!(!response.ok && response.refused, "{response:?}");
        response.error.unwrap()
    }

    #[test]
    fn the_gate_refuses_before_anything_runs() {
        let apply = encode(&Request::ApplyBest);
        assert_eq!(refusal(apply.as_bytes(), false), OFF);
        assert_eq!(refusal(b"not json", false), OFF, "off comes first");
        let turn_on = encode(&Request::SaveSettings {
            settings: json!({ "cli_enabled": true }),
        });
        assert_eq!(
            refusal(turn_on.as_bytes(), false),
            OFF,
            "the CLI cannot turn itself on"
        );
        assert!(refusal(b"not json", true).starts_with("the request does not parse"));
        assert!(
            refusal(br#"{"v":4,"cmd":"format_disk"}"#, true).contains("format_disk"),
            "an unknown command is named"
        );
        assert!(refusal(br#"{"cmd":"apply_best"}"#, true).contains("`v`"));
        assert_eq!(
            refusal(br#"{"v":3,"cmd":"format_disk"}"#, true),
            "protocol v3 is not supported: update zapret-hub-cli.exe and Zapret Hub together"
        );
    }

    #[test]
    fn a_refusal_is_one_line_whatever_the_request_holds() {
        // serde names an unknown `cmd` as sent; the app journals the reason as `cli: refused: …`.
        let reason = refusal(br#"{"v":4,"cmd":"x\ncli: apply_best: ok"}"#, true);
        assert!(!reason.contains(char::is_control), "{reason:?}");
        assert!(reason.contains("x cli: apply_best: ok"), "{reason}");
    }

    #[test]
    fn the_gate_takes_1_mib_and_not_a_byte_more() {
        let padded = |len: usize| {
            let mut line = encode(&Request::ApplyBest).into_bytes();
            line.resize(len, b' ');
            line
        };
        assert_eq!(gate(&padded(MAX_REQUEST), true), Ok(Request::ApplyBest));
        assert_eq!(
            refusal(&padded(MAX_REQUEST + 1), true),
            "the request is longer than 1 MiB"
        );
    }

    #[test]
    fn answers_read_back_as_they_were_sent() {
        for (response, line) in [
            (
                Response::ok(json!({ "app": "0.2.0" })),
                r#"{"ok":true,"data":{"app":"0.2.0"}}"#,
            ),
            (Response::ok(Value::Null), r#"{"ok":true}"#),
            (
                Response::error("the zapret folder is not set".into()),
                r#"{"ok":false,"error":"the zapret folder is not set"}"#,
            ),
            (
                Response::refused(OFF.into()),
                r#"{"ok":false,"refused":true,"error":"the CLI is off: enable it in Zapret Hub → Settings"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&response).unwrap(), line);
            assert_eq!(serde_json::from_str::<Response>(line).unwrap(), response);
        }
    }

    #[test]
    fn a_run_ends_saved_or_failed() {
        let saved: Outcome = serde_json::from_str(r#"{"run_id":"2026-10-10_05-03-34"}"#).unwrap();
        assert_eq!(
            saved,
            Outcome::Run {
                run_id: "2026-10-10_05-03-34".into()
            }
        );
        let failed: Outcome =
            serde_json::from_str(r#"{"error":"curl.exe was not found"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&failed).unwrap(),
            r#"{"error":"curl.exe was not found"}"#
        );
    }
}

//! The machine-readable part of a report (agent proposal, docs/superpowers/specs/
//! 2026-10-24-zapret-hub-diag-agent-proposal.md): stable English codes next to the Russian text,
//! so an agent branches on codes and retells the text. Every type has `Default`: old reports load.

use serde::{Deserialize, Serialize};

/// The run's answer in one code (proposal 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VerdictCode {
    /// Nothing seen: the program opened no connections while watched.
    #[default]
    NoFlows,
    /// The program was not in the state the profile needs (Discord not running, not in voice).
    NotReproducible,
    /// Everything seen worked, and everything that matters was seen.
    Clean,
    /// Nothing failed, but part of the traffic or the client state was not seen.
    Inconclusive,
    /// Culprits, and a path that fixes every one of them.
    CulpritsFixed,
    /// Culprits, some fixed.
    CulpritsPartial,
    /// Culprits, none fixed.
    CulpritsUnfixed,
    /// The problem is on the other side (a peer, the service).
    PeerSide,
    /// A whitelist network (Р5).
    Whitelist,
    /// A live-only profile in auto mode.
    LiveOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Confidence {
    pub level: Level,
    /// Codes, e.g. `stopped`, `budget_hit`, `source_missing:discord_log`, `no_voice_flow`.
    pub why: Vec<String>,
}

/// One fact: a stable code, its arguments, and the text for a person.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Fact {
    pub code: String,
    pub args: Vec<String>,
    pub text: String,
}

/// Where Discord's voice got to, from its own logs (proposal 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStage {
    /// No voice session in the logs.
    #[default]
    None,
    /// Joined, but no voice server came (gateway side).
    NoEndpoint,
    /// The voice WebSocket did not connect.
    WsFailed,
    /// The WebSocket connected, UDP did not.
    UdpFailed,
    /// Connected; DAVE waits for a second participant (normal when alone).
    DaveWaitingPeer,
    /// Connected; DAVE stalled with others present.
    DaveStalled,
    Connected,
}

/// One media connection (voice or stream) as the client logged it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct MediaState {
    pub stage: VoiceStage,
    /// `ip:port` of the media server.
    pub server: Option<String>,
    pub peers: u32,
    /// From the client's pings: lost / sent over the session.
    pub loss_pct: Option<u32>,
    pub rtt_ms: Option<u32>,
    pub reconnects: u32,
    /// Packets went out and none came in.
    pub one_way: bool,
}

/// What the program's own logs say (proposal 3); empty for programs without known logs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ClientState {
    /// `running`, `not_running`, `unknown`.
    pub app: String,
    pub voice: Option<MediaState>,
    pub stream: Option<MediaState>,
    /// The gateway's last state, e.g. `connected`, `connect_failed`.
    pub gateway: Option<String>,
    /// Facts from other logs: the Discord updater, Steam's connection and content logs.
    pub facts: Vec<Fact>,
}

/// The observation in counts (proposal 2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Observed {
    pub secs: u32,
    pub flows: u32,
    pub tcp: u32,
    pub udp: u32,
    pub via_router: u32,
    pub via_system_proxy: u32,
}

/// Whether the current strategy and lists cover a culprit (proposal 1, 5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Coverage {
    /// The culprit group's key.
    pub key: String,
    /// `zapret`, `zapret2`, `router:<rule>`, `system_proxy`, `direct`.
    pub route: String,
    pub in_hostlist: Option<bool>,
    pub in_ipset: Option<bool>,
    pub game_filter_covers: Option<bool>,
    /// Age of the list that should cover it, in days.
    pub list_age_days: Option<u32>,
}

/// The setup the run happened in (proposal 1): engine, routing, proxy, tests, the hub's errors.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Context {
    pub strategy: Option<String>,
    pub engine_running: bool,
    pub routing: String,
    pub proxy: Option<String>,
    pub proxy_alive: Option<bool>,
    pub proxy_udp: Option<bool>,
    pub egress_ip_direct: Option<String>,
    pub egress_ip_proxy: Option<String>,
    /// Place of the current strategy in the newest standard test run, and that run's best.
    pub tests_rank: Option<u32>,
    pub tests_best: Option<String>,
    /// The hub's own errors of the last day, newest first.
    pub hub_errors: Vec<String>,
}

/// Which of the client's connections a culprit group carries (proposal 3, Go Live).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct CulpritKind {
    /// The culprit group's key.
    pub key: String,
    /// `voice`, `stream`, `other`.
    pub kind: String,
}

/// The agent's part of a report; `Summary.agent`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AgentView {
    pub verdict: VerdictCode,
    pub confidence: Confidence,
    pub duration_s: u32,
    pub client: ClientState,
    pub observed: Observed,
    pub coverage: Vec<Coverage>,
    pub context: Context,
    /// Short, checkable facts behind the verdict.
    pub evidence: Vec<Fact>,
    /// What was not checked and why.
    pub not_checked: Vec<Fact>,
    /// Differences from the previous report of the same profile.
    pub changes: Vec<Fact>,
    /// Exact CLI commands or `ask_user: …` lines, in order.
    pub next_steps: Vec<String>,
    /// Culprit groups by the client connection they belong to; Discord only.
    pub culprit_kinds: Vec<CulpritKind>,
}

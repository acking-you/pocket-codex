//! `_pcx/*` extension methods (T3) and the result types shared by the hub
//! and the controller-side bridge engine.
//!
//! These live in core because the bridge compiles for mobile too, while the
//! hub (host-svc) is desktop-only.

use serde::{Deserialize, Serialize};

use super::{
    transcript::{HubItem, TurnInfo},
    types::{
        AvailableCommand, ConfigOption, ContentBlock, Implementation, SessionModeState, UsageUpdate,
    },
};

/// Version of the `_meta.pcx` hub metadata.
pub const HUB_META_VERSION: u32 = 1;

/// Controller → hub methods.
pub mod methods {
    /// `_pcx/session/attach`
    pub const SESSION_ATTACH: &str = "_pcx/session/attach";
    /// `_pcx/session/detach`
    pub const SESSION_DETACH: &str = "_pcx/session/detach";
    /// `_pcx/session/window`
    pub const SESSION_WINDOW: &str = "_pcx/session/window";
    /// `_pcx/session/submit`
    pub const SESSION_SUBMIT: &str = "_pcx/session/submit";
    /// `_pcx/session/reload`
    pub const SESSION_RELOAD: &str = "_pcx/session/reload";
    /// `_pcx/sessions/running`
    pub const SESSIONS_RUNNING: &str = "_pcx/sessions/running";
    /// `_pcx/auth/authenticate`
    pub const AUTH_AUTHENTICATE: &str = "_pcx/auth/authenticate";
    /// `_pcx/hub/defaults`
    pub const HUB_DEFAULTS: &str = "_pcx/hub/defaults";
}

/// Hub → controller notifications.
pub mod notifications {
    /// `_pcx/session/loaded`
    pub const SESSION_LOADED: &str = "_pcx/session/loaded";
    /// `_pcx/session/loadFailed`
    pub const SESSION_LOAD_FAILED: &str = "_pcx/session/loadFailed";
    /// `_pcx/turn/started`
    pub const TURN_STARTED: &str = "_pcx/turn/started";
    /// `_pcx/turn/completed`
    pub const TURN_COMPLETED: &str = "_pcx/turn/completed";
    /// `_pcx/request/resolved`
    pub const REQUEST_RESOLVED: &str = "_pcx/request/resolved";
    /// `_pcx/session/state`
    pub const SESSION_STATE: &str = "_pcx/session/state";
    /// `_pcx/session/generation`
    pub const SESSION_GENERATION: &str = "_pcx/session/generation";
    /// `_pcx/queue/failed`
    pub const QUEUE_FAILED: &str = "_pcx/queue/failed";
    /// `_pcx/sessions/changed`
    pub const SESSIONS_CHANGED: &str = "_pcx/sessions/changed";
    /// `_pcx/hub/state`
    pub const HUB_STATE: &str = "_pcx/hub/state";
    /// Injected by the hub's agent peer when a line exceeds its size limit;
    /// never sent over the wire.
    pub const INTERNAL_OVERSIZED: &str = "_pcx/internal/oversized";
}

/// Stop reasons the hub itself produces.
pub mod stop_reason {
    /// The agent process exited during the turn.
    pub const AGENT_EXITED: &str = "_pcx_agent_exited";
    /// `session/prompt` failed.
    pub const ERROR: &str = "_pcx_error";
}

/// Capabilities negotiated by the hub (`_meta.pcx.caps`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcxCaps {
    /// `session/list`.
    pub list: bool,
    /// `session/load`.
    pub load: bool,
    /// `session/resume`.
    pub resume: bool,
    /// `session/close`.
    pub close: bool,
    /// Image prompt blocks.
    pub image: bool,
    /// Embedded resource prompt blocks.
    pub embedded_context: bool,
    /// Session config options seen.
    pub config_options: bool,
    /// Session modes seen.
    pub modes: bool,
    /// Slash commands seen.
    pub commands: bool,
    /// The hub queues submissions (always true).
    pub queue: bool,
    /// Mid-turn steering (always false).
    pub steer: bool,
    /// URL elicitation (always true).
    pub url_elicitation: bool,
}

/// One queued submission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedInfo {
    /// Hub submission id.
    pub submission_id: String,
    /// First characters of the prompt text.
    pub text_preview: String,
}

/// `_pcx/session/attach` and `_pcx/session/reload` result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    /// Session id.
    pub session_id: String,
    /// Working directory.
    pub cwd: String,
    /// Title.
    pub title: Option<String>,
    /// Last update (RFC 3339).
    pub updated_at: Option<String>,
    /// True while the hub is still loading the session; `items` is empty and
    /// `_pcx/session/loaded` (or `_pcx/session/loadFailed`) follows.
    pub loading: bool,
    /// Transcript generation.
    pub generation: String,
    /// Sequence number of the last session notification already reflected in
    /// this snapshot (see T14 `seq`).
    pub seq: u64,
    /// Tail window, oldest first.
    pub items: Vec<HubItem>,
    /// Older items exist in the transcript.
    pub has_older: bool,
    /// Older history exists but is not available.
    pub older_unavailable: bool,
    /// At most 2000, oldest first.
    pub turns: Vec<TurnInfo>,
    /// Whole turns no longer available.
    pub dropped_turns: u32,
    /// A turn is running.
    pub running: bool,
    /// The running turn.
    pub active_turn: Option<u32>,
    /// Queued submissions.
    pub queue: Vec<QueuedInfo>,
    /// Current config options.
    pub config_options: Vec<ConfigOption>,
    /// Current modes.
    pub modes: Option<SessionModeState>,
    /// Available slash commands.
    pub commands: Vec<AvailableCommand>,
    /// Last usage update.
    pub usage: Option<UsageUpdate>,
}

/// `_pcx/session/window` result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowResult {
    /// Transcript generation.
    pub generation: String,
    /// Items, oldest first.
    pub items: Vec<HubItem>,
    /// Older items exist.
    pub has_older: bool,
    /// More items of the requested turn exist.
    pub has_more: bool,
    /// Older history exists but is not available.
    pub older_unavailable: bool,
}

/// `_pcx/session/submit` result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    /// Hub submission id.
    pub submission_id: String,
    /// The prompt waits in the queue.
    pub queued: bool,
    /// Queue position (0 when started immediately).
    pub position: u32,
    /// Turn number when started immediately.
    pub turn: Option<u32>,
}

/// One entry of `_pcx/sessions/running`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningSession {
    /// Session id.
    pub session_id: String,
    /// A turn is running.
    pub running: bool,
    /// Pending requests.
    pub pending: u32,
    /// Queued submissions.
    pub queue: u32,
}

/// `_pcx/sessions/running` result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningResult {
    /// Sessions with activity.
    pub sessions: Vec<RunningSession>,
}

/// `_pcx/hub/defaults` result: the options a new session starts from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubDefaultsResult {
    /// Empty when the agent reports no config options.
    pub config_options: Vec<ConfigOption>,
}

/// Agent process state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProcessState {
    /// Not running.
    Stopped,
    /// Spawning and initializing.
    Starting,
    /// Accepting session operations.
    Ready,
    /// Waiting to restart after a crash.
    Restarting {
        /// Restart attempt.
        attempt: u32,
        /// Delay before the attempt.
        retry_in_ms: u64,
    },
    /// Gave up; only a manual restart helps.
    Failed {
        /// What went wrong.
        message: String,
        /// Last part of the agent's stderr.
        stderr_tail: String,
    },
}

/// Authentication status values.
pub mod auth_status {
    /// Not yet known.
    pub const UNKNOWN: &str = "unknown";
    /// Authenticated (or not required).
    pub const OK: &str = "ok";
    /// The agent requires authentication.
    pub const REQUIRED: &str = "required";
    /// An `authenticate` call is running.
    pub const IN_PROGRESS: &str = "inProgress";
}

/// Authentication method kinds.
pub mod auth_kind {
    /// Handled by the agent itself (`authenticate`).
    pub const AGENT: &str = "agent";
    /// Runs a login command in a terminal on the host.
    pub const TERMINAL: &str = "terminal";
    /// D20 model gateway.
    pub const GATEWAY: &str = "gateway";
}

/// Authentication state of an agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    /// `unknown` | `ok` | `required` | `inProgress`
    pub status: String,
    /// Offered methods.
    pub methods: Vec<AuthMethodInfo>,
    /// Human-readable detail.
    pub message: Option<String>,
}

/// One authentication method, classified by the hub.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethodInfo {
    /// Method id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: String,
    /// `agent` | `terminal` | `gateway`
    pub kind: String,
    /// Whether a remote controller may start it.
    pub remote: bool,
    /// False when a legacy terminal method cannot be reproduced safely.
    pub available: bool,
    /// `_meta.gateway.protocol` of a gateway method (e.g. `anthropic`,
    /// `openai`), if given.
    pub gateway_protocol: Option<String>,
    /// For gateway methods: whether the host has a gateway configured for this
    /// method.
    pub gateway_configured: bool,
}

/// Which agent a hub runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    /// Catalog or custom agent id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Installed version.
    pub version: Option<String>,
    /// The catalog pins this version.
    pub pinned: bool,
    /// `agentInfo` from `initialize`.
    pub info: Option<Implementation>,
}

/// `initialize` result `_meta.pcx` and the `_pcx/hub/state` notification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubMeta {
    /// [`HUB_META_VERSION`].
    pub version: u32,
    /// The agent.
    pub agent: AgentIdentity,
    /// Negotiated capabilities.
    pub caps: PcxCaps,
    /// Authentication state.
    pub auth: AuthState,
    /// Process state.
    pub process: ProcessState,
    /// Config options of the most recent session response.
    pub default_config_options: Vec<ConfigOption>,
}

/// Install / hosting status of one agent on a host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// Agent id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: String,
    /// `catalog` | `custom`
    pub source: String,
    /// Version pinned by the catalog.
    pub pinned_version: Option<String>,
    /// Installed version.
    pub installed_version: Option<String>,
    /// not_installed | installing | installed | failed | unsupported_platform |
    /// engine_missing | engine_incompatible
    pub state: String,
    /// Detail for the state.
    pub detail: Option<String>,
    /// Running install job.
    pub job_id: Option<String>,
    /// Newer version seen in the ACP registry (unverified).
    pub registry_version: Option<String>,
    /// Instances hosting this agent.
    pub hosted_names: Vec<String>,
    /// Approximate install size.
    pub approx_size_mb: u32,
    /// Needs the private Node runtime.
    pub needs_node: bool,
    /// Remote controllers may install it.
    pub remote_install_allowed: bool,
}

/// Progress of an install or host job.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    /// Job id.
    pub id: String,
    /// `install` | `host`
    pub kind: String,
    /// Agent id.
    pub agent_id: String,
    /// Version being installed.
    pub version: String,
    /// install: queued | downloading | verifying | extracting | installing |
    /// validating | done | failed; host: queued | starting | done | failed
    pub state: String,
    /// Bytes downloaded.
    pub bytes: u64,
    /// Total bytes, when known.
    pub total: Option<u64>,
    /// Detail.
    pub message: Option<String>,
    /// `acp.<code>` on failure.
    pub error_code: Option<String>,
    /// Set when a `host` job is done.
    pub service_key: Option<String>,
}

/// Host ACP settings as the bridge facade sees them (`agents.toml`, §5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSettingsView {
    /// Remote controllers may install and host agents (D14).
    pub remote_management: bool,
    /// npm registry mirror (https).
    pub npm_registry: Option<String>,
    /// Engine override of the catalog agent with `engine_override` (D7).
    pub claude_engine_path: Option<String>,
    /// Engine path of the catalog agent with `external_engine` (D6).
    pub codex_binary: Option<String>,
    /// (agent id, executable) overrides of archive agents.
    pub binary_overrides: Vec<(String, String)>,
    /// (data family, `auto` | `shared` | `isolated`) (D19).
    pub opencode_data: Vec<(String, String)>,
    /// D20 gateways.
    pub gateways: Vec<GatewayView>,
    /// D21 catalog switches.
    pub flags: Vec<AgentFlagView>,
}

/// One catalog `conditional_args` switch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFlagView {
    /// Agent id.
    pub agent_id: String,
    /// Setting key under `[agents.<id>]`.
    pub setting: String,
    /// l10n key of the switch label.
    pub label_key: String,
    /// l10n key of the confirm dialog when turning it on.
    pub confirm_key: Option<String>,
    /// Current value.
    pub value: bool,
}

/// D20 gateway of one agent. Reading returns `token: None` plus `has_token`;
/// writing with `token: None` keeps the stored token, `Some("")` deletes it;
/// `clear` removes the whole entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayView {
    /// Agent id.
    pub agent_id: String,
    /// Gateway method; `None` = the first gateway method.
    pub method_id: Option<String>,
    /// Gateway base URL.
    pub base_url: String,
    /// Write-only token.
    pub token: Option<String>,
    /// A token is stored.
    pub has_token: bool,
    /// Provider name (codex-acp).
    pub provider_name: Option<String>,
    /// Extra headers.
    pub extra_headers: Vec<(String, String)>,
    /// Remove this gateway.
    pub clear: bool,
}

/// A user-defined ACP agent (D15).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgentDef {
    /// `^[a-z][a-z0-9-]{0,63}$`, not a catalog id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Absolute path of the executable.
    pub command: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Environment (stored in plain text).
    pub env: Vec<(String, String)>,
}

/// `_pcx/session/attach` params.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachParams {
    /// Session id.
    pub session_id: String,
    /// Working directory, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Tail size (default 20, max 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<u32>,
}

/// Params naming only a session (`detach`, `reload`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionParams {
    /// Session id.
    pub session_id: String,
}

/// `_pcx/session/window` params.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowParams {
    /// Session id.
    pub session_id: String,
    /// Expected generation.
    pub generation: String,
    /// Items before this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Items of this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    /// With `turn`: items after this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Page size (default 60, max 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `_pcx/session/submit` params.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitParams {
    /// Session id.
    pub session_id: String,
    /// Prompt content.
    pub prompt: Vec<ContentBlock>,
    /// Idempotency key chosen by the controller.
    pub client_submission_id: String,
}

/// `_pcx/auth/authenticate` params.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    /// Method id.
    pub method_id: String,
}

/// `_meta.pcx` the hub adds to every forwarded `session/update` (T14).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeta {
    /// Session notification sequence number.
    pub seq: u64,
    /// Authoritative item id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// The item is new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<bool>,
    /// Turn of the item (current turn otherwise).
    pub turn: u32,
    /// Transcript generation.
    pub generation: String,
    /// Folded item for `tool_call`, `tool_call_update` and `plan`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<HubItem>,
}

/// `_meta.pcx` of a hub → controller request (`requestId` is the hub's
/// pending id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMeta {
    /// Hub pending id (`r{n}`).
    pub request_id: String,
}

/// `_meta.pcx` of each listed session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListMeta {
    /// A turn is running.
    pub running: bool,
    /// Pending requests.
    pub pending: u32,
    /// Queued submissions.
    pub queue: u32,
}

/// `_pcx/session/loaded` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadedParams {
    /// Session id.
    pub session_id: String,
    /// Sequence number.
    pub seq: u64,
}

/// `_pcx/session/loadFailed` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadFailedParams {
    /// Session id.
    pub session_id: String,
    /// Error message (with `[acp.<code>]` prefix).
    pub error: String,
    /// Sequence number.
    pub seq: u64,
}

/// `_pcx/turn/started` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartedParams {
    /// Session id.
    pub session_id: String,
    /// Turn number.
    pub turn: u32,
    /// Hub submission id.
    pub submission_id: String,
    /// Id of the user item.
    pub user_item_id: String,
    /// Start time.
    pub started_at_ms: i64,
    /// Sequence number.
    pub seq: u64,
}

/// `_pcx/turn/completed` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletedParams {
    /// Session id.
    pub session_id: String,
    /// Turn number.
    pub turn: u32,
    /// ACP stop reason or a hub one ([`stop_reason`]).
    pub stop_reason: String,
    /// Error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// End time.
    pub completed_at_ms: i64,
    /// Duration.
    pub duration_ms: i64,
    /// Sequence number.
    pub seq: u64,
}

/// `_pcx/request/resolved` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestResolvedParams {
    /// Session id (`None` for hub-level requests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Hub pending id.
    pub request_id: String,
    /// Sequence number (session-level requests only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

/// `_pcx/session/state` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateParams {
    /// Session id.
    pub session_id: String,
    /// A turn is running.
    pub running: bool,
    /// Queued submissions.
    pub queue: u32,
    /// Pending requests.
    pub pending: u32,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Last update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Sequence number.
    pub seq: u64,
}

/// `_pcx/session/generation` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGenerationParams {
    /// Session id.
    pub session_id: String,
    /// New generation.
    pub generation: String,
    /// Sequence number.
    pub seq: u64,
}

/// A prompt that could not be sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedPrompt {
    /// Hub submission id.
    pub submission_id: String,
    /// Prompt text, so the user can resend it.
    pub text: String,
}

/// `_pcx/queue/failed` params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueFailedParams {
    /// Session id.
    pub session_id: String,
    /// `cancelled` | `agent_exited` | `error`
    pub reason: String,
    /// Dropped prompts.
    pub prompts: Vec<FailedPrompt>,
    /// Sequence number.
    pub seq: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn process_state_uses_state_tag_and_camel_case_fields() {
        assert_eq!(
            serde_json::to_value(ProcessState::Ready).expect("ser"),
            json!({"state": "ready"})
        );
        assert_eq!(
            serde_json::to_value(ProcessState::Restarting {
                attempt: 2,
                retry_in_ms: 2000
            })
            .expect("ser"),
            json!({"state": "restarting", "attempt": 2, "retryInMs": 2000})
        );
        let failed: ProcessState =
            serde_json::from_value(json!({"state": "failed", "message": "m", "stderrTail": "t"}))
                .expect("de");
        assert_eq!(failed, ProcessState::Failed {
            message: "m".into(),
            stderr_tail: "t".into()
        });
    }
}

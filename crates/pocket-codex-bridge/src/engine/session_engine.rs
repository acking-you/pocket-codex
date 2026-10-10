//! The protocol boundary behind the shared session API.
//!
//! A service key's kind selects the protocol — never the provider name:
//!
//! | kind       | protocol                    | engine                 |
//! |------------|-----------------------------|------------------------|
//! | `app`      | Codex app-server (native)   | [`CodexEngine`]        |
//! | `acp`      | Pocket-Codex ACP gateway    | [`AcpEngine`]          |
//!
//! Each `app_*` bridge call goes through [`engine`] once instead of testing
//! the provider itself. Operations a protocol does not have fail with
//! [`unsupported`] before any side effect; callers gate them on
//! [`Capabilities`]. Codex-only calls outside this trait (realtime voice,
//! rate limits, takeover, native thread metadata) use [`require_native`].

use anyhow::{anyhow, Result};
use pocket_codex_account_proto::NamespacedServiceId;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use tokio::sync::broadcast;

use super::{
    acp,
    app_session::{
        self, AppEvent, ModelInfo, OlderPage, ThreadHistory, ThreadItem, ThreadMeta,
        ThreadRuntimeConfig, TurnItemsPage,
    },
    transport,
};

/// Wire protocol of a session service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Native Codex app-server JSON-RPC.
    CodexAppServer,
    /// The Pocket-Codex gateway to a host-owned ACP agent.
    Acp,
}

impl Protocol {
    /// Stable identifier shown to the UI.
    pub fn id(self) -> &'static str {
        match self {
            Self::CodexAppServer => "codex-app-server",
            Self::Acp => "acp",
        }
    }
}

/// The logical service id of a self-host (`pcx:`) or account (`pcxu:`) key.
pub fn service_id_of(service_key: &str) -> Option<ServiceId> {
    ServiceId::parse_key(service_key)
        .or_else(|| NamespacedServiceId::parse_key(service_key).map(|id| id.service))
}

/// The bare logical (`pcx:`) form of `service_key`, which is how this app
/// identifies services; the transport adds the account namespace on the wire.
pub fn logical_key(service_key: &str) -> Option<String> {
    service_id_of(service_key).map(|id| id.key())
}

/// The protocol of `service_key`. Keys that are not Pocket-Codex session keys
/// keep the historical behaviour of addressing the native app-server.
pub fn protocol_of(service_key: &str) -> Protocol {
    match service_id_of(service_key).map(|id| id.kind) {
        Some(ServiceKind::Acp) => Protocol::Acp,
        _ => Protocol::CodexAppServer,
    }
}

/// The error for an operation `protocol` does not have.
pub fn unsupported(protocol: Protocol, operation: &str) -> anyhow::Error {
    anyhow!("{operation} is not available for {} services", protocol.id())
}

/// Fail unless `service_key` is a native Codex app-server.
pub fn require_native(service_key: &str, operation: &str) -> Result<()> {
    match protocol_of(service_key) {
        Protocol::CodexAppServer => Ok(()),
        other => Err(unsupported(other, operation)),
    }
}

/// `thread/start` options; protocols ignore what they cannot express.
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// Model id.
    pub model: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Codex approval policy.
    pub approval_policy: Option<String>,
    /// Codex approvals reviewer.
    pub approvals_reviewer: Option<String>,
    /// Codex service tier.
    pub service_tier: Option<String>,
    /// Codex sandbox mode.
    pub sandbox: Option<String>,
}

/// Turn options; protocols ignore what they cannot express.
#[derive(Debug, Clone, Default)]
pub struct TurnOptions {
    /// Prompt text.
    pub text: String,
    /// Data-URL images.
    pub images: Vec<String>,
    /// Model id.
    pub model: Option<String>,
    /// Codex approval policy.
    pub approval_policy: Option<String>,
    /// Codex approvals reviewer.
    pub approvals_reviewer: Option<String>,
    /// Codex service tier.
    pub service_tier: Option<String>,
    /// Codex sandbox mode.
    pub sandbox: Option<String>,
    /// Collaboration mode.
    pub collaboration_mode: Option<String>,
    /// Codex reasoning effort.
    pub reasoning_effort: Option<String>,
}

/// What a connection supports. Booleans default to "no": a feature is only
/// shown when the protocol (and, for ACP, the negotiated agent) has it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Provider family: `codex` or `acp`.
    pub provider: String,
    /// Human-readable agent name.
    pub provider_name: String,
    /// Protocol id (see [`Protocol::id`]).
    pub protocol: String,
    /// Whether the values come from a live negotiation (ACP) rather than the
    /// protocol's fixed feature set.
    pub negotiated: bool,
    /// Host generation the values belong to (ACP; 0 otherwise).
    pub generation: u64,
    /// Fast service tier toggle.
    pub fast: bool,
    /// Codex approval / sandbox presets.
    pub permission_presets: bool,
    /// Guardian (auto-review) approvals.
    pub guardian: bool,
    /// Account rate limits.
    pub rate_limits: bool,
    /// Taking over a session held by another local process.
    pub takeover: bool,
    /// Monitoring another writer.
    pub external_writer_monitor: bool,
    /// Sessions read from this device's disk.
    pub local_sessions: bool,
    /// Plan collaboration mode.
    pub plan_mode: bool,
    /// Native live voice calls.
    pub voice: bool,
    /// Native composer dictation.
    pub dictation: bool,
    /// Image attachments in prompts.
    pub image_input: bool,
    /// Supplementing a running turn.
    pub steer: bool,
    /// Renaming sessions.
    pub rename: bool,
    /// Manual compaction.
    pub compact: bool,
    /// Working-tree diff.
    pub git_diff: bool,
    /// A service-wide model catalog for the model picker.
    pub model_catalog: bool,
    /// Per-session agent configuration (ACP select options / modes).
    pub session_config: bool,
    /// Approvals carry the agent's own options (answered by option id).
    pub permission_options: bool,
    /// Native thread metadata (Guardian pre-resume checks).
    pub native_metadata: bool,
    /// How earlier sessions reopen: `native`, `load`, `resume`, `none` or
    /// `unknown` (not negotiated yet).
    pub session_reopen: String,
    /// Where the session list comes from: `native`, `agent` or `host`.
    pub session_list: String,
    /// `full` (the provider keeps complete history) or `retained` (only what
    /// the host still holds in memory).
    pub history_scope: String,
    /// Running-session inventory: `meta`, `engine` or `none`.
    pub running_inventory: String,
    /// The agent reported that authentication is required.
    pub auth_required: bool,
}

impl Capabilities {
    /// Native Codex: every field as it has always been.
    pub fn codex() -> Self {
        Self {
            provider: "codex".into(),
            provider_name: "Codex".into(),
            protocol: Protocol::CodexAppServer.id().into(),
            fast: true,
            permission_presets: true,
            guardian: true,
            rate_limits: true,
            takeover: true,
            external_writer_monitor: true,
            local_sessions: true,
            plan_mode: true,
            voice: true,
            dictation: true,
            image_input: true,
            steer: true,
            rename: true,
            compact: true,
            git_diff: true,
            model_catalog: true,
            native_metadata: true,
            session_reopen: "native".into(),
            session_list: "native".into(),
            history_scope: "full".into(),
            running_inventory: "meta".into(),
            ..Self::default()
        }
    }

    /// ACP before (or without) a negotiation: nothing optional.
    pub fn acp_unnegotiated() -> Self {
        Self {
            provider: "acp".into(),
            provider_name: "ACP agent".into(),
            protocol: Protocol::Acp.id().into(),
            permission_options: true,
            session_reopen: "unknown".into(),
            session_list: "host".into(),
            history_scope: "retained".into(),
            running_inventory: "engine".into(),
            ..Self::default()
        }
    }
}

/// One protocol's implementation of the shared session operations.
pub trait SessionEngine: Sync {
    /// The protocol.
    fn protocol(&self) -> Protocol;
    /// Connect (idempotent while healthy).
    fn connect(&self, service_key: String, local_port: u16) -> Result<()>;
    /// Whether a live connection exists.
    fn is_connected(&self, service_key: &str) -> bool;
    /// Drop this controller's connection (never another client's work).
    fn disconnect(&self, service_key: &str);
    /// Why the service is unreachable, or `None`.
    fn probe_reason(&self, service_key: String) -> Result<Option<String>>;
    /// Live events.
    fn subscribe_events(&self, service_key: &str) -> Result<broadcast::Receiver<AppEvent>>;
    /// Capabilities of the current connection.
    fn capabilities(&self, service_key: &str) -> Capabilities;
    /// Sessions.
    fn thread_list(&self, service_key: &str) -> Result<Vec<ThreadMeta>>;
    /// Model catalog.
    fn model_list(&self, service_key: &str) -> Result<Vec<ModelInfo>> {
        let _ = service_key;
        Err(unsupported(self.protocol(), "the model catalog"))
    }
    /// Create a session.
    fn thread_start(&self, service_key: &str, options: StartOptions) -> Result<String>;
    /// Reopen a session before reading it or sending turns.
    fn thread_resume(&self, service_key: &str, thread_id: &str) -> Result<()>;
    /// Newest history window. `include_turn_pages` keeps the native
    /// monitoring semantics (omit cached selected-turn windows when false).
    fn thread_read(
        &self,
        service_key: &str,
        thread_id: &str,
        include_turn_pages: bool,
    ) -> Result<ThreadHistory>;
    /// One page further back.
    fn thread_older_page(&self, service_key: &str, thread_id: &str) -> Result<OlderPage>;
    /// A selected turn's items; `delta_only` keeps the native continuation
    /// semantics.
    fn thread_turn_page(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: &str,
        load_more: bool,
        delta_only: bool,
    ) -> Result<TurnItemsPage>;
    /// Every item of one turn.
    fn thread_turn_items(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Vec<ThreadItem>> {
        self.thread_turn_page(service_key, thread_id, turn_id, false, false)
            .map(|page| page.items)
    }
    /// Cached runtime configuration (no network).
    fn thread_runtime_config(
        &self,
        service_key: &str,
        thread_id: &str,
    ) -> Option<ThreadRuntimeConfig>;
    /// Start a turn.
    fn turn_start(&self, service_key: &str, thread_id: &str, options: TurnOptions) -> Result<()>;
    /// Supplement the running turn.
    fn turn_steer(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: Option<&str>,
        text: &str,
        images: &[String],
    ) -> Result<String> {
        let _ = (service_key, thread_id, turn_id, text, images);
        Err(unsupported(self.protocol(), "supplementing a running turn"))
    }
    /// Interrupt / cancel the running turn.
    fn turn_interrupt(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: Option<String>,
    ) -> Result<()>;
    /// Answer an approval. ACP passes the chosen option id verbatim.
    fn respond_approval(&self, service_key: &str, request_id: &str, decision: &str) -> Result<()>;
    /// Answer a structured question.
    fn respond_user_input(
        &self,
        service_key: &str,
        request_id: &str,
        answers_json: &str,
    ) -> Result<()> {
        let _ = (service_key, request_id, answers_json);
        Err(unsupported(self.protocol(), "structured questions"))
    }
    /// Rename a session.
    fn set_thread_name(&self, service_key: &str, thread_id: &str, name: &str) -> Result<()> {
        let _ = (service_key, thread_id, name);
        Err(unsupported(self.protocol(), "renaming sessions"))
    }
    /// Manual compaction.
    fn compact(&self, service_key: &str, thread_id: &str) -> Result<()> {
        let _ = (service_key, thread_id);
        Err(unsupported(self.protocol(), "manual compaction"))
    }
    /// Working-tree diff.
    fn git_diff(&self, service_key: &str, cwd: &str) -> Result<String> {
        let _ = (service_key, cwd);
        Err(unsupported(self.protocol(), "the working-tree diff"))
    }
    /// One-line summary of a session (`None` when unavailable).
    fn thread_summary(&self, service_key: &str, thread_id: &str) -> Result<Option<String>> {
        let _ = (service_key, thread_id);
        Ok(None)
    }
    /// Sessions running now, from the engine itself.
    fn running_threads(&self, service_key: &str) -> Result<Vec<String>> {
        let _ = service_key;
        Err(unsupported(self.protocol(), "the engine running-session inventory"))
    }
    /// The agent's latest per-session state (select options, modes, usage)
    /// as this controller saw it; no network. `None` when the protocol has
    /// no such state.
    fn session_settings(&self, service_key: &str, thread_id: &str) -> Option<serde_json::Value> {
        let _ = (service_key, thread_id);
        None
    }
    /// Set a per-session select option to one of its advertised values.
    fn set_session_config(
        &self,
        service_key: &str,
        thread_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<()> {
        let _ = (service_key, thread_id, config_id, value);
        Err(unsupported(self.protocol(), "agent session options"))
    }
    /// Switch the per-session mode to an advertised one.
    fn set_session_mode(&self, service_key: &str, thread_id: &str, mode_id: &str) -> Result<()> {
        let _ = (service_key, thread_id, mode_id);
        Err(unsupported(self.protocol(), "agent session modes"))
    }
}

/// The engine for `service_key`.
pub fn engine(service_key: &str) -> &'static dyn SessionEngine {
    match protocol_of(service_key) {
        Protocol::CodexAppServer => &CodexEngine,
        Protocol::Acp => &AcpEngine,
    }
}

/// Native Codex app-server.
pub struct CodexEngine;

impl SessionEngine for CodexEngine {
    fn protocol(&self) -> Protocol {
        Protocol::CodexAppServer
    }

    fn connect(&self, service_key: String, local_port: u16) -> Result<()> {
        app_session::connect(service_key, local_port, &transport::resolve_blocking()?)
    }

    fn is_connected(&self, service_key: &str) -> bool {
        app_session::is_connected(service_key)
    }

    fn disconnect(&self, service_key: &str) {
        app_session::disconnect(service_key);
    }

    fn probe_reason(&self, service_key: String) -> Result<Option<String>> {
        Ok(app_session::probe_reason(service_key, 0, &transport::resolve_blocking()?))
    }

    fn subscribe_events(&self, service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
        app_session::subscribe_events(service_key)
    }

    fn capabilities(&self, _service_key: &str) -> Capabilities {
        Capabilities::codex()
    }

    fn thread_list(&self, service_key: &str) -> Result<Vec<ThreadMeta>> {
        app_session::thread_list(service_key)
    }

    fn model_list(&self, service_key: &str) -> Result<Vec<ModelInfo>> {
        app_session::model_list(service_key)
    }

    fn thread_start(&self, service_key: &str, options: StartOptions) -> Result<String> {
        app_session::thread_start(
            service_key,
            options.model,
            options.cwd,
            options.approval_policy,
            options.approvals_reviewer,
            options.service_tier,
            options.sandbox,
        )
    }

    fn thread_resume(&self, service_key: &str, thread_id: &str) -> Result<()> {
        app_session::thread_resume(service_key, thread_id)
    }

    fn thread_read(
        &self,
        service_key: &str,
        thread_id: &str,
        include_turn_pages: bool,
    ) -> Result<ThreadHistory> {
        if include_turn_pages {
            app_session::thread_read(service_key, thread_id)
        } else {
            app_session::thread_read_with_pages(service_key, thread_id, false)
        }
    }

    fn thread_older_page(&self, service_key: &str, thread_id: &str) -> Result<OlderPage> {
        app_session::thread_older_page(service_key, thread_id)
    }

    fn thread_turn_page(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: &str,
        load_more: bool,
        delta_only: bool,
    ) -> Result<TurnItemsPage> {
        if delta_only {
            app_session::thread_turn_page_delta(service_key, thread_id, turn_id, load_more)
        } else {
            app_session::thread_turn_page(service_key, thread_id, turn_id, load_more)
        }
    }

    fn thread_turn_items(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Vec<ThreadItem>> {
        app_session::thread_turn_items(service_key, thread_id, turn_id)
    }

    fn thread_runtime_config(
        &self,
        service_key: &str,
        thread_id: &str,
    ) -> Option<ThreadRuntimeConfig> {
        app_session::thread_runtime_config(service_key, thread_id)
    }

    fn turn_start(&self, service_key: &str, thread_id: &str, options: TurnOptions) -> Result<()> {
        app_session::turn_start(
            service_key,
            thread_id,
            options.text,
            options.images,
            options.model,
            options.approval_policy,
            options.approvals_reviewer,
            options.service_tier,
            options.sandbox,
            options.collaboration_mode,
            options.reasoning_effort,
        )
    }

    fn turn_steer(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: Option<&str>,
        text: &str,
        images: &[String],
    ) -> Result<String> {
        app_session::turn_steer(service_key, thread_id, turn_id, text, images)
    }

    fn turn_interrupt(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: Option<String>,
    ) -> Result<()> {
        app_session::turn_interrupt(service_key, thread_id, turn_id)
    }

    fn respond_approval(&self, service_key: &str, request_id: &str, decision: &str) -> Result<()> {
        app_session::respond_approval(service_key, request_id, decision)
    }

    fn respond_user_input(
        &self,
        service_key: &str,
        request_id: &str,
        answers_json: &str,
    ) -> Result<()> {
        app_session::respond_user_input(service_key, request_id, answers_json)
    }

    fn set_thread_name(&self, service_key: &str, thread_id: &str, name: &str) -> Result<()> {
        app_session::set_thread_name(service_key, thread_id, name)
    }

    fn compact(&self, service_key: &str, thread_id: &str) -> Result<()> {
        app_session::compact(service_key, thread_id)
    }

    fn git_diff(&self, service_key: &str, cwd: &str) -> Result<String> {
        app_session::git_diff(service_key, cwd)
    }

    fn thread_summary(&self, service_key: &str, thread_id: &str) -> Result<Option<String>> {
        app_session::thread_summary(service_key, thread_id)
    }
}

/// The generic ACP gateway.
pub struct AcpEngine;

impl SessionEngine for AcpEngine {
    fn protocol(&self) -> Protocol {
        Protocol::Acp
    }

    fn connect(&self, service_key: String, local_port: u16) -> Result<()> {
        acp::connect(&service_key, local_port, &transport::resolve_owned_blocking()?)
    }

    fn is_connected(&self, service_key: &str) -> bool {
        acp::is_connected(service_key)
    }

    fn disconnect(&self, service_key: &str) {
        acp::disconnect(service_key);
    }

    fn probe_reason(&self, service_key: String) -> Result<Option<String>> {
        Ok(acp::probe_reason(&service_key, &transport::resolve_owned_blocking()?))
    }

    fn subscribe_events(&self, service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
        acp::subscribe_events(service_key)
    }

    fn capabilities(&self, service_key: &str) -> Capabilities {
        acp::capabilities(service_key)
    }

    fn thread_list(&self, service_key: &str) -> Result<Vec<ThreadMeta>> {
        acp::thread_list(service_key)
    }

    fn thread_start(&self, service_key: &str, options: StartOptions) -> Result<String> {
        acp::thread_start(service_key, options.cwd)
    }

    fn thread_resume(&self, service_key: &str, thread_id: &str) -> Result<()> {
        acp::thread_resume(service_key, thread_id)
    }

    fn thread_read(
        &self,
        service_key: &str,
        thread_id: &str,
        _include_turn_pages: bool,
    ) -> Result<ThreadHistory> {
        acp::thread_read(service_key, thread_id)
    }

    fn thread_older_page(&self, service_key: &str, thread_id: &str) -> Result<OlderPage> {
        acp::thread_older_page(service_key, thread_id)
    }

    fn thread_turn_page(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: &str,
        load_more: bool,
        _delta_only: bool,
    ) -> Result<TurnItemsPage> {
        acp::thread_turn_page(service_key, thread_id, turn_id, load_more)
    }

    fn thread_runtime_config(
        &self,
        _service_key: &str,
        _thread_id: &str,
    ) -> Option<ThreadRuntimeConfig> {
        // ACP has no Codex runtime configuration; per-session agent options
        // are read through the session settings call instead.
        None
    }

    fn turn_start(&self, service_key: &str, thread_id: &str, options: TurnOptions) -> Result<()> {
        acp::turn_start(service_key, thread_id, &options.text, &options.images)
    }

    fn turn_interrupt(
        &self,
        service_key: &str,
        thread_id: &str,
        turn_id: Option<String>,
    ) -> Result<()> {
        acp::turn_interrupt(service_key, thread_id, turn_id)
    }

    fn respond_approval(&self, service_key: &str, request_id: &str, decision: &str) -> Result<()> {
        acp::respond_permission(service_key, request_id, decision)
    }

    fn running_threads(&self, service_key: &str) -> Result<Vec<String>> {
        acp::running_threads(service_key)
    }

    fn session_settings(&self, service_key: &str, thread_id: &str) -> Option<serde_json::Value> {
        acp::session_settings(service_key, thread_id)
    }

    fn set_session_config(
        &self,
        service_key: &str,
        thread_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<()> {
        acp::set_session_config(service_key, thread_id, config_id, value)
    }

    fn set_session_mode(&self, service_key: &str, thread_id: &str, mode_id: &str) -> Result<()> {
        acp::set_session_mode(service_key, thread_id, mode_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_follow_the_key_kind_in_both_namespaces() {
        assert_eq!(protocol_of("pcx:mac:app:default"), Protocol::CodexAppServer);
        assert_eq!(protocol_of("pcx:mac:acp:agent"), Protocol::Acp);
        assert_eq!(protocol_of("pcxu:alice:mac:acp:agent"), Protocol::Acp);
        // An instance name equal to a kind does not change the protocol.
        assert_eq!(protocol_of("pcx:mac:app:acp"), Protocol::CodexAppServer);
        assert_eq!(protocol_of("garbage"), Protocol::CodexAppServer);
        assert_eq!(logical_key("pcxu:alice:mac:acp:agent").as_deref(), Some("pcx:mac:acp:agent"));
    }

    #[test]
    fn native_codex_capabilities_are_unchanged() {
        let codex = Capabilities::codex();
        assert_eq!(
            (
                codex.provider.as_str(),
                codex.fast,
                codex.permission_presets,
                codex.guardian,
                codex.rate_limits,
                codex.takeover,
                codex.external_writer_monitor,
                codex.local_sessions,
                codex.plan_mode,
            ),
            ("codex", true, true, true, true, true, true, true, true)
        );
    }

    #[test]
    fn unnegotiated_acp_offers_nothing_optional() {
        let acp = Capabilities::acp_unnegotiated();
        assert!(!acp.negotiated);
        assert!(!acp.voice && !acp.dictation && !acp.fast && !acp.guardian);
        assert!(!acp.image_input && !acp.steer && !acp.model_catalog && !acp.session_config);
        assert!(!acp.native_metadata && !acp.plan_mode);
        assert_eq!(acp.session_reopen, "unknown");
    }

    #[test]
    fn native_only_calls_are_refused_for_other_protocols() {
        assert!(require_native("pcx:mac:app:default", "voice").is_ok());
        assert!(require_native("pcx:mac:acp:a", "thread metadata").is_err());
        assert!(engine("pcx:mac:acp:a").model_list("pcx:mac:acp:a").is_err());
        assert!(engine("pcx:mac:acp:a")
            .turn_steer("pcx:mac:acp:a", "t", None, "x", &[])
            .is_err());
        // Agent session options exist only for ACP.
        assert!(engine("pcx:mac:app:default")
            .session_settings("pcx:mac:app:default", "t")
            .is_none());
        assert!(engine("pcx:mac:app:default")
            .set_session_config("pcx:mac:app:default", "t", "model", "x")
            .is_err());
        assert!(engine("pcx:mac:app:default")
            .set_session_mode("pcx:mac:app:default", "t", "code")
            .is_err());
    }
}

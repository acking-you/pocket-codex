//! FRB-exposed bridge surface: config, discovery, API-service subscribe,
//! and app-server remote control (sessions, threads, turns, event stream).
//! Thin glue over `crate::engine`; DTOs are plain (FRB-friendly) structs.
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use flutter_rust_bridge::frb;
use pocket_codex_core::config::Mode;

use crate::{
    engine::{
        account, app_session, config, discovery, logging, meta, runtime, serve, serve_acp,
        serve_opencode,
        session_engine::{self, engine, require_native, Protocol, StartOptions, TurnOptions},
        sessions, transport,
    },
    frb_generated::StreamSink,
};

/// View of persisted config for the UI; never exposes the raw key or token.
pub struct ConfigView {
    /// Configured relay `host:port`, if any.
    pub relay: Option<String>,
    /// Whether a 32-byte key is stored (value withheld).
    pub has_key: bool,
    /// Configured UI locale (BCP-47, e.g. `en`/`zh`), or `None` to follow
    /// the system locale.
    pub locale: Option<String>,
    /// Active transport mode: `account` / `self_host` / `unconfigured`.
    pub mode: String,
    /// Signed-in GitHub login (account mode), if any.
    pub account_login: Option<String>,
    /// Signed-in GitHub numeric account id, if any. The UI builds the avatar
    /// URL from it; there is no avatar field to fetch.
    pub account_id: Option<String>,
    /// Whether an account session token is stored (value withheld).
    pub has_account_token: bool,
}

/// A discovered service, mirrored for Dart.
pub struct ServiceIdDto {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
    /// Full relay key.
    pub key: String,
}

/// Status of one active subscription, mirrored for Dart.
pub struct SubStatusDto {
    /// Service key.
    pub key: String,
    /// Local `host:port`.
    pub local_addr: String,
    /// Task still running.
    pub alive: bool,
}

/// Initialise the engine with the platform app-support dir (from Dart's
/// path_provider). Must be called once after `RustLib.init()`.
pub fn init_bridge(support_dir: String) -> Result<()> {
    // Pin the process crypto provider before any TLS connection is opened.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Capture bridge events before starting local services.
    logging::init();
    let support_dir = PathBuf::from(support_dir);
    // Mirror the captured log to disk (last 6 hours), so a hang can be read
    // after the fact instead of only in a viewer that was open at the time.
    logging::init_file(&support_dir);
    runtime::init(support_dir)
}

/// Current config view (relay/key presence, locale, and account state).
pub fn get_config() -> Result<ConfigView> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let mode = match cfg.account_mode() {
        Mode::Account => "account",
        Mode::SelfHost => "self_host",
        Mode::Unconfigured => "unconfigured",
    }
    .to_string();
    Ok(ConfigView {
        relay: cfg.relay().map(str::to_string),
        has_key: cfg.relay_key().is_some(),
        locale: cfg.locale().map(str::to_string),
        mode,
        account_login: cfg.account_login().map(str::to_string),
        account_id: cfg.account_id().map(str::to_string),
        has_account_token: cfg.account_token().is_some(),
    })
}

/// Set the relay `host:port` and persist.
pub fn set_relay(relay: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay(&relay);
        Ok(())
    })?;
    // Connections of the previous relay are not this relay's services.
    transport::context_changed();
    Ok(())
}

/// Set the 32-byte MSG_HEADER_KEY and persist (validates length).
pub fn set_key(key: String) -> Result<()> {
    if key.len() != 32 {
        return Err(anyhow!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", key.len()));
    }
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay_key(&key);
        Ok(())
    })?;
    transport::context_changed();
    Ok(())
}

/// Set the UI locale (BCP-47, e.g. `en`/`zh`) and persist. An empty string
/// clears it, meaning the app follows the system locale.
pub fn set_locale(locale: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_locale(&locale);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// external codex bootstrap: CODEX_HOME provider setup, ChatGPT login, system
// prompt
// ---------------------------------------------------------------------------

/// What the external codex has on disk in `CODEX_HOME`, for the onboarding
/// wizard.
pub struct CodexSetupStatusDto {
    /// Resolved `CODEX_HOME` (display path).
    pub codex_home: String,
    /// `config.toml` exists.
    pub has_config: bool,
    /// A credential exists (`auth.json` or `CODEX_ACCESS_TOKEN`).
    pub has_auth: bool,
    /// A non-OpenAI custom provider is configured (authorizes turns on its
    /// own).
    pub has_custom_provider: bool,
    /// `auth.json`'s `auth_mode` (`apikey` / `chatgpt` / …), when present.
    pub auth_mode: Option<String>,
    /// Nothing lets codex authenticate yet → show the setup wizard.
    pub needs_setup: bool,
    /// Active system-prompt variant (`default` / `non_degraded` / `custom`).
    pub prompt_variant: String,
}

/// Detect whether the external codex has a usable provider + credentials.
/// Drives the first-run setup wizard: `needs_setup` is `true` when neither a
/// login nor a custom provider is configured.
pub fn codex_setup_status() -> Result<CodexSetupStatusDto> {
    let s = pocket_codex_codex::setup::setup_status()?;
    Ok(CodexSetupStatusDto {
        codex_home: s.codex_home,
        has_config: s.has_config,
        has_auth: s.has_auth,
        has_custom_provider: s.has_custom_provider,
        auth_mode: s.auth_mode,
        needs_setup: s.needs_setup,
        prompt_variant: s.prompt_variant,
    })
}

/// Configure a minimal custom OpenAI-compatible provider (base URL + API key)
/// for the external codex, writing `$CODEX_HOME/config.toml`. No `codex login`
/// is needed — the key rides as the provider's bearer token. `model` is
/// optional (defaults to a sensible model). Takes effect on the next hosting
/// start.
pub fn codex_setup_provider(
    base_url: String,
    api_key: String,
    model: Option<String>,
) -> Result<()> {
    pocket_codex_codex::setup::write_provider_config(&base_url, &api_key, model.as_deref())
}

/// The active external-codex system-prompt variant (`default` / `non_degraded`
/// / `custom`).
pub fn codex_prompt_variant() -> Result<String> {
    Ok(pocket_codex_codex::setup::setup_status()?.prompt_variant)
}

/// Switch the external-codex system prompt. `non_degraded` swaps in the bundled
/// prompt that drops the commentary / intermediary-update mandates (which can
/// starve reasoning, see openai/codex#30364); `default` restores codex's
/// built-in prompt. Takes effect for threads started after the change.
pub fn codex_set_prompt_variant(variant: String) -> Result<()> {
    pocket_codex_codex::setup::set_prompt_variant(&variant)
}

/// A started ChatGPT login on the external codex, mirrored for Dart. `mode` is
/// `"browser"` (open `auth_url`) or `"device"` (open `verification_url` and
/// enter `user_code`) — codex falls back to device code when it can't bind its
/// local OAuth callback port.
pub struct CodexLoginStartDto {
    /// `"browser"` or `"device"`.
    pub mode: String,
    /// Opaque id to pass back to [`codex_login_cancel`].
    pub login_id: String,
    /// Browser flow: OAuth URL to open. `None` for device flow.
    pub auth_url: Option<String>,
    /// Device flow: URL to open. `None` for browser flow.
    pub verification_url: Option<String>,
    /// Device flow: one-time code to enter. `None` for browser flow.
    pub user_code: Option<String>,
}

/// codex auth status for the app-server behind `service_key`, mirrored for
/// Dart.
pub struct CodexAuthStatusDto {
    /// Signed in (a credential is active).
    pub authenticated: bool,
    /// Method when signed in (`chatgpt` / `apikey` / …).
    pub method: Option<String>,
}

/// Begin codex's official ChatGPT login on the app-server behind `service_key`
/// (which must be a connected host). Tries the browser flow, falling back to
/// device code when codex can't bind its local callback port (`:1455`/`:1457`,
/// reserved on many Windows machines). codex writes `auth.json` itself —
/// Pocket- Codex never generates the credential. Poll [`codex_auth_status`]
/// until `authenticated`. The OAuth HTTP inherits the proxy the host was
/// started with, so a China-network user hosts with a proxy and login goes
/// through it.
pub fn codex_login_chatgpt_start(service_key: String) -> Result<CodexLoginStartDto> {
    let s = app_session::login_chatgpt_start(&service_key)?;
    Ok(CodexLoginStartDto {
        mode: s.mode,
        login_id: s.login_id,
        auth_url: s.auth_url,
        verification_url: s.verification_url,
        user_code: s.user_code,
    })
}

/// Poll the codex auth status for the app-server behind `service_key`.
pub fn codex_auth_status(service_key: String) -> Result<CodexAuthStatusDto> {
    let (authenticated, method) = app_session::auth_status(&service_key)?;
    Ok(CodexAuthStatusDto {
        authenticated,
        method,
    })
}

/// Cancel an in-flight ChatGPT login (from [`codex_login_chatgpt_start`]).
pub fn codex_login_cancel(service_key: String, login_id: String) -> Result<()> {
    app_session::login_cancel(&service_key, &login_id)
}

/// Sign the external codex out (revoke + delete its `auth.json`) on
/// `service_key`.
pub fn codex_logout(service_key: String) -> Result<()> {
    app_session::codex_logout(&service_key)
}

/// Import a `pcx1:` share string: decode, persist relay + key, return relay.
pub fn import_config(text: String) -> Result<String> {
    let payload = config::decode_pcx1(&text)?;
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay(&payload.relay);
        cfg.set_relay_key(&payload.key);
        Ok(())
    })?;
    transport::context_changed();
    Ok(payload.relay)
}

/// Export the current relay+key as a `pcx1:` share string.
pub fn export_config() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let relay = cfg.relay().ok_or_else(|| anyhow!("no relay configured"))?;
    let key = cfg
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    config::encode_pcx1(relay, key)
}

/// Discover the services this device can reach on the relay.
///
/// One query in both modes: an account credential sees only its own namespace,
/// so the relay's listing IS the account's inventory. Reported with BARE `pcx:`
/// keys whatever the mode, because that is the identity the app and its Dart
/// layer use — [`Transport::relay_key`] maps back when the relay is next
/// addressed.
pub fn discover_services() -> Result<Vec<ServiceIdDto>> {
    let transport = transport::resolve_blocking()?;
    let found = runtime::runtime().block_on(discovery::discover(&transport))?;
    Ok(found
        .into_iter()
        .map(|id| ServiceIdDto {
            device: id.device.clone(),
            kind: id.kind.as_key_segment().to_string(),
            name: id.name.clone(),
            key: id.key(),
        })
        .collect())
}

/// Subscribe to an API service, exposing it on `127.0.0.1:<local_port>`.
pub fn api_subscribe(service_key: String, local_port: u16) -> Result<SubStatusDto> {
    let transport = transport::resolve_blocking()?;
    let s = runtime::subscribe_service(service_key, local_port, &transport)?;
    Ok(SubStatusDto {
        key: s.key,
        local_addr: s.local_addr,
        alive: s.alive,
    })
}

/// Stop an API-service subscription.
pub fn api_unsubscribe(service_key: String) {
    runtime::unsubscribe_service(&service_key);
}

/// List all active subscriptions.
pub fn subscriptions() -> Vec<SubStatusDto> {
    runtime::list_subscriptions()
        .into_iter()
        .map(|s| SubStatusDto {
            key: s.key,
            local_addr: s.local_addr,
            alive: s.alive,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Local hosting (desktop): run + manage a local codex app-server, the in-app
// equivalent of `pocket-codex serve` (account mode).
// ---------------------------------------------------------------------------

/// Result of starting local hosting, mirrored for Dart. One host publishes two
/// tunnels (`app:<name>` remote control + `api:<name>` Responses proxy).
pub struct AppServeDto {
    /// Device id both services registered under.
    pub device: String,
    /// Service instance name (shared by the app + api tunnels).
    pub name: String,
    /// `pcx:<device>:app:<name>` key (what discovery + `app_connect` use).
    pub app_service_key: String,
    /// Loopback `host:port` codex is listening on.
    pub app_listen_addr: String,
    /// `pcx:<device>:api:<name>` key (what an `api connect` resolves).
    pub api_service_key: String,
    /// Loopback `host:port` the in-app Responses API proxy is listening on.
    pub api_listen_addr: String,
    /// `pcx:<device>:meta:<name>` key (the host meta service tunnel).
    pub meta_service_key: String,
    /// Loopback `host:port` the in-app meta service is listening on.
    pub meta_listen_addr: String,
    /// The codex process id.
    pub pid: u32,
    /// Whether an already-running host was reused instead of freshly spawned.
    pub reused: bool,
}

/// Status of one local host, mirrored for Dart. Each host carries both tunnels'
/// publish state so the UI can offer per-tunnel 注销 / 重新注册.
pub struct AppServeStatusDto {
    /// Service instance name.
    pub name: String,
    /// Device id.
    pub device: String,
    /// codex process id.
    pub pid: Option<u32>,
    /// codex is accepting on its listen port.
    pub alive: bool,
    /// Loopback `host:port` codex listens on.
    pub app_listen_addr: String,
    /// `pcx:<device>:app:<name>` key.
    pub app_service_key: String,
    /// The app tunnel is currently published.
    pub app_registered: bool,
    /// Loopback `host:port` the API proxy listens on.
    pub api_listen_addr: String,
    /// `pcx:<device>:api:<name>` key.
    pub api_service_key: String,
    /// The api tunnel is currently published.
    pub api_registered: bool,
    /// Loopback `host:port` the meta service listens on.
    pub meta_listen_addr: String,
    /// `pcx:<device>:meta:<name>` key.
    pub meta_service_key: String,
    /// The meta tunnel is currently published.
    pub meta_registered: bool,
    /// Legacy runtime flag; always false for external Codex hosts.
    pub embedded: bool,
    /// The resolved external codex binary path.
    pub codex_binary: Option<String>,
    /// Upstream proxy codex + the API proxy were started with, or `None` when
    /// they inherit the app's environment.
    pub proxy: Option<String>,
    /// Service provider family of this host: `codex`, `opencode` or `acp`.
    /// For `opencode` / `acp` the `app_*` fields describe the gateway and
    /// `api_*` are empty.
    pub provider: String,
    /// Provider version when known (OpenCode, ACP agents).
    pub provider_version: Option<String>,
    /// Whether that version is the one this build was verified against.
    pub provider_verified: bool,
    /// Wire protocol: `codex-app-server`, `opencode-http` or `acp`.
    pub protocol: String,
    /// Human-readable provider / agent name.
    pub provider_name: String,
    /// ACP agent profile id.
    pub profile_id: Option<String>,
    /// ACP agent phase: `starting`, `ready`, `failed` or `stopped`.
    pub agent_phase: Option<String>,
    /// Why the ACP agent is not running, safe to show.
    pub agent_error: Option<String>,
    /// The ACP agent reported that authentication is required.
    pub auth_required: bool,
}

/// Result of attaching and publishing a local OpenCode service.
pub struct OpenCodeServeDto {
    /// Device id the services registered under.
    pub device: String,
    /// Instance name.
    pub name: String,
    /// `pcx:<device>:opencode:<name>` key.
    pub service_key: String,
    /// Loopback gateway address.
    pub listen_addr: String,
    /// `pcx:<device>:meta:<name>` key.
    pub meta_service_key: String,
    /// OpenCode version.
    pub version: String,
    /// Whether the version is the verified one (others passed the contract).
    pub verified: bool,
    /// Whether an existing host was reused.
    pub reused: bool,
    /// Whether this call asked OpenCode to start its background service.
    pub started_service: bool,
}

/// Attach to the local OpenCode background service (asking OpenCode to start
/// it when none is running) and publish it as `opencode:<name>` plus a
/// `meta:<name>` service. Stopping hosting never stops OpenCode. The name must
/// not be in use by a Codex host on this device.
pub fn app_serve_start_opencode(
    name: Option<String>,
    binary_override: Option<String>,
) -> Result<OpenCodeServeDto> {
    let r = serve_opencode::start(name, binary_override)?;
    Ok(OpenCodeServeDto {
        device: r.device,
        name: r.name,
        service_key: r.service_key,
        listen_addr: r.listen_addr,
        meta_service_key: r.meta_service_key,
        version: r.version,
        verified: r.verified,
        reused: r.reused,
        started_service: r.started_service,
    })
}

/// The resolved `opencode` executable (explicit → `$PATH` →
/// `~/.opencode/bin/opencode`), or `None`.
pub fn opencode_locate(binary_override: Option<String>) -> Option<String> {
    serve_opencode::locate(binary_override.as_deref()).map(|p| p.display().to_string())
}

/// An ACP agent to host: profile identity plus executable and argument
/// vector, passed to the operating system verbatim (no shell).
pub struct AcpAgentSpecDto {
    /// Profile id (`opencode` for the preset, or a custom id).
    pub profile_id: String,
    /// Name shown in the UI.
    pub display_name: String,
    /// Executable: an explicit path, or a bare name searched on `PATH`.
    pub program: String,
    /// Arguments, one entry per argument.
    pub args: Vec<String>,
}

/// A built-in ACP agent preset.
pub struct AcpPresetDto {
    /// Profile id.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Bare executable name.
    pub program: String,
    /// Default arguments.
    pub args: Vec<String>,
}

/// Result of hosting an ACP agent.
pub struct AcpServeDto {
    /// Device id the services registered under.
    pub device: String,
    /// Instance name.
    pub name: String,
    /// `pcx:<device>:acp:<name>` key.
    pub service_key: String,
    /// Loopback gateway address.
    pub listen_addr: String,
    /// `pcx:<device>:meta:<name>` key.
    pub meta_service_key: String,
    /// Profile id.
    pub profile_id: String,
    /// Display name.
    pub display_name: String,
    /// Whether an existing host was reused.
    pub reused: bool,
}

/// Built-in ACP agent presets (OpenCode: `opencode acp`).
#[frb(sync)]
pub fn acp_presets() -> Vec<AcpPresetDto> {
    pocket_codex_host_svc::acp::PRESETS
        .iter()
        .map(|preset| AcpPresetDto {
            id: preset.id.to_string(),
            display_name: preset.display_name.to_string(),
            program: preset.program.to_string(),
            args: preset.args.iter().map(|arg| arg.to_string()).collect(),
        })
        .collect()
}

/// Whether this device can host ACP agents (Unix desktops). Any device can
/// use a remote ACP host.
#[frb(sync)]
pub fn acp_hosting_supported() -> bool {
    serve_acp::hosting_supported()
}

/// The executable `program` resolves to (explicit path, `PATH`, then the
/// preset's install locations), or `None`.
pub fn acp_locate(program: String, profile_id: Option<String>) -> Option<String> {
    serve_acp::locate(&program, profile_id.as_deref()).map(|p| p.display().to_string())
}

/// Launch and host an ACP agent as `acp:<name>` plus `meta:<name>`. Works in
/// self-host and account mode. Stopping hosting stops the agent.
pub fn app_serve_start_acp(name: Option<String>, spec: AcpAgentSpecDto) -> Result<AcpServeDto> {
    let r = serve_acp::start(name, pocket_codex_host_svc::acp::AgentSpec {
        profile_id: spec.profile_id,
        display_name: spec.display_name,
        program: spec.program,
        args: spec.args,
    })?;
    Ok(AcpServeDto {
        device: r.device,
        name: r.name,
        service_key: r.service_key,
        listen_addr: r.listen_addr,
        meta_service_key: r.meta_service_key,
        profile_id: r.profile_id,
        display_name: r.display_name,
        reused: r.reused,
    })
}

/// Restart the agent of a local ACP host (new process, new generation).
pub fn app_serve_restart_acp(name: String) -> Result<()> {
    serve_acp::restart(&name)
}

/// Legacy version endpoint; returns `unavailable` because no engine is bundled.
pub fn embedded_codex_version() -> String {
    pocket_codex_codex::EMBEDDED_CODEX_COMMIT.to_string()
}

/// Start hosting a local codex app-server **and** Responses API proxy under the
/// signed-in account, publishing both `app:<name>` and `api:<name>`. Re-hosting
/// a name whose codex is still alive just re-registers any dropped tunnels.
/// `proxy` is the upstream proxy both use to reach chatgpt.com (`None` =
/// inherit env). Desktop only. `embedded` is retained for bridge compatibility;
/// passing true returns a built-in-engine-not-implemented error before startup.
pub fn app_serve_start(
    port: u16,
    binary_override: Option<String>,
    name: Option<String>,
    proxy: Option<String>,
    embedded: bool,
) -> Result<AppServeDto> {
    let r = serve::serve_start(port, binary_override, name, proxy, embedded)?;
    Ok(AppServeDto {
        device: r.device,
        name: r.name,
        app_service_key: r.app_service_key,
        app_listen_addr: r.app_listen_addr,
        api_service_key: r.api_service_key,
        api_listen_addr: r.api_listen_addr,
        meta_service_key: r.meta_service_key,
        meta_listen_addr: r.meta_listen_addr,
        pid: r.pid,
        reused: r.reused,
    })
}

/// Snapshot of every local host (for the status cards + periodic re-probe).
pub fn app_serve_status() -> Vec<AppServeStatusDto> {
    serve::serve_status()
        .into_iter()
        .map(|s| AppServeStatusDto {
            name: s.name,
            device: s.device,
            pid: s.pid,
            alive: s.alive,
            app_listen_addr: s.app_listen_addr,
            app_service_key: s.app_service_key,
            app_registered: s.app_registered,
            api_listen_addr: s.api_listen_addr,
            api_service_key: s.api_service_key,
            api_registered: s.api_registered,
            meta_listen_addr: s.meta_listen_addr,
            meta_service_key: s.meta_service_key,
            meta_registered: s.meta_registered,
            embedded: s.embedded,
            codex_binary: s.codex_binary,
            proxy: s.proxy,
            provider: s.provider,
            provider_version: s.provider_version,
            provider_verified: s.provider_verified,
            protocol: s.protocol,
            provider_name: s.provider_name,
            profile_id: s.profile_id,
            agent_phase: s.agent_phase,
            agent_error: s.agent_error,
            auth_required: s.auth_required,
        })
        .collect()
}

/// Take one tunnel (`kind` = `"app"`/`"api"`/`"meta"`) of a local host off the
/// relay without stopping the host — a reversible unpublish. The codex / API
/// proxy / meta service keep running; [`app_serve_reregister`] re-publishes it.
pub fn app_serve_deregister(name: String, kind: String) -> Result<()> {
    serve::serve_deregister(&name, &kind)
}

/// Re-publish a previously deregistered tunnel (`kind` = `"app"`/`"api"`/
/// `"meta"`) of a still-running local host.
pub fn app_serve_reregister(name: String, kind: String) -> Result<()> {
    serve::serve_reregister(&name, &kind)
}

/// Fully stop one local host by name (both tunnels + watchdog + API proxy, and
/// stops codex).
pub fn app_serve_stop(name: String) -> Result<()> {
    serve::serve_stop(&name)
}

/// Stop every local host (called on app quit so a real quit leaves no orphan
/// codex). A no-op when nothing is hosting.
pub fn app_serve_stop_all() -> Result<()> {
    serve::serve_stop_all();
    Ok(())
}

/// The resolved `codex` binary path (persisted config → `$PATH`), or `None` so
/// the UI can prompt the user to point at one.
pub fn codex_locate() -> Option<String> {
    serve::codex_locate()
}

// ---------------------------------------------------------------------------
// App-server remote control
// ---------------------------------------------------------------------------

/// One app-server event mirrored for Dart. `kind` is the JSON-RPC method
/// (e.g. `turn/started`, `item/agentMessage/delta`, `turn/completed`).
pub struct AppEventDto {
    /// JSON-RPC method name of the originating notification.
    pub kind: String,
    /// Thread id the event belongs to, when present.
    pub thread_id: Option<String>,
    /// Item id the event refers to, when present.
    pub item_id: Option<String>,
    /// Item type tag when this event carries an item (`agentMessage`,
    /// `commandExecution`, `webSearch`, `mcpToolCall`, `fileChange`,
    /// `reasoning`, …); `None` for turn-level events.
    pub item_type: Option<String>,
    /// One-line summary for tool/activity items (command, query, tool name…).
    pub title: Option<String>,
    /// Text payload (a streaming delta or an item's body/detail).
    pub text: Option<String>,
    /// User attachments or generated artifacts, as data URLs or host paths.
    pub images: Vec<String>,
    /// Token to answer a server approval request via [`app_respond_approval`];
    /// `None` for ordinary notifications.
    pub request_id: Option<String>,
    /// Full params JSON for fields not modelled above.
    pub raw: String,
}

/// Thread summary mirrored for Dart.
pub struct ThreadMetaDto {
    /// Thread id.
    pub id: String,
    /// Preview (usually the first user message).
    pub preview: String,
    /// User-set title, or `None` when the thread was never renamed (the UI
    /// falls back to `preview`).
    pub name: Option<String>,
    /// App-owned classification, including `pocket-codex-voice`.
    pub thread_source: Option<String>,
    /// Parent thread for a spawned child.
    pub parent_thread_id: Option<String>,
    /// Working directory (the project the thread controls).
    pub cwd: String,
    /// Unix seconds of last update.
    pub updated_at: i64,
}

/// One model offered by the app-server, mirrored for Dart.
pub struct ModelInfoDto {
    /// Model id (used as the `model` param).
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// Short description.
    pub description: String,
    /// Reasoning efforts this model supports (so the UI offers only valid
    /// levels).
    pub supported_reasoning_efforts: Vec<String>,
    /// The model's default reasoning effort, if any.
    pub default_reasoning_effort: Option<String>,
    /// Service tier ids advertised by the model catalog.
    pub supported_service_tiers: Vec<String>,
    /// Catalog default service tier.
    pub default_service_tier: Option<String>,
    /// Whether this is the server default model.
    pub is_default: bool,
}

/// One materialised conversation item mirrored for Dart.
pub struct ThreadItemDto {
    /// Item id.
    pub id: String,
    /// Item type tag (`userMessage` / `agentMessage` / `commandExecution` /
    /// `webSearch` / `mcpToolCall` / `fileChange` / `reasoning` / …).
    pub item_type: String,
    /// One-line summary for tool/activity items.
    pub title: String,
    /// Body / detail text.
    pub text: String,
    /// Structured asynchronous questions on an agent message, as JSON.
    pub questions_json: Option<String>,
    /// User attachments or generated artifacts, as data URLs or host paths.
    pub images: Vec<String>,
    /// Id of the turn this item belongs to — the server's own turn boundary
    /// (`thread/read` nests items under their turn), so the UI can render one
    /// turn's reply as one block instead of inferring boundaries from the item
    /// sequence. Empty when the item came from the live stream buffer.
    pub turn_id: String,
    /// Unix seconds when this item's turn completed; `None` while it runs.
    pub turn_completed_at: Option<i64>,
    /// This item's turn duration in milliseconds, when the server reports it.
    pub turn_duration_ms: Option<i64>,
}

/// A thread's recovered history + whether a turn is still running, plus the
/// metadata the status bar / git chip seed from on open.
pub struct ThreadHistoryDto {
    /// Source generation used to invalidate replaced historical UI windows.
    pub history_epoch: Option<String>,
    /// Conversation items, oldest first.
    pub items: Vec<ThreadItemDto>,
    /// Whether the most recent turn is still in progress.
    pub running: bool,
    /// Identity from the same turn snapshot that established `running`.
    pub active_turn_id: Option<String>,
    /// Current git branch of the thread's cwd, if it's a repo.
    pub branch: Option<String>,
    /// The thread's resolved working directory (for git diff / status).
    pub cwd: Option<String>,
    /// Tokens currently occupying the model context window.
    pub tokens_used: Option<i64>,
    /// The model's context-window size in tokens.
    pub context_window: Option<i64>,
    /// Sticky collaboration mode (`"plan"` / `"default"`) so the UI plan toggle
    /// reflects the server's real state.
    pub collaboration_mode: Option<String>,
    /// Current reasoning effort (`"low"`/`"medium"`/`"high"`) so the UI can
    /// show the "thinking level" the thread runs with (from the resume
    /// response).
    pub reasoning_effort: Option<String>,
    /// The effective model id the thread runs with, per the server. `None`
    /// when the server never reported it (older servers).
    pub model: Option<String>,
    /// Provider of the effective model (e.g. `openai`), when reported.
    pub model_provider: Option<String>,
    /// The effective approval policy (`untrusted`/`on-failure`/`on-request`/
    /// `never`/`granular`), when reported.
    pub approval_policy: Option<String>,
    /// Approval reviewer (`user` or `auto_review`), when reported.
    pub approvals_reviewer: Option<String>,
    /// Effective service tier, when reported.
    pub service_tier: Option<String>,
    /// The effective sandbox mode (`read-only`/`workspace-write`/
    /// `danger-full-access`/`external-sandbox`), when reported.
    pub sandbox_mode: Option<String>,
    /// Whether a live `thread/settings/updated` notification has confirmed
    /// this config (vs only a start/resume snapshot).
    pub config_confirmed: bool,
    /// Whether earlier items remain unread — [`app_thread_older_page`] fetches
    /// them. False for a thread whose history arrives whole.
    pub has_older: bool,
    /// One entry per turn in the WHOLE thread, oldest first, including turns
    /// whose items aren't loaded yet. The turn rail shows a conversation's
    /// shape, so it needs every turn even before their bodies are read.
    pub turns: Vec<TurnSummaryDto>,
    /// Actual first turn when the server's summary cursor was exhausted.
    pub first_turn_id: Option<String>,
    /// Cached pages of independently selected turns.
    pub turn_pages: Vec<TurnItemsPageDto>,
}

/// A selected turn's bounded, ascending history window.
pub struct TurnItemsPageDto {
    /// Turn owning the window.
    pub turn_id: String,
    /// Loaded items in chronological order.
    pub items: Vec<ThreadItemDto>,
    /// Whether this turn still has a continuation page.
    pub has_more: bool,
}

fn turn_page_dto(page: app_session::TurnItemsPage) -> TurnItemsPageDto {
    TurnItemsPageDto {
        turn_id: page.turn_id,
        items: page.items.into_iter().map(item_dto).collect(),
        has_more: page.has_more,
    }
}

/// Read or continue a selected turn, reusing its previously loaded pages.
/// `delta_only` opts into new items only on continuation; omission preserves
/// cumulative windows for older callers. Opening always returns the cached
/// prefix.
pub fn app_thread_turn_page(
    service_key: String,
    thread_id: String,
    turn_id: String,
    load_more: bool,
    delta_only: Option<bool>,
) -> Result<TurnItemsPageDto> {
    engine(&service_key)
        .thread_turn_page(
            &service_key,
            &thread_id,
            &turn_id,
            load_more,
            delta_only.unwrap_or(false),
        )
        .map(turn_page_dto)
}

/// A turn reduced to what the rail shows.
pub struct TurnSummaryDto {
    /// Id of the turn, for fetching its items on demand.
    pub turn_id: String,
    /// The user's message that opened the turn; empty when it had none.
    pub user_text: String,
    /// The turn's final agent message; empty when it produced no prose.
    pub assistant_text: String,
    /// Whether this turn's items are already in the transcript.
    pub loaded: bool,
}

/// One page of older items, and whether history continues before them.
pub struct OlderPageDto {
    /// Older items, oldest first, to prepend to the transcript.
    pub items: Vec<ThreadItemDto>,
    /// Whether older items still remain.
    pub has_older: bool,
}

/// The server-reported runtime configuration of a thread — what its turns
/// actually run with. Mirrors the engine cache fed by `thread/start` /
/// `thread/resume` responses and live `thread/settings/updated` notifications;
/// every field is `None` when the server hasn't said (never a guess).
pub struct ThreadRuntimeConfigDto {
    /// Effective model id.
    pub model: Option<String>,
    /// Provider of the effective model.
    pub model_provider: Option<String>,
    /// Effective reasoning effort.
    pub reasoning_effort: Option<String>,
    /// Effective approval policy.
    pub approval_policy: Option<String>,
    /// Approval reviewer (`user` or `auto_review`), when reported.
    pub approvals_reviewer: Option<String>,
    /// Effective service tier, when reported.
    pub service_tier: Option<String>,
    /// Effective sandbox mode (kebab wire string).
    pub sandbox_mode: Option<String>,
    /// Effective collaboration mode (`plan`/`default`), when reported.
    pub collaboration_mode: Option<String>,
    /// True once a live settings update confirmed this config.
    pub confirmed_by_update: bool,
}

/// Connect to an app-server service: subscribe on `127.0.0.1:<local_port>`,
/// open the JSON-RPC websocket and run the `initialize` handshake. Idempotent.
pub fn app_connect(service_key: String, local_port: u16) -> Result<()> {
    engine(&service_key).connect(service_key.clone(), local_port)
}

/// Whether a live app-server session exists for `service_key`.
#[frb(sync)]
pub fn app_is_connected(service_key: String) -> bool {
    engine(&service_key).is_connected(&service_key)
}

/// Disconnect this controller's session and its pb-mapper subscription.
/// Work owned by a host (ACP turns, permissions) keeps running there.
pub fn app_disconnect(service_key: String) {
    engine(&service_key).disconnect(&service_key);
}

/// Probe whether an app-server is actually REACHABLE — its backend responds to
/// a handshake — rather than merely registered on the relay. The services list
/// uses this so a registered-but-dead app-server (a live relay registrant
/// forwarding to a codex app-server that has died) shows as unreachable instead
/// of a false "online". Opens a transient tunnel + `initialize` with a timeout,
/// then tears it down; a live session short-circuits to `true`.
pub fn app_probe(service_key: String) -> Result<bool> {
    Ok(app_probe_reason(service_key)?.is_none())
}

/// Why [`app_probe`] considers a service unreachable, or `None` when it is
/// reachable.
///
/// A bare `false` made the UI say "the app-server did not respond", which is
/// wrong whenever the tunnel DID answer and refused us — a relay rejecting the
/// handshake (missing or stale authentication code) is the common case, and it
/// needs a different fix from a dead backend. This hands the transport's own
/// words to the UI so it can name the actual problem.
pub fn app_probe_reason(service_key: String) -> Result<Option<String>> {
    engine(&service_key).probe_reason(service_key.clone())
}

/// Probe whether an API proxy is actually REACHABLE — its host answers a
/// minimal HTTP request — rather than merely registered on the relay. The
/// services list uses this so a registered-but-dead API proxy (a live relay
/// registrant forwarding to an api-proxy that has died) shows unreachable
/// instead of a false "online", matching the app-server's [`app_probe`]. Opens
/// a transient tunnel, hits the proxy's local 403 fallback (no upstream model
/// call), then tears it down.
pub fn api_probe(service_key: String) -> Result<bool> {
    Ok(app_session::probe_api(service_key, &transport::resolve_blocking()?))
}

/// Health-check an app-server THIS machine hosts itself, by its loopback
/// app-listen address (e.g. `127.0.0.1:18080`) — a direct `initialize`
/// handshake with no relay hop. Unlike a bare port-open check this catches a
/// wedged / half-open codex (the listener still `accept`s but never answers
/// RPC), so a locally-hosted server that has silently stopped serving reads
/// `false` instead of a false "running". Fast because it stays on loopback.
pub fn app_probe_local(local_addr: String) -> bool {
    app_session::probe_endpoint(&local_addr)
}

/// Health-check an API proxy THIS machine hosts itself, by its loopback
/// api-listen address — a direct minimal HTTP request, no relay hop. Lets a
/// local host's API tunnel read "online" the instant its proxy is up instead
/// of waiting on a slower transient relay round-trip. Mirrors
/// [`app_probe_local`].
pub fn api_probe_local(local_addr: String) -> bool {
    app_session::probe_http_endpoint(&local_addr)
}

/// One captured runtime log line for the in-app log viewer.
pub struct LogLineDto {
    /// `TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`.
    pub level: String,
    /// Event target (crate / module path).
    pub target: String,
    /// The rendered message + any structured fields.
    pub message: String,
    /// Capture time, unix milliseconds.
    pub timestamp_ms: i64,
}

fn to_log_dto(l: logging::LogLine) -> LogLineDto {
    LogLineDto {
        level: l.level,
        target: l.target,
        message: l.message,
        timestamp_ms: l.timestamp_ms,
    }
}

/// Stream captured `tracing` events for the in-app log viewer: the retained
/// recent history (oldest first) followed by every new event live, until the
/// Dart side drops the stream. Subscribes before replaying history so nothing
/// is missed in between (a line captured right at that boundary may appear
/// twice — harmless for a log tail).
pub fn log_events(sink: StreamSink<LogLineDto>) -> Result<()> {
    runtime::runtime().spawn(async move {
        let Some(mut rx) = logging::subscribe() else {
            return;
        };
        for line in logging::snapshot() {
            if sink.add(to_log_dto(line)).is_err() {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(line) => {
                    if sink.add(to_log_dto(line)).is_err() {
                        break;
                    }
                },
                // Dropped some lines under load — keep streaming the rest.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

/// One in-flight retry of a host meta request, mirrored for Dart.
pub struct RetryProgressDto {
    /// Attempts made so far (1-based).
    pub attempt: u32,
    /// Total attempt budget before the request gives up.
    pub max_attempts: u32,
}

/// Stream retry progress for host meta requests, so the UI can show "retrying
/// 2/10" instead of appearing frozen through the backoff.
///
/// Notifications only — the request's own success or failure is still delivered
/// by whichever call the UI made. A dropped tick is harmless (a newer one
/// supersedes it), so lag is skipped rather than treated as an error.
pub fn meta_retry_events(sink: StreamSink<RetryProgressDto>) -> Result<()> {
    runtime::runtime().spawn(async move {
        let mut rx = meta::subscribe_retries();
        loop {
            match rx.recv().await {
                Ok(p) => {
                    if sink
                        .add(RetryProgressDto {
                            attempt: p.attempt,
                            max_attempts: p.max_attempts,
                        })
                        .is_err()
                    {
                        break;
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

/// Stream live app-server events (turn/item notifications) for `service_key`.
/// The Dart side receives one [`AppEventDto`] per notification until the
/// session is disconnected or the feed lags and requires history recovery.
pub fn app_events(service_key: String, sink: StreamSink<AppEventDto>) -> Result<()> {
    // Subscribe *inside* the task and always return `Ok` at setup. If the service
    // isn't connected, the task returns immediately and dropping `sink` closes the
    // Dart stream (`onDone`), which the UI's reconnect path already handles.
    // Returning the error from here instead would be worse: flutter_rust_bridge
    // delivers a stream function's setup `Err` on an *unawaited* Future, so it
    // surfaces as an uncaught async error on the Dart side — fatal on desktop
    // (no global handler) — rather than a catchable stream `onError`/`onDone`.
    runtime::runtime().spawn(async move {
        let subscribed = engine(&service_key).subscribe_events(&service_key);
        let rx = match subscribed {
            Ok(rx) => rx,
            // Not connected: close the stream so Dart sees `onDone`.
            Err(_) => return,
        };
        forward_app_events(rx, |ev| {
            sink.add(AppEventDto {
                kind: ev.kind,
                thread_id: ev.thread_id,
                item_id: ev.item_id,
                item_type: ev.item_type,
                title: ev.title,
                text: ev.text,
                images: ev.images,
                request_id: ev.request_id,
                raw: ev.raw,
            })
            .is_ok()
        })
        .await;
    });
    Ok(())
}

async fn forward_app_events(
    mut rx: tokio::sync::broadcast::Receiver<app_session::AppEvent>,
    mut send: impl FnMut(app_session::AppEvent) -> bool,
) {
    use tokio::sync::broadcast::error::RecvError;

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !send(event) {
                    break;
                }
            },
            Err(RecvError::Lagged(skipped)) => {
                // Missing final items or turn/completed cannot be recovered by
                // later deltas. Close only this feed so Dart reloads history.
                tracing::warn!(skipped, "app event feed lagged; closing for history recovery");
                forward_retained_requests(&mut rx, &mut send);
                break;
            },
            Err(RecvError::Closed) => break,
        }
    }
}

fn forward_retained_requests(
    rx: &mut tokio::sync::broadcast::Receiver<app_session::AppEvent>,
    send: &mut impl FnMut(app_session::AppEvent) -> bool,
) {
    // History cannot reconstruct request IDs. Preserve prompts in the retained
    // tail before closing, without waiting for new data or chasing new events.
    let retained = rx.len();
    for _ in 0..retained {
        match rx.try_recv() {
            Ok(event) => {
                if event.request_id.is_some() && !send(event) {
                    return;
                }
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {},
            Err(_) => break,
        }
    }
}

#[cfg(test)]
#[path = "bridge_events_tests.rs"]
mod bridge_events_tests;

/// List threads known to the app-server.
pub fn app_thread_list(service_key: String) -> Result<Vec<ThreadMetaDto>> {
    let threads = engine(&service_key).thread_list(&service_key)?;
    Ok(threads
        .into_iter()
        .map(|t| ThreadMetaDto {
            id: t.id,
            preview: t.preview,
            name: t.name,
            thread_source: t.thread_source,
            parent_thread_id: t.parent_thread_id,
            cwd: t.cwd,
            updated_at: t.updated_at,
        })
        .collect())
}

/// Inspect one thread without resuming it or taking ownership. Native Codex
/// only (callers gate on `native_metadata`); other protocols are refused
/// before any network call.
pub fn app_thread_metadata(service_key: String, thread_id: String) -> Result<ThreadMetaDto> {
    require_native(&service_key, "thread metadata")?;
    let t = app_session::thread_metadata(&service_key, &thread_id)?;
    Ok(ThreadMetaDto {
        id: t.id,
        preview: t.preview,
        name: t.name,
        thread_source: t.thread_source,
        parent_thread_id: t.parent_thread_id,
        cwd: t.cwd,
        updated_at: t.updated_at,
    })
}

/// List the models the app-server offers.
pub fn app_model_list(service_key: String) -> Result<Vec<ModelInfoDto>> {
    let models = engine(&service_key).model_list(&service_key)?;
    Ok(models
        .into_iter()
        .map(|m| ModelInfoDto {
            id: m.id,
            display_name: m.display_name,
            description: m.description,
            supported_reasoning_efforts: m.supported_reasoning_efforts,
            default_reasoning_effort: m.default_reasoning_effort,
            supported_service_tiers: m.supported_service_tiers,
            default_service_tier: m.default_service_tier,
            is_default: m.is_default,
        })
        .collect())
}

/// Send a thread realtime control request; returns the upstream JSON result.
/// Accepts the six `thread/realtime/*` methods, `thread/timeline/list`,
/// marked voice `thread/start` (and ephemeral dictation `thread/start`), and
/// `thread/unsubscribe`.
pub fn app_realtime_request(
    service_key: String,
    method: String,
    params_json: String,
) -> Result<String> {
    // Live voice and dictation are native Codex realtime features. An ACP
    // agent's audio prompt support is a different thing and is not routed here.
    require_native(&service_key, "realtime voice")?;
    app_session::realtime_request(&service_key, &method, &params_json)
}

/// Start a new thread / project. `approval_policy` is one of
/// `untrusted` / `on-failure` / `on-request` / `never`; `sandbox` is one of
/// `read-only` / `workspace-write` / `danger-full-access`. Returns the id.
pub fn app_thread_start(
    service_key: String,
    model: Option<String>,
    cwd: Option<String>,
    approval_policy: Option<String>,
    approvals_reviewer: Option<String>,
    service_tier: Option<String>,
    sandbox: Option<String>,
) -> Result<String> {
    engine(&service_key).thread_start(&service_key, StartOptions {
        model,
        cwd,
        approval_policy,
        approvals_reviewer,
        service_tier,
        sandbox,
    })
}

/// Append an asynchronous answer and return the accepted active turn ID.
pub fn app_turn_steer(
    service_key: String,
    thread_id: String,
    turn_id: Option<String>,
    text: String,
    images: Option<Vec<String>>,
) -> Result<String> {
    engine(&service_key).turn_steer(
        &service_key,
        &thread_id,
        turn_id.as_deref(),
        &text,
        &images.unwrap_or_default(),
    )
}

/// Answer a server approval request. For Codex and OpenCode `decision` is
/// the wire value the session layer recognises: `accept` or
/// `acceptForSession` to grant, any other value (e.g. `decline`) to decline.
/// For an ACP permission (`acp/permission/requested`) `decision` is the
/// agent's own option id, passed verbatim; an option the request did not
/// offer is refused and the request stays pending.
pub fn app_respond_approval(
    service_key: String,
    request_id: String,
    decision: String,
) -> Result<()> {
    engine(&service_key).respond_approval(&service_key, &request_id, &decision)
}

/// Answer an `item/tool/requestUserInput` elicitation (the model asking the
/// user structured questions, NOT a command/file approval). `answers_json` is a
/// JSON object mapping each question id to its chosen answer string(s) (option
/// labels and/or free-text), e.g. `{"theme":["山水抒怀"]}`; an empty object
/// `{}` cancels. The session layer wraps it into the protocol's
/// `ToolRequestUserInputResponse` so the model actually receives the user's
/// selections.
pub fn app_respond_user_input(
    service_key: String,
    request_id: String,
    answers_json: String,
) -> Result<()> {
    engine(&service_key).respond_user_input(&service_key, &request_id, &answers_json)
}

/// Resume an existing thread (load it into the session) before reading it or
/// sending turns; otherwise the server reports "thread not found".
pub fn app_thread_resume(service_key: String, thread_id: String) -> Result<()> {
    engine(&service_key).thread_resume(&service_key, &thread_id)
}

fn item_dto(i: app_session::ThreadItem) -> ThreadItemDto {
    ThreadItemDto {
        id: i.id,
        item_type: i.item_type,
        title: i.title,
        text: i.text,
        images: i.images,
        turn_id: i.turn_id,
        turn_completed_at: i.turn_completed_at,
        turn_duration_ms: i.turn_duration_ms,
        questions_json: i.questions_json,
    }
}

/// Read a thread's conversation items (oldest first) and whether a turn is
/// still running, so re-opening an in-flight thread restores live state.
///
/// A paginated thread returns only its newest turns' items — walk further back
/// with [`app_thread_older_page`] — plus a summary of every turn in `turns`.
/// Monitoring clients set `include_turn_pages` to false to omit cached windows
/// they already hold. Omission retains the full reopening response.
pub fn app_thread_read(
    service_key: String,
    thread_id: String,
    include_turn_pages: Option<bool>,
) -> Result<ThreadHistoryDto> {
    let h = engine(&service_key).thread_read(
        &service_key,
        &thread_id,
        include_turn_pages.unwrap_or(true),
    )?;
    Ok(history_dto(h))
}

fn history_dto(h: app_session::ThreadHistory) -> ThreadHistoryDto {
    ThreadHistoryDto {
        history_epoch: h.history_epoch,
        items: h.items.into_iter().map(item_dto).collect(),
        running: h.running,
        active_turn_id: h.active_turn_id,
        branch: h.branch,
        cwd: h.cwd,
        tokens_used: h.tokens_used,
        context_window: h.context_window,
        collaboration_mode: h.collaboration_mode,
        reasoning_effort: h.reasoning_effort,
        model: h.model,
        model_provider: h.model_provider,
        approval_policy: h.approval_policy,
        approvals_reviewer: h.approvals_reviewer,
        service_tier: h.service_tier,
        sandbox_mode: h.sandbox_mode,
        config_confirmed: h.config_confirmed,
        has_older: h.has_older,
        first_turn_id: h.first_turn_id,
        turn_pages: h.turn_pages.into_iter().map(turn_page_dto).collect(),
        turns: h
            .turns
            .into_iter()
            .map(|t| TurnSummaryDto {
                turn_id: t.turn_id,
                user_text: t.user_text,
                assistant_text: t.assistant_text,
                loaded: t.loaded,
            })
            .collect(),
    }
}

/// One page further back through a paginated thread's history.
///
/// Returns an empty page when the thread reads whole or is already at its
/// start.
pub fn app_thread_older_page(service_key: String, thread_id: String) -> Result<OlderPageDto> {
    let page = engine(&service_key).thread_older_page(&service_key, &thread_id)?;
    Ok(OlderPageDto {
        items: page.items.into_iter().map(item_dto).collect(),
        has_older: page.has_older,
    })
}

/// Every item of one turn, oldest first — for jumping to a turn the transcript
/// hasn't scrolled back to yet.
pub fn app_thread_turn_items(
    service_key: String,
    thread_id: String,
    turn_id: String,
) -> Result<Vec<ThreadItemDto>> {
    Ok(engine(&service_key)
        .thread_turn_items(&service_key, &thread_id, &turn_id)?
        .into_iter()
        .map(item_dto)
        .collect())
}

/// The latest server-reported runtime config for a thread (from its
/// start/resume response, kept fresh by live `thread/settings/updated`
/// notifications), or `None` when the server hasn't reported any. Reads the
/// engine cache only — no RPC — so the UI can poll it cheaply after a send.
#[frb(sync)]
pub fn app_thread_runtime_config(
    service_key: String,
    thread_id: String,
) -> Option<ThreadRuntimeConfigDto> {
    let config = engine(&service_key).thread_runtime_config(&service_key, &thread_id);
    config.map(|c| ThreadRuntimeConfigDto {
        model: c.model,
        model_provider: c.model_provider,
        reasoning_effort: c.reasoning_effort,
        approval_policy: c.approval_policy,
        approvals_reviewer: c.approvals_reviewer,
        service_tier: c.service_tier,
        sandbox_mode: c.sandbox_mode,
        collaboration_mode: c.collaboration_mode,
        confirmed_by_update: c.confirmed_by_update,
    })
}

/// Read the account rate-limit / quota snapshot as raw JSON (5h + weekly
/// windows). Parsed on the Dart side since the shape is nested and volatile.
pub fn app_rate_limits(service_key: String) -> Result<String> {
    require_native(&service_key, "rate limits")?;
    app_session::rate_limits(&service_key)
}

/// Unified diff of the repo at `cwd` vs its remote default branch. Empty when
/// the cwd isn't a git repo or there are no changes.
pub fn app_git_diff(service_key: String, cwd: String) -> Result<String> {
    engine(&service_key).git_diff(&service_key, &cwd)
}

/// Start a manual conversation compaction; the server emits `thread/compacted`
/// when done.
pub fn app_compact(service_key: String, thread_id: String) -> Result<()> {
    engine(&service_key).compact(&service_key, &thread_id)
}

/// A one-line gist of where a thread got to (the opening sentence of its most
/// recent agent message), or `None` when it has produced none.
///
/// Its own call rather than a field on [`app_thread_list`] because the upstream
/// `thread/list` carries no summary: the only source is a full `thread/read`,
/// so the UI fetches these lazily for the rows it actually shows instead of
/// paying for every conversation up front.
pub async fn app_thread_summary(service_key: String, thread_id: String) -> Result<Option<String>> {
    // Engines without a summary source answer `None` without any I/O.
    if !matches!(session_engine::protocol_of(&service_key), Protocol::CodexAppServer) {
        return engine(&service_key).thread_summary(&service_key, &thread_id);
    }
    // Summary RPCs must not occupy the CPU-sized FRB pool, even on one-core
    // devices. The UI separately bounds how many background requests run.
    runtime::runtime()
        .spawn_blocking(move || engine(&service_key).thread_summary(&service_key, &thread_id))
        .await?
}

/// Rename a conversation. The title is persisted by the app-server (so it
/// follows the thread across devices); an empty `name` clears it, and the UI
/// falls back to the thread preview.
pub fn app_set_thread_name(service_key: String, thread_id: String, name: String) -> Result<()> {
    engine(&service_key).set_thread_name(&service_key, &thread_id, &name)
}

/// Send a user message (text and/or attached images), starting a model turn.
/// `images` are `data:image/...;base64,...` URLs — the wire form that reaches
/// BOTH local and relay-tunneled remote app-servers (a host filesystem path
/// would not); pass an empty list for a text-only turn, and text may be empty
/// when at least one image is attached. `model` / `approval_policy` /
/// `sandbox` are optional per-turn overrides (apply to this and subsequent
/// turns) so model and permission can change mid-conversation.
/// `collaboration_mode` ("plan" / "default", or null to leave unchanged) is
/// sticky on the thread, so pass "default" to leave plan mode.
/// `reasoning_effort` ("low"/"medium"/"high", or null for the model default) is
/// the "thinking level" for this turn. The reply streams via [`app_events`];
/// this returns once the turn is accepted.
#[allow(clippy::too_many_arguments)]
pub fn app_turn_start(
    service_key: String,
    thread_id: String,
    text: String,
    images: Vec<String>,
    model: Option<String>,
    approval_policy: Option<String>,
    approvals_reviewer: Option<String>,
    service_tier: Option<String>,
    sandbox: Option<String>,
    collaboration_mode: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    engine(&service_key).turn_start(&service_key, &thread_id, TurnOptions {
        text,
        images,
        model,
        approval_policy,
        approvals_reviewer,
        service_tier,
        sandbox,
        collaboration_mode,
        reasoning_effort,
    })
}

/// Interrupt the running turn. `turn_id` (from the latest `turn/started`) is
/// required by the server; pass null only if unknown.
pub fn app_turn_interrupt(
    service_key: String,
    thread_id: String,
    turn_id: Option<String>,
) -> Result<()> {
    engine(&service_key).turn_interrupt(&service_key, &thread_id, turn_id)
}

/// What a session service's provider supports, so the shared session UI can
/// hide controls that do not apply. The first thirteen fields keep their
/// historical values for Codex and OpenCode; the rest were added for ACP and
/// are conservative (`false`) until a feature is actually available.
pub struct AppCapabilitiesDto {
    /// `codex`, `opencode` or `acp`.
    pub provider: String,
    /// Fast service tier toggle.
    pub fast: bool,
    /// Approval / sandbox permission presets.
    pub permission_presets: bool,
    /// Guardian (auto-review) approvals.
    pub guardian: bool,
    /// Account rate-limit snapshot.
    pub rate_limits: bool,
    /// Taking over a session held by another local process.
    pub takeover: bool,
    /// Monitoring another writer of the same session.
    pub external_writer_monitor: bool,
    /// Sessions read directly from this device's disk.
    pub local_sessions: bool,
    /// Plan collaboration mode.
    pub plan_mode: bool,
    /// Label of the reasoning selector: `effort` or `variant`.
    pub effort_label: String,
    /// Whether "allow for session" persists a project rule instead.
    pub approve_always_persists_project: bool,
    /// Multi-select questions.
    pub multi_select_questions: bool,
    /// Child (subagent) sessions that can be opened read-only.
    pub child_sessions: bool,
    /// Wire protocol: `codex-app-server`, `opencode-http` or `acp`.
    pub protocol: String,
    /// Human-readable agent name (ACP: the host's profile name).
    pub provider_name: String,
    /// Whether the values come from a live ACP negotiation.
    pub negotiated: bool,
    /// Host generation the values belong to (ACP; 0 otherwise). A change
    /// invalidates cached capabilities and session settings.
    pub generation: u64,
    /// Native live voice calls.
    pub voice: bool,
    /// Native composer dictation.
    pub dictation: bool,
    /// Image attachments in prompts.
    pub image_input: bool,
    /// Supplementing a running turn (`turn/steer`).
    pub steer: bool,
    /// Renaming sessions.
    pub rename: bool,
    /// Manual compaction.
    pub compact: bool,
    /// Working-tree diff.
    pub git_diff: bool,
    /// A service-wide model catalog for the model picker.
    pub model_catalog: bool,
    /// Per-session agent options (ACP select options / modes).
    pub session_config: bool,
    /// Approvals carry the agent's own options, answered by option id.
    pub permission_options: bool,
    /// Native thread metadata (Guardian pre-resume checks).
    pub native_metadata: bool,
    /// How earlier sessions reopen: `native`, `load`, `resume`, `none` or
    /// `unknown`.
    pub session_reopen: String,
    /// Source of the session list: `native`, `agent` or `host`.
    pub session_list: String,
    /// `full` or `retained` (only what the host still holds).
    pub history_scope: String,
    /// Running-session inventory: `meta`, `engine` or `none`.
    pub running_inventory: String,
    /// The agent reported that authentication is required.
    pub auth_required: bool,
}

/// Capabilities of the connection behind `service_key` (no network). Codex
/// and OpenCode are fixed; ACP reflects the host's current negotiation and
/// is conservative until connected.
#[frb(sync)]
pub fn app_capabilities(service_key: String) -> AppCapabilitiesDto {
    let c = engine(&service_key).capabilities(&service_key);
    AppCapabilitiesDto {
        provider: c.provider,
        fast: c.fast,
        permission_presets: c.permission_presets,
        guardian: c.guardian,
        rate_limits: c.rate_limits,
        takeover: c.takeover,
        external_writer_monitor: c.external_writer_monitor,
        local_sessions: c.local_sessions,
        plan_mode: c.plan_mode,
        effort_label: c.effort_label,
        approve_always_persists_project: c.approve_always_persists_project,
        multi_select_questions: c.multi_select_questions,
        child_sessions: c.child_sessions,
        protocol: c.protocol,
        provider_name: c.provider_name,
        negotiated: c.negotiated,
        generation: c.generation,
        voice: c.voice,
        dictation: c.dictation,
        image_input: c.image_input,
        steer: c.steer,
        rename: c.rename,
        compact: c.compact,
        git_diff: c.git_diff,
        model_catalog: c.model_catalog,
        session_config: c.session_config,
        permission_options: c.permission_options,
        native_metadata: c.native_metadata,
        session_reopen: c.session_reopen,
        session_list: c.session_list,
        history_scope: c.history_scope,
        running_inventory: c.running_inventory,
        auth_required: c.auth_required,
    }
}

/// Ids of sessions executing now, from the engine itself (OpenCode, ACP).
/// Codex lists them through the meta service instead.
pub fn app_running_threads(service_key: String) -> Result<Vec<String>> {
    engine(&service_key).running_threads(&service_key)
}

/// One value of an ACP select configuration option.
pub struct SessionConfigValueDto {
    /// Opaque value id, sent back verbatim.
    pub value: String,
    /// Label.
    pub name: String,
    /// Group label, when the agent groups values.
    pub group: Option<String>,
    /// Description.
    pub description: Option<String>,
}

/// One ACP select configuration option (model, mode, thought level, …).
pub struct SessionConfigOptionDto {
    /// Opaque option id.
    pub id: String,
    /// Label.
    pub name: String,
    /// Semantic category (`model`, `mode`, `thought_level`, …), UX only.
    pub category: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Current value id.
    pub current_value: String,
    /// Selectable values, in agent order.
    pub values: Vec<SessionConfigValueDto>,
}

/// One legacy ACP session mode.
pub struct SessionModeDto {
    /// Opaque mode id.
    pub id: String,
    /// Label.
    pub name: String,
    /// Description.
    pub description: Option<String>,
}

/// Per-session state an ACP agent reported. Everything is optional: agents
/// that report nothing show no controls.
pub struct SessionSettingsDto {
    /// Select configuration options.
    pub config_options: Vec<SessionConfigOptionDto>,
    /// Current legacy mode id.
    pub current_mode: Option<String>,
    /// Advertised legacy modes.
    pub modes: Vec<SessionModeDto>,
    /// Context tokens in use, when reported.
    pub usage_used: Option<i64>,
    /// Context window size, when reported.
    pub usage_size: Option<i64>,
    /// A turn is running.
    pub running: bool,
    /// Cancellation was requested and the agent has not finished yet.
    pub cancel_requested: bool,
}

fn opt_str(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn settings_dto(body: &serde_json::Value) -> SessionSettingsDto {
    let config_options = body["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|option| {
            let mut values = Vec::new();
            for entry in option["options"].as_array().into_iter().flatten() {
                if let Some(nested) = entry["options"].as_array() {
                    for value in nested {
                        values.push(SessionConfigValueDto {
                            value: value["value"].as_str().unwrap_or("").to_string(),
                            name: value["name"].as_str().unwrap_or("").to_string(),
                            group: opt_str(&entry["name"]),
                            description: opt_str(&value["description"]),
                        });
                    }
                } else {
                    values.push(SessionConfigValueDto {
                        value: entry["value"].as_str().unwrap_or("").to_string(),
                        name: entry["name"].as_str().unwrap_or("").to_string(),
                        group: None,
                        description: opt_str(&entry["description"]),
                    });
                }
            }
            SessionConfigOptionDto {
                id: option["id"].as_str().unwrap_or("").to_string(),
                name: option["name"].as_str().unwrap_or("").to_string(),
                category: opt_str(&option["category"]),
                description: opt_str(&option["description"]),
                current_value: option["currentValue"].as_str().unwrap_or("").to_string(),
                values,
            }
        })
        .collect();
    let modes = body["modes"]["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|mode| SessionModeDto {
            id: mode["id"].as_str().unwrap_or("").to_string(),
            name: mode["name"].as_str().unwrap_or("").to_string(),
            description: opt_str(&mode["description"]),
        })
        .collect();
    SessionSettingsDto {
        config_options,
        current_mode: opt_str(&body["modes"]["currentModeId"]),
        modes,
        usage_used: body["usage"]["used"].as_i64(),
        usage_size: body["usage"]["size"].as_i64(),
        running: body["runningTurnId"].is_string(),
        cancel_requested: body["cancelRequested"] == true,
    }
}

/// The ACP agent's latest per-session state as this controller saw it (no
/// network), or `None` for other protocols or unknown sessions.
#[frb(sync)]
pub fn app_session_settings(service_key: String, thread_id: String) -> Option<SessionSettingsDto> {
    engine(&service_key)
        .session_settings(&service_key, &thread_id)
        .map(|body| settings_dto(&body))
}

/// Set an ACP select configuration option to one of its advertised values.
/// The agent answers with the complete new option state.
pub fn app_set_session_config(
    service_key: String,
    thread_id: String,
    config_id: String,
    value: String,
) -> Result<()> {
    engine(&service_key).set_session_config(&service_key, &thread_id, &config_id, &value)
}

/// Switch an ACP session's legacy mode to one it advertised.
pub fn app_set_session_mode(service_key: String, thread_id: String, mode_id: String) -> Result<()> {
    engine(&service_key).set_session_mode(&service_key, &thread_id, &mode_id)
}

// ---------------------------------------------------------------------------
// Local session takeover (shared CODEX_HOME)
// ---------------------------------------------------------------------------

/// A process holding a session's rollout open (a would-be takeover
/// target), mirrored for Dart.
pub struct HolderDto {
    /// Operating-system process id.
    pub pid: i64,
    /// Process image name (e.g. `codex.exe`).
    pub name: String,
}

/// A session discovered under `CODEX_HOME`, with the state the UI needs to
/// render it read-only or resumable, mirrored for Dart.
pub struct LocalSessionDto {
    /// Thread / conversation id.
    pub thread_id: String,
    /// Working directory the session controls, when recorded.
    pub cwd: Option<String>,
    /// Best-effort first-user-message preview.
    pub preview: String,
    /// Originating client (`cli` / `vscode` / …), when recorded.
    pub source: Option<String>,
    /// Parent thread for a spawned child.
    pub parent_thread_id: Option<String>,
    /// Persisted thread classification.
    pub thread_source: Option<String>,
    /// Last-modified time of the rollout, unix seconds.
    pub updated_at: i64,
    /// Most-recent-turn state (`empty`/`completed`/`aborted`/`incomplete`).
    pub turn_state: String,
    /// Whether the rollout is currently held open by a live process.
    pub held_open: bool,
    /// Resume-safety tag (`resumable`/`resumableUnfinished`/`ownedRunning`/
    /// `ownedIdle`).
    pub safety: String,
    /// Whether the UI may offer a resume action (false only while a turn is
    /// actively running).
    pub allows_resume: bool,
    /// Whether resuming requires a force takeover (a live owner must be
    /// evicted first).
    pub requires_takeover: bool,
}

/// One session's liveness detail, including the would-be takeover targets,
/// mirrored for Dart.
pub struct SessionLivenessDto {
    /// Thread / conversation id.
    pub thread_id: String,
    /// Most-recent-turn state tag.
    pub turn_state: String,
    /// Whether the rollout is currently held open.
    pub held_open: bool,
    /// Resume-safety tag.
    pub safety: String,
    /// Whether the UI may offer a resume action.
    pub allows_resume: bool,
    /// Whether resuming requires a force takeover.
    pub requires_takeover: bool,
    /// Processes a force takeover would attempt to terminate (Pocket-Codex's
    /// own app-server already excluded).
    pub holders: Vec<HolderDto>,
}

/// One read-only history revision + ownership update from the host's live
/// session follow stream, mirrored for Dart.
pub struct SessionFollowUpdateDto {
    /// Current ownership and resume-safety state.
    pub liveness: SessionLivenessDto,
    /// Full transcript for legacy hosts; empty in metadata-only mode.
    pub items: Vec<ThreadItemDto>,
    /// Opaque revision for a metadata-only stream; read items via app-server.
    pub history_revision: Option<String>,
}

/// Outcome of a force-resume, mirrored for Dart.
pub struct ForceResumeReportDto {
    /// Holders that were successfully terminated.
    pub killed: Vec<HolderDto>,
    /// Holders the kill could not reach.
    pub survived: Vec<HolderDto>,
    /// Whether the rollout is still held open after the attempt (the resume
    /// proceeded regardless).
    pub still_held: bool,
    /// Whether the subsequent `thread/resume` succeeded.
    pub resumed: bool,
    /// The resume error message, when `resumed` is false.
    pub resume_error: Option<String>,
}

fn holder_dto(h: pocket_codex_codex::liveness::Holder) -> HolderDto {
    HolderDto {
        pid: i64::from(h.pid),
        name: h.name,
    }
}

/// List every codex session under the shared `CODEX_HOME`, newest first,
/// each annotated with whether it is safe to resume.
///
/// Works without any app-server connection — it reads the local rollout
/// files and process table directly, so it surfaces sessions created by
/// *other* codex clients (the desktop app, the CLI, the VS Code
/// extension) that share this `CODEX_HOME`. Meaningful only when the UI
/// runs on the same machine as those sessions.
pub fn app_local_sessions() -> Result<Vec<LocalSessionDto>> {
    Ok(sessions::list_local_sessions()?
        .into_iter()
        .map(|s| LocalSessionDto {
            thread_id: s.thread_id,
            cwd: s.cwd,
            preview: s.preview,
            source: s.source,
            parent_thread_id: s.parent_thread_id,
            thread_source: s.thread_source,
            updated_at: s.updated_at,
            turn_state: s.turn_state,
            held_open: s.held_open,
            safety: s.safety,
            allows_resume: s.allows_resume,
            requires_takeover: s.requires_takeover,
        })
        .collect())
}

/// Inspect one session's current resume-safety and the processes a force
/// takeover would evict. Poll this before showing a resume button so the
/// UI reflects live ownership (a session can flip between read-only and
/// resumable as the desktop app loads / releases it).
pub fn app_session_liveness(thread_id: String) -> Result<SessionLivenessDto> {
    let view = sessions::session_liveness(&thread_id)?;
    Ok(SessionLivenessDto {
        thread_id: view.thread_id,
        turn_state: view.turn_state,
        held_open: view.held_open,
        safety: view.safety,
        allows_resume: view.allows_resume,
        requires_takeover: view.requires_takeover,
        holders: view.holders.into_iter().map(holder_dto).collect(),
    })
}

/// Read a local session's transcript for READ-ONLY viewing. Parses the
/// on-disk rollout directly (no app-server connection, no resume, no write),
/// so it works even while another codex client still owns the session.
/// Items are in the same shape as [`app_thread_read`], so the read-only
/// viewer reuses the live-conversation rendering. Poll it alongside
/// [`app_session_liveness`] to follow a running session and notice when it
/// goes idle (resume-eligible).
pub fn app_local_session_transcript(thread_id: String) -> Result<Vec<ThreadItemDto>> {
    Ok(sessions::local_session_transcript(&thread_id)?
        .into_iter()
        .map(|i| ThreadItemDto {
            id: i.id,
            item_type: i.item_type,
            title: i.title,
            text: i.text,
            images: i.images,
            // A rollout file on disk has no turn envelope, so these read as
            // "unknown" rather than being reconstructed from the item order.
            turn_id: String::new(),
            turn_completed_at: None,
            turn_duration_ms: None,
            questions_json: None,
        })
        .collect())
}

/// Force-resume a session into the app-server behind `service_key`.
///
/// Best-effort terminates every live process holding the session's rollout
/// open (never Pocket-Codex's own app-server), then issues `thread/resume`
/// regardless of the eviction outcome. The UI must gate this on explicit
/// user confirmation and must not offer it while a turn is actively
/// running (`SessionLivenessDto::allows_resume == false`). The returned
/// report says exactly which processes were killed / survived and whether
/// the resume took.
pub fn app_force_resume(service_key: String, thread_id: String) -> Result<ForceResumeReportDto> {
    require_native(&service_key, "forced resume")?;
    let outcome = sessions::force_resume(&service_key, &thread_id)?;
    Ok(ForceResumeReportDto {
        killed: outcome.killed.into_iter().map(holder_dto).collect(),
        survived: outcome.survived.into_iter().map(holder_dto).collect(),
        still_held: outcome.still_held,
        resumed: outcome.resumed,
        resume_error: outcome.resume_error,
    })
}

// ---------------------------------------------------------------------------
// Remote (meta service) sessions + per-thread config
//
// The same session inventory / transcript / force-resume as the `app_local_*`
// functions above, but served by the *host's* meta service over its `meta:`
// tunnel — so a phone can view and resume a desktop host's sessions, and so
// per-thread config persists on the host and is shared across devices. Each
// takes the app-server `service_key` being viewed; the matching meta key is
// derived internally. When the host is this app, the meta service is reached
// over loopback; otherwise by subscribing to it on the relay.
// ---------------------------------------------------------------------------

fn meta_holder_dto(h: pocket_codex_host_svc::sessions::Holder) -> HolderDto {
    HolderDto {
        pid: i64::from(h.pid),
        name: h.name,
    }
}

fn meta_liveness_dto(
    value: pocket_codex_host_svc::sessions::SessionLiveness,
) -> SessionLivenessDto {
    SessionLivenessDto {
        thread_id: value.thread_id,
        turn_state: value.turn_state,
        held_open: value.held_open,
        safety: value.safety,
        allows_resume: value.allows_resume,
        requires_takeover: value.requires_takeover,
        holders: value.holders.into_iter().map(meta_holder_dto).collect(),
    }
}

fn meta_thread_item_dto(value: pocket_codex_host_svc::sessions::TranscriptItem) -> ThreadItemDto {
    ThreadItemDto {
        id: value.id,
        item_type: value.item_type,
        title: value.title,
        text: value.text,
        images: value.images,
        // A rollout file on disk has no turn envelope, so these read as
        // "unknown" rather than being reconstructed from the item order.
        turn_id: String::new(),
        turn_completed_at: None,
        turn_duration_ms: None,
        questions_json: None,
    }
}

fn meta_follow_update_dto(
    value: pocket_codex_host_svc::sessions::SessionFollowUpdate,
) -> SessionFollowUpdateDto {
    SessionFollowUpdateDto {
        liveness: meta_liveness_dto(value.liveness),
        items: value.items.into_iter().map(meta_thread_item_dto).collect(),
        history_revision: value.history_revision,
    }
}

/// Per-thread session config persisted on the host (mirrored for Dart). Every
/// field is optional: `None`/null means "no stored preference", so the UI falls
/// back to its own default.
pub struct ThreadConfigDto {
    /// Selected model id, when pinned for this thread.
    pub model: Option<String>,
    /// Reasoning-effort tag (`minimal`/`low`/`medium`/`high`), when set.
    pub reasoning_effort: Option<String>,
    /// Permission / approval mode tag, when set.
    pub permission_mode: Option<String>,
    /// Requested service tier (`priority` for Fast, `default` for standard).
    pub service_tier: Option<String>,
    /// Whether plan mode is on for this thread, when set.
    pub plan_mode: Option<bool>,
}

fn thread_config_dto(c: pocket_codex_host_svc::store::ThreadConfig) -> ThreadConfigDto {
    ThreadConfigDto {
        model: c.model,
        reasoning_effort: c.reasoning_effort,
        permission_mode: c.permission_mode,
        service_tier: c.service_tier,
        plan_mode: c.plan_mode,
    }
}

fn thread_config_from_dto(c: ThreadConfigDto) -> pocket_codex_host_svc::store::ThreadConfig {
    pocket_codex_host_svc::store::ThreadConfig {
        model: c.model,
        reasoning_effort: c.reasoning_effort,
        permission_mode: c.permission_mode,
        service_tier: c.service_tier,
        plan_mode: c.plan_mode,
    }
}

/// Remote analogue of [`app_local_sessions`]: list the sessions of the host
/// behind `service_key` via its meta tunnel (loopback when this app is the
/// host, a relay subscription when remote). Lets a phone see a desktop host's
/// sessions — including those owned by another codex client.
pub fn meta_sessions(
    service_key: String,
    running_only: Option<bool>,
) -> Result<Vec<LocalSessionDto>> {
    Ok(meta::sessions(&service_key, running_only.unwrap_or(false))?
        .into_iter()
        .map(|s| LocalSessionDto {
            thread_id: s.thread_id,
            cwd: s.cwd,
            preview: s.preview,
            source: s.source,
            parent_thread_id: s.parent_thread_id,
            thread_source: s.thread_source,
            updated_at: s.updated_at,
            turn_state: s.turn_state,
            held_open: s.held_open,
            safety: s.safety,
            allows_resume: s.allows_resume,
            requires_takeover: s.requires_takeover,
        })
        .collect())
}

/// Remote analogue of [`app_session_liveness`].
pub fn meta_session_liveness(service_key: String, thread_id: String) -> Result<SessionLivenessDto> {
    let v = meta::session_liveness(&service_key, &thread_id)?;
    Ok(meta_liveness_dto(v))
}

/// Subscribe to live read-only transcript + ownership snapshots for a session
/// held by another app-server. One long-lived meta response replaces client
/// polling; dropping the Dart stream stops forwarding.
pub fn meta_session_events(
    service_key: String,
    thread_id: String,
    sink: StreamSink<SessionFollowUpdateDto>,
) -> Result<()> {
    runtime::runtime().spawn(async move {
        let result = meta::follow_session(&service_key, &thread_id, |update| {
            sink.add(meta_follow_update_dto(update)).is_ok()
        })
        .await;
        if let Err(error) = result {
            let _ = sink.add_error(error.to_string());
        }
    });
    Ok(())
}

/// Remote analogue of [`app_local_session_transcript`].
pub fn meta_session_transcript(
    service_key: String,
    thread_id: String,
) -> Result<Vec<ThreadItemDto>> {
    Ok(meta::transcript(&service_key, &thread_id)?
        .into_iter()
        .map(meta_thread_item_dto)
        .collect())
}

/// Remote analogue of [`app_force_resume`]: the host evicts the rollout's live
/// holders and resumes it into its colocated app-server over loopback. The UI
/// must gate this on explicit confirmation and not offer it while a turn runs.
pub fn meta_force_resume(service_key: String, thread_id: String) -> Result<ForceResumeReportDto> {
    let o = meta::force_resume(&service_key, &thread_id)?;
    Ok(ForceResumeReportDto {
        killed: o.killed.into_iter().map(meta_holder_dto).collect(),
        survived: o.survived.into_iter().map(meta_holder_dto).collect(),
        still_held: o.still_held,
        resumed: o.resumed,
        resume_error: o.resume_error,
    })
}

/// Upload a document/file attachment to the host behind `service_key` (over
/// its meta tunnel — loopback when this app is the host), returning the
/// absolute HOST filesystem path where it was stored. The turn text then
/// references that path so the agent reads the file with its own tools —
/// codex's native host-file workflow (its input protocol carries only text
/// and images inline; there is no document slot).
pub fn meta_upload_file(service_key: String, file_name: String, bytes: Vec<u8>) -> Result<String> {
    Ok(meta::upload_file(&service_key, &file_name, bytes)?.path)
}

/// Capture attachment destination ownership before asynchronous file reads.
#[frb(sync)]
pub fn meta_upload_context() -> Result<String> {
    meta::upload_context()
}

/// Upload using the context captured before reading the file; reject a changed
/// account or relay without transmitting attachment bytes.
pub fn meta_upload_file_scoped(
    service_key: String,
    file_name: String,
    bytes: Vec<u8>,
    context: String,
) -> Result<String> {
    Ok(meta::upload_file_scoped(&service_key, &file_name, bytes, &context)?.path)
}

/// Read a thread's persisted config from the host behind `service_key`.
pub fn meta_thread_config_get(service_key: String, thread_id: String) -> Result<ThreadConfigDto> {
    Ok(thread_config_dto(meta::config_get(&service_key, &thread_id)?))
}

/// Persist a thread's config on the host behind `service_key`; returns the
/// stored value.
pub fn meta_thread_config_set(
    service_key: String,
    thread_id: String,
    config: ThreadConfigDto,
) -> Result<ThreadConfigDto> {
    Ok(thread_config_dto(meta::config_put(
        &service_key,
        &thread_id,
        thread_config_from_dto(config),
    )?))
}

/// The host's project-folder config (mirrored for Dart): the roots a remote
/// folder browser is confined to, and the default project new sessions open in.
pub struct ProjectConfigDto {
    /// Absolute host paths configured as project roots.
    pub project_roots: Vec<String>,
    /// Absolute host path new conversations default to (`None` = codex
    /// default).
    pub default_project: Option<String>,
}

fn project_config_dto(c: pocket_codex_host_svc::store::HostConfig) -> ProjectConfigDto {
    ProjectConfigDto {
        project_roots: c.project_roots,
        default_project: c.default_project,
    }
}

/// One browsable child directory of a host folder (mirrored for Dart).
pub struct DirEntryDto {
    /// The directory's own name (final path component).
    pub name: String,
    /// Absolute host path, ready to browse into or use as a session's cwd.
    pub path: String,
    /// Whether the directory is a git repository (a project hint).
    pub is_git_repo: bool,
}

/// One file in a host directory (mirrored for Dart), with size + mtime for the
/// file-transfer panel.
pub struct FileEntryDto {
    /// The file's own name (final path component).
    pub name: String,
    /// Absolute host path.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified time in unix seconds (0 when unavailable).
    pub mtime: i64,
}

/// Read the project-folder config of the host behind `service_key`.
pub fn meta_project_config(service_key: String) -> Result<ProjectConfigDto> {
    Ok(project_config_dto(meta::project_config(&service_key)?))
}

/// Replace the project-folder config of the host behind `service_key`; returns
/// the stored value. The desktop host edits this over its own loopback tunnel.
pub fn meta_set_project_config(
    service_key: String,
    project_roots: Vec<String>,
    default_project: Option<String>,
) -> Result<ProjectConfigDto> {
    Ok(project_config_dto(meta::set_project_config(
        &service_key,
        pocket_codex_host_svc::store::HostConfig {
            project_roots,
            default_project,
        },
    )?))
}

/// List the sub-directories of `path` on the host behind `service_key`, for the
/// remote project-folder browser. Errors (with a `403`-carrying message) if
/// `path` is outside the host's configured project roots.
pub fn meta_list_dir(service_key: String, path: String) -> Result<Vec<DirEntryDto>> {
    Ok(meta::list_dir(&service_key, &path)?
        .into_iter()
        .map(|e| DirEntryDto {
            name: e.name,
            path: e.path,
            is_git_repo: e.is_git_repo,
        })
        .collect())
}

/// List the files (not sub-directories) in `path` on the host behind
/// `service_key`, for the file-transfer panel. Errors (with a `403`-carrying
/// message) if `path` is outside the host's configured project roots.
pub fn meta_list_files(service_key: String, path: String) -> Result<Vec<FileEntryDto>> {
    Ok(meta::list_files(&service_key, &path)?
        .into_iter()
        .map(|e| FileEntryDto {
            name: e.name,
            path: e.path,
            size: e.size,
            mtime: e.mtime,
        })
        .collect())
}

/// Download a host file's raw bytes (root-confined) from the host behind
/// `service_key`, for saving to the controller's local disk. Errors (with a
/// `403`-carrying message) if `path` is outside the configured project roots.
pub fn meta_read_file(service_key: String, path: String) -> Result<Vec<u8>> {
    meta::read_file(&service_key, &path)
}

/// Bounded file preview returned after an explicit user action.
pub struct FilePreviewDto {
    /// Preview bytes, capped at 8 MiB.
    pub bytes: Vec<u8>,
    /// Total file size, including bytes omitted from the preview.
    pub total_size: u64,
}

/// Determine whether the selected host shares this app's filesystem.
pub fn meta_host_is_local(service_key: String) -> Result<bool> {
    meta::host_is_local(&service_key)
}

/// Read a bounded preview of a selected session file link.
pub fn meta_file_preview(
    service_key: String,
    thread_id: Option<String>,
    href: String,
) -> Result<FilePreviewDto> {
    let preview = meta::file_preview(&service_key, thread_id.as_deref(), &href)?;
    Ok(FilePreviewDto {
        bytes: preview.bytes,
        total_size: preview.total_size,
    })
}

/// Stream a selected file into a new controller-side staging file.
pub fn meta_file_download(
    service_key: String,
    thread_id: Option<String>,
    href: String,
    destination: String,
) -> Result<()> {
    meta::file_download(&service_key, thread_id.as_deref(), &href, &destination)
}

/// Read an image that `thread_id`'s transcript already references, so the UI
/// can render it inline instead of naming it. NOT root-confined: the host
/// authorises user attachments and typed generated artifacts, which is what
/// makes a pasted screenshot in the OS temp directory visible to a remote
/// controller without granting it a general file read. Errors (with a
/// `403`-carrying message) for a path the transcript never mentioned.
pub fn meta_read_thread_image(
    service_key: String,
    thread_id: String,
    path: String,
) -> Result<Vec<u8>> {
    meta::read_thread_image(&service_key, &thread_id, &path)
}

/// Upload local `bytes` as `file_name` into host directory `dir`
/// (root-confined) on the host behind `service_key`; returns the absolute HOST
/// path where it landed. Never overwrites an existing same-named file (a
/// collision surfaces the host's `409` in the returned message).
pub fn meta_write_file(
    service_key: String,
    dir: String,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<String> {
    Ok(meta::write_file(&service_key, &dir, &file_name, bytes)?.path)
}

// ---------------------------------------------------------------------------
// Hosted account (GitHub device-flow login)
// ---------------------------------------------------------------------------

/// A started device flow, mirrored for Dart: show the code + URL, then poll.
pub struct DeviceCodeDto {
    /// Code the user types at [`Self::verification_uri`].
    pub user_code: String,
    /// URL the user opens to enter the code.
    pub verification_uri: String,
    /// Opaque handle passed back to [`account_login_poll`].
    pub poll_handle: String,
    /// Minimum seconds between polls.
    pub interval_secs: u64,
    /// Seconds until the flow expires.
    pub expires_in_secs: u64,
    /// Resolved backend base URL to echo back to [`account_login_poll`].
    pub backend: String,
}

/// Signed-in identity mirrored for Dart.
pub struct AccountUserDto {
    /// GitHub login/handle.
    pub login: String,
    /// GitHub account id, if known.
    pub account_id: Option<String>,
}

/// One service in the account, mirrored for Dart (the `pcxu:` prefix stripped).
pub struct AccountServiceDto {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
}

/// Outcome of one device-flow poll, mirrored for Dart. `status` is one of
/// `pending` / `slow_down` / `authorized` / `expired` / `denied`; `login` is
/// set only when `authorized`.
pub struct AccountPollDto {
    /// Poll status string.
    pub status: String,
    /// Signed-in login, when `status == "authorized"`.
    pub login: Option<String>,
    /// GitHub account id, when authorized and known.
    pub account_id: Option<String>,
}

/// Begin a GitHub device-flow login. `backend` overrides the configured /
/// default backend (and is remembered on success).
pub fn account_login_start(backend: Option<String>) -> Result<DeviceCodeDto> {
    let dir = runtime::support_dir()?;
    let start = runtime::runtime().block_on(account::device_start(&dir, backend.as_deref()))?;
    Ok(DeviceCodeDto {
        user_code: start.user_code,
        verification_uri: start.verification_uri,
        poll_handle: start.poll_handle,
        interval_secs: start.interval_secs,
        expires_in_secs: start.expires_in_secs,
        backend: start.backend,
    })
}

/// Poll a device flow once. On `authorized` the session is persisted and the
/// app switches to account mode.
pub fn account_login_poll(poll_handle: String, backend: String) -> Result<AccountPollDto> {
    let dir = runtime::support_dir()?;
    let outcome = runtime::runtime().block_on(account::device_poll(&dir, &backend, poll_handle))?;
    Ok(match outcome {
        account::PollOutcome::Pending => AccountPollDto {
            status: "pending".to_string(),
            login: None,
            account_id: None,
        },
        account::PollOutcome::SlowDown => AccountPollDto {
            status: "slow_down".to_string(),
            login: None,
            account_id: None,
        },
        account::PollOutcome::Expired => AccountPollDto {
            status: "expired".to_string(),
            login: None,
            account_id: None,
        },
        account::PollOutcome::Denied => AccountPollDto {
            status: "denied".to_string(),
            login: None,
            account_id: None,
        },
        account::PollOutcome::Authorized {
            login,
            account_id,
        } => {
            // Signed in: services are now the account's.
            transport::context_changed();
            AccountPollDto {
                status: "authorized".to_string(),
                login: Some(login),
                account_id,
            }
        },
    })
}

/// A started web (authorization-code) login, mirrored for Dart. The caller
/// opens [`Self::authorize_url`] in a browser, captures the redirect to its
/// `redirect_uri`, checks the redirect's `state` equals [`Self::state`], then
/// calls [`account_web_login_exchange`] with the redirect's `exchange_code` and
/// [`Self::code_verifier`].
pub struct WebLoginStartDto {
    /// GitHub authorization URL to open in a browser.
    pub authorize_url: String,
    /// CSRF state to match against the redirect's `state`.
    pub state: String,
    /// PKCE verifier to pass back to [`account_web_login_exchange`].
    pub code_verifier: String,
    /// Resolved backend base URL to echo back to
    /// [`account_web_login_exchange`].
    pub backend: String,
}

/// Begin a web (browser-redirect) GitHub login. `redirect_uri` is the
/// platform-specific callback the browser returns to (the app's custom scheme
/// on mobile, a loopback URL on desktop). `backend` overrides the configured /
/// default backend (and is remembered on a successful exchange).
pub fn account_web_login_start(
    redirect_uri: String,
    backend: Option<String>,
) -> Result<WebLoginStartDto> {
    let dir = runtime::support_dir()?;
    let start = runtime::runtime().block_on(account::web_login_start(
        &dir,
        &redirect_uri,
        backend.as_deref(),
    ))?;
    Ok(WebLoginStartDto {
        authorize_url: start.authorize_url,
        state: start.state,
        code_verifier: start.code_verifier,
        backend: start.backend,
    })
}

/// Redeem the one-time `exchange_code` (with its PKCE `code_verifier`) from the
/// browser redirect. On success the session is persisted and the app switches
/// to account mode. Returns the signed-in identity.
pub fn account_web_login_exchange(
    exchange_code: String,
    code_verifier: String,
    backend: String,
) -> Result<AccountUserDto> {
    let dir = runtime::support_dir()?;
    let outcome = runtime::runtime().block_on(account::web_login_exchange(
        &dir,
        &backend,
        exchange_code,
        code_verifier,
    ))?;
    match outcome {
        account::PollOutcome::Authorized {
            login,
            account_id,
        } => {
            transport::context_changed();
            Ok(AccountUserDto {
                login,
                account_id,
            })
        },
        _ => Err(anyhow!("web exchange did not authorize")),
    }
}

/// The signed-in user (verified against the backend), or `None` if not signed
/// in.
pub fn account_current_user() -> Result<Option<AccountUserDto>> {
    let dir = runtime::support_dir()?;
    Ok(runtime::runtime()
        .block_on(account::current_user(&dir))?
        .map(|u| AccountUserDto {
            login: u.login,
            account_id: u.account_id,
        }))
}

/// Sign out: revoke the refresh token (best effort) and clear the local
/// session.
pub fn account_logout() -> Result<()> {
    let dir = runtime::support_dir()?;
    let result = runtime::runtime().block_on(account::logout(&dir));
    // Whatever the revocation did, this device no longer acts as the account.
    transport::context_changed();
    result
}

/// List the account's services from the backend.
pub fn account_services() -> Result<Vec<AccountServiceDto>> {
    let dir = runtime::support_dir()?;
    Ok(runtime::runtime()
        .block_on(account::services(&dir))?
        .into_iter()
        .map(|s| AccountServiceDto {
            device: s.device,
            kind: s.kind.as_key_segment().to_string(),
            name: s.name,
        })
        .collect())
}

/// Deregister one of the account's services from the relay (best-effort; a
/// still-running host re-registers shortly after). `kind` is `"app"` or
/// `"api"`.
pub fn account_deregister_service(device: String, kind: String, name: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    runtime::runtime().block_on(account::deregister_service(&dir, &device, &kind, &name))
}

/// Load a display-only persisted history without contacting a host.
pub fn app_history_cached(
    service_key: String,
    thread_id: String,
) -> Result<Option<ThreadHistoryDto>> {
    Ok(crate::engine::session_sync::cached_history(&service_key, &thread_id)?.map(history_dto))
}

/// Negotiate the independent meta history protocol before remote reads.
pub fn app_history_sync_prepare(service_key: String) -> Result<bool> {
    // The meta history protocol reads Codex rollouts; OpenCode keeps its own
    // controller cache path and ACP history is the host's retained window.
    if session_engine::protocol_of(&service_key) != Protocol::CodexAppServer {
        return Ok(false);
    }
    crate::engine::session_sync::prepare(&service_key)
}

/// Prefetch only a bounded running-session tail without resuming it.
pub fn app_history_prefetch(service_key: String, thread_id: String) -> Result<()> {
    if session_engine::protocol_of(&service_key) != Protocol::CodexAppServer {
        return Ok(());
    }
    crate::engine::session_sync::prefetch(&service_key, &thread_id)
}

/// Prioritize the current reader over background prefetch in the disk cache.
pub fn app_history_focus(service_key: String, thread_id: Option<String>) -> Result<()> {
    crate::engine::session_cache::set_focus(&service_key, thread_id.as_deref())
}

/// Controller-wide persistent cache capacity and actual disk usage.
pub struct HistoryCacheStatusDto {
    /// Shared capacity in decimal MB; zero disables persistence.
    pub limit_mb: u32,
    /// Bytes currently allocated to cache entries, including metadata.
    pub used_bytes: u64,
}

/// Inspect and enforce the shared disk quota without contacting any host.
pub fn history_cache_status() -> Result<HistoryCacheStatusDto> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    Ok(HistoryCacheStatusDto {
        limit_mb: cfg.history_cache.disk_limit_mb,
        used_bytes: crate::engine::session_cache::application_cache()?.usage()?,
    })
}

/// Set the shared cache quota, immediately evicting cold disposable windows.
pub fn history_cache_set_limit(limit_mb: u32) -> Result<()> {
    crate::engine::session_cache::set_limit(&runtime::support_dir()?, limit_mb)
}

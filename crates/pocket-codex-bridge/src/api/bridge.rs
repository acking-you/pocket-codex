//! FRB-exposed bridge surface: config, discovery, API-service subscribe,
//! and app-server remote control (sessions, threads, turns, event stream).
//! Thin glue over `crate::engine`; DTOs are plain (FRB-friendly) structs.
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use flutter_rust_bridge::frb;
use pocket_codex_core::config::Mode;

use crate::{
    engine::{account, app_session, config, discovery, meta, runtime, serve, sessions},
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
    runtime::init(PathBuf::from(support_dir))
}

fn current_relay() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    cfg.relay()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no relay configured"))
}

/// Apply the stored MSG_HEADER_KEY to this process (relay validates it).
fn apply_key() -> Result<()> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    if let Some(k) = cfg.relay_key() {
        // Guard length here so a hand-edited config.toml can't reach the
        // upstream length error (which echoes the raw key into its message).
        if k.len() != 32 {
            return Err(anyhow!("stored MSG_HEADER_KEY is not 32 bytes; re-run setup"));
        }
        pocket_codex_pb::set_msg_header_key(Some(k)).map_err(|e| anyhow!("{e}"))?;
    }
    Ok(())
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
        has_account_token: cfg.account_token().is_some(),
    })
}

/// Set the relay `host:port` and persist.
pub fn set_relay(relay: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&relay);
    config::save_config(&dir, &cfg)
}

/// Set the 32-byte MSG_HEADER_KEY and persist (validates length).
pub fn set_key(key: String) -> Result<()> {
    if key.len() != 32 {
        return Err(anyhow!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", key.len()));
    }
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay_key(&key);
    config::save_config(&dir, &cfg)
}

/// Set the UI locale (BCP-47, e.g. `en`/`zh`) and persist. An empty string
/// clears it, meaning the app follows the system locale.
pub fn set_locale(locale: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_locale(&locale);
    config::save_config(&dir, &cfg)
}

/// Import a `pcx1:` share string: decode, persist relay + key, return relay.
pub fn import_config(text: String) -> Result<String> {
    let payload = config::decode_pcx1(&text)?;
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&payload.relay);
    cfg.set_relay_key(&payload.key);
    config::save_config(&dir, &cfg)?;
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

/// Discover services: in account mode from the backend (`/v1/services`), in
/// self-host mode from the relay (applying the stored key first).
pub fn discover_services() -> Result<Vec<ServiceIdDto>> {
    let dir = runtime::support_dir()?;
    if config::load_config(&dir)?.account_mode() == Mode::Account {
        let services = runtime::runtime().block_on(account::services(&dir))?;
        return Ok(services
            .into_iter()
            .map(|s| {
                let id = s.to_service_id();
                ServiceIdDto {
                    device: id.device.clone(),
                    kind: id.kind.as_key_segment().to_string(),
                    name: id.name.clone(),
                    key: id.key(),
                }
            })
            .collect());
    }
    apply_key()?;
    let relay = current_relay()?;
    let found = runtime::runtime().block_on(discovery::discover(&relay))?;
    Ok(found
        .into_iter()
        .map(|s| ServiceIdDto {
            device: s.device,
            kind: s.kind,
            name: s.name,
            key: s.key,
        })
        .collect())
}

/// Subscribe to an API service, exposing it on `127.0.0.1:<local_port>`.
pub fn api_subscribe(service_key: String, local_port: u16) -> Result<SubStatusDto> {
    let dir = runtime::support_dir()?;
    let s = if config::load_config(&dir)?.account_mode() == Mode::Account {
        runtime::subscribe_account(service_key, local_port, &dir)?
    } else {
        apply_key()?;
        let relay = current_relay()?;
        runtime::subscribe_service(service_key, local_port, relay)?
    };
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
}

/// Start hosting a local codex app-server **and** Responses API proxy under the
/// signed-in account, publishing both `app:<name>` and `api:<name>`. Re-hosting
/// a name whose codex is still alive just re-registers any dropped tunnels.
/// `proxy` is the upstream proxy both use to reach chatgpt.com (`None` =
/// inherit env). Desktop only.
pub fn app_serve_start(
    port: u16,
    binary_override: Option<String>,
    name: Option<String>,
    proxy: Option<String>,
) -> Result<AppServeDto> {
    let r = serve::serve_start(port, binary_override, name, proxy)?;
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
}

/// A thread's recovered history + whether a turn is still running, plus the
/// metadata the status bar / git chip seed from on open.
pub struct ThreadHistoryDto {
    /// Conversation items, oldest first.
    pub items: Vec<ThreadItemDto>,
    /// Whether the most recent turn is still in progress.
    pub running: bool,
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
}

/// Connect to an app-server service: subscribe on `127.0.0.1:<local_port>`,
/// open the JSON-RPC websocket and run the `initialize` handshake. Idempotent.
pub fn app_connect(service_key: String, local_port: u16) -> Result<()> {
    let dir = runtime::support_dir()?;
    if config::load_config(&dir)?.account_mode() == Mode::Account {
        return app_session::connect_account(service_key, local_port, &dir);
    }
    apply_key()?;
    let relay = current_relay()?;
    app_session::connect(service_key, local_port, relay)
}

/// Whether a live app-server session exists for `service_key`.
#[frb(sync)]
pub fn app_is_connected(service_key: String) -> bool {
    app_session::is_connected(&service_key)
}

/// Disconnect the app-server session and its pb-mapper subscription.
pub fn app_disconnect(service_key: String) {
    app_session::disconnect(&service_key);
}

/// Probe whether an app-server is actually REACHABLE — its backend responds to
/// a handshake — rather than merely registered on the relay. The services list
/// uses this so a registered-but-dead app-server (a live relay registrant
/// forwarding to a codex app-server that has died) shows as unreachable instead
/// of a false "online". Opens a transient tunnel + `initialize` with a timeout,
/// then tears it down; a live session short-circuits to `true`.
pub fn app_probe(service_key: String) -> Result<bool> {
    let dir = runtime::support_dir()?;
    if config::load_config(&dir)?.account_mode() == Mode::Account {
        return Ok(app_session::probe_account(service_key, 0, &dir));
    }
    apply_key()?;
    let relay = current_relay()?;
    Ok(app_session::probe(service_key, 0, relay))
}

/// Probe whether an API proxy is actually REACHABLE — its host answers a
/// minimal HTTP request — rather than merely registered on the relay. The
/// services list uses this so a registered-but-dead API proxy (a live relay
/// registrant forwarding to an api-proxy that has died) shows unreachable
/// instead of a false "online", matching the app-server's [`app_probe`]. Opens
/// a transient tunnel, hits the proxy's local 403 fallback (no upstream model
/// call), then tears it down.
pub fn api_probe(service_key: String) -> Result<bool> {
    let dir = runtime::support_dir()?;
    if config::load_config(&dir)?.account_mode() == Mode::Account {
        return Ok(app_session::probe_api_account(service_key, &dir));
    }
    apply_key()?;
    let relay = current_relay()?;
    Ok(app_session::probe_api(service_key, relay))
}

/// Stream live app-server events (turn/item notifications) for `service_key`.
/// The Dart side receives one [`AppEventDto`] per notification until the
/// session is disconnected.
pub fn app_events(service_key: String, sink: StreamSink<AppEventDto>) -> Result<()> {
    // Subscribe *inside* the task and always return `Ok` at setup. If the service
    // isn't connected, the task returns immediately and dropping `sink` closes the
    // Dart stream (`onDone`), which the UI's reconnect path already handles.
    // Returning the error from here instead would be worse: flutter_rust_bridge
    // delivers a stream function's setup `Err` on an *unawaited* Future, so it
    // surfaces as an uncaught async error on the Dart side — fatal on desktop
    // (no global handler) — rather than a catchable stream `onError`/`onDone`.
    runtime::runtime().spawn(async move {
        let mut rx = match app_session::subscribe_events(&service_key) {
            Ok(rx) => rx,
            // Not connected: close the stream so Dart sees `onDone`.
            Err(_) => return,
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let dto = AppEventDto {
                        kind: ev.kind,
                        thread_id: ev.thread_id,
                        item_id: ev.item_id,
                        item_type: ev.item_type,
                        title: ev.title,
                        text: ev.text,
                        request_id: ev.request_id,
                        raw: ev.raw,
                    };
                    // Dart dropped the stream: stop forwarding.
                    if sink.add(dto).is_err() {
                        break;
                    }
                },
                // Slow consumer dropped some events; keep going.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

/// List threads known to the app-server.
pub fn app_thread_list(service_key: String) -> Result<Vec<ThreadMetaDto>> {
    Ok(app_session::thread_list(&service_key)?
        .into_iter()
        .map(|t| ThreadMetaDto {
            id: t.id,
            preview: t.preview,
            cwd: t.cwd,
            updated_at: t.updated_at,
        })
        .collect())
}

/// List the models the app-server offers.
pub fn app_model_list(service_key: String) -> Result<Vec<ModelInfoDto>> {
    Ok(app_session::model_list(&service_key)?
        .into_iter()
        .map(|m| ModelInfoDto {
            id: m.id,
            display_name: m.display_name,
            description: m.description,
            supported_reasoning_efforts: m.supported_reasoning_efforts,
            default_reasoning_effort: m.default_reasoning_effort,
        })
        .collect())
}

/// Start a new thread / project. `approval_policy` is one of
/// `untrusted` / `on-failure` / `on-request` / `never`; `sandbox` is one of
/// `read-only` / `workspace-write` / `danger-full-access`. Returns the id.
pub fn app_thread_start(
    service_key: String,
    model: Option<String>,
    cwd: Option<String>,
    approval_policy: Option<String>,
    sandbox: Option<String>,
) -> Result<String> {
    app_session::thread_start(&service_key, model, cwd, approval_policy, sandbox)
}

/// Answer a server approval request. `decision` is the wire value the session
/// layer recognises: `accept` or `acceptForSession` to grant, any other value
/// (e.g. `decline`) to decline.
pub fn app_respond_approval(
    service_key: String,
    request_id: String,
    decision: String,
) -> Result<()> {
    app_session::respond_approval(&service_key, &request_id, &decision)
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
    app_session::respond_user_input(&service_key, &request_id, &answers_json)
}

/// Resume an existing thread (load it into the session) before reading it or
/// sending turns; otherwise the server reports "thread not found".
pub fn app_thread_resume(service_key: String, thread_id: String) -> Result<()> {
    app_session::thread_resume(&service_key, &thread_id)
}

/// Read a thread's conversation items (oldest first) and whether a turn is
/// still running, so re-opening an in-flight thread restores live state.
pub fn app_thread_read(service_key: String, thread_id: String) -> Result<ThreadHistoryDto> {
    let h = app_session::thread_read(&service_key, &thread_id)?;
    Ok(ThreadHistoryDto {
        items: h
            .items
            .into_iter()
            .map(|i| ThreadItemDto {
                id: i.id,
                item_type: i.item_type,
                title: i.title,
                text: i.text,
            })
            .collect(),
        running: h.running,
        branch: h.branch,
        cwd: h.cwd,
        tokens_used: h.tokens_used,
        context_window: h.context_window,
        collaboration_mode: h.collaboration_mode,
        reasoning_effort: h.reasoning_effort,
    })
}

/// Read the account rate-limit / quota snapshot as raw JSON (5h + weekly
/// windows). Parsed on the Dart side since the shape is nested and volatile.
pub fn app_rate_limits(service_key: String) -> Result<String> {
    app_session::rate_limits(&service_key)
}

/// Unified diff of the repo at `cwd` vs its remote default branch. Empty when
/// the cwd isn't a git repo or there are no changes.
pub fn app_git_diff(service_key: String, cwd: String) -> Result<String> {
    app_session::git_diff(&service_key, &cwd)
}

/// Start a manual conversation compaction; the server emits `thread/compacted`
/// when done.
pub fn app_compact(service_key: String, thread_id: String) -> Result<()> {
    app_session::compact(&service_key, &thread_id)
}

/// Send a user message, starting a model turn. `model` / `approval_policy` /
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
    model: Option<String>,
    approval_policy: Option<String>,
    sandbox: Option<String>,
    collaboration_mode: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    app_session::turn_start(
        &service_key,
        &thread_id,
        text,
        model,
        approval_policy,
        sandbox,
        collaboration_mode,
        reasoning_effort,
    )
}

/// Interrupt the running turn. `turn_id` (from the latest `turn/started`) is
/// required by the server; pass null only if unknown.
pub fn app_turn_interrupt(
    service_key: String,
    thread_id: String,
    turn_id: Option<String>,
) -> Result<()> {
    app_session::turn_interrupt(&service_key, &thread_id, turn_id)
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
// over loopback; otherwise through the account broker.
// ---------------------------------------------------------------------------

fn meta_holder_dto(h: pocket_codex_host_svc::sessions::Holder) -> HolderDto {
    HolderDto {
        pid: i64::from(h.pid),
        name: h.name,
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
    /// Whether plan mode is on for this thread, when set.
    pub plan_mode: Option<bool>,
}

fn thread_config_dto(c: pocket_codex_host_svc::store::ThreadConfig) -> ThreadConfigDto {
    ThreadConfigDto {
        model: c.model,
        reasoning_effort: c.reasoning_effort,
        permission_mode: c.permission_mode,
        plan_mode: c.plan_mode,
    }
}

fn thread_config_from_dto(c: ThreadConfigDto) -> pocket_codex_host_svc::store::ThreadConfig {
    pocket_codex_host_svc::store::ThreadConfig {
        model: c.model,
        reasoning_effort: c.reasoning_effort,
        permission_mode: c.permission_mode,
        plan_mode: c.plan_mode,
    }
}

/// Remote analogue of [`app_local_sessions`]: list the sessions of the host
/// behind `service_key` via its meta tunnel (loopback when this app is the
/// host, broker when remote). Lets a phone see a desktop host's sessions —
/// including those owned by another codex client.
pub fn meta_sessions(service_key: String) -> Result<Vec<LocalSessionDto>> {
    Ok(meta::sessions(&service_key)?
        .into_iter()
        .map(|s| LocalSessionDto {
            thread_id: s.thread_id,
            cwd: s.cwd,
            preview: s.preview,
            source: s.source,
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
    Ok(SessionLivenessDto {
        thread_id: v.thread_id,
        turn_state: v.turn_state,
        held_open: v.held_open,
        safety: v.safety,
        allows_resume: v.allows_resume,
        requires_takeover: v.requires_takeover,
        holders: v.holders.into_iter().map(meta_holder_dto).collect(),
    })
}

/// Remote analogue of [`app_local_session_transcript`].
pub fn meta_session_transcript(
    service_key: String,
    thread_id: String,
) -> Result<Vec<ThreadItemDto>> {
    Ok(meta::transcript(&service_key, &thread_id)?
        .into_iter()
        .map(|i| ThreadItemDto {
            id: i.id,
            item_type: i.item_type,
            title: i.title,
            text: i.text,
        })
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
        } => AccountPollDto {
            status: "authorized".to_string(),
            login: Some(login),
            account_id,
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
        } => Ok(AccountUserDto {
            login,
            account_id,
        }),
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
    runtime::runtime().block_on(account::logout(&dir))
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

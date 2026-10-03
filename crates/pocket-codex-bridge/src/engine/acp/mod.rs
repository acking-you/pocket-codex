//! Controller side of the generic ACP provider (TRD §4.4.2): the `app_*`
//! bridge surface for services of kind `acp`, producing the same items,
//! histories and events as the Codex engine so the session UI is shared.
//!
//! One connection per service key speaks `_pcx`-extended ACP to the host's
//! hub over WebSocket (loopback when this process hosts it, otherwise a relay
//! tunnel). Compiled on every platform; the hosting half (`serve_acp`,
//! `acp_manage`) is desktop only.
//!
//! ```text
//!   hub ws ─► AppClient ─► event_loop ─► events::on_inbound ─► Shared (views,
//!                │                                  pending, unacked)
//!                └─ ops / history / turns (block_on) ─┘     └─► broadcast → UI
//! ```

#[cfg(test)]
mod engine_tests;
mod events;
mod history;
#[cfg(test)]
mod live_tests;
mod mapping;
mod ops;
mod state;
mod turns;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{anyhow, Result};
pub use history::{
    prefetch_history, thread_older_page, thread_read, thread_reload, thread_turn_items,
    thread_turn_page,
};
use once_cell::sync::OnceCell;
pub use ops::{
    config_options, model_list, running_sessions, set_config_option, slash_commands, thread_list,
    thread_resume, thread_start,
};
use pocket_codex_codex::client::{AppClient, Inbound};
use pocket_codex_core::{
    acp::{
        pcx::{AuthState, HubMeta},
        rpc,
    },
    service::ServiceKind,
};
use reqwest::Url;
use serde_json::{json, Value};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
pub use turns::{
    auth_authenticate, auth_state, respond_approval, respond_elicitation_url,
    respond_permission_option, respond_user_input, thread_runtime_config, turn_interrupt,
    turn_start,
};

pub use self::mapping::config_role;
use self::state::Shared;
use crate::engine::{app_session::AppEvent, runtime, serve, transport::Transport};

/// Deadline of the WebSocket handshake and of `initialize`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Capacity of the event broadcast.
const EVENT_CAPACITY: usize = 1024;
/// Upper bound of the reconnect delay.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Result of starting (or reusing) an ACP host, surfaced to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpServeReport {
    /// Device id the services registered under.
    pub device: String,
    /// Instance name.
    pub name: String,
    /// `pcx:<device>:acp:<name>`.
    pub service_key: String,
    /// Loopback WebSocket address of the hub.
    pub listen_addr: String,
    /// `pcx:<device>:meta:<name>`.
    pub meta_service_key: String,
    /// Agent id.
    pub agent_id: String,
    /// Agent display name.
    pub agent_name: String,
    /// Installed agent version (empty for custom agents).
    pub agent_version: String,
    /// Authentication state after the first detection.
    pub auth: AuthState,
    /// An existing host was reused.
    pub reused: bool,
}

/// What the session UI may offer for an ACP service (§4.4.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// A hub answered `initialize`.
    pub connected: bool,
    /// Agent display name.
    pub agent_name: String,
    /// Image prompt blocks.
    pub images: bool,
    /// Config options were seen.
    pub config_options: bool,
    /// Slash commands were seen.
    pub slash_commands: bool,
    /// Some `mode` option offers `plan`.
    pub plan_mode: bool,
}

/// Whether `service_key` names an ACP session service.
pub fn is_acp(service_key: &str) -> bool {
    let parts: Vec<&str> = service_key.split(':').collect();
    let kind = match parts.first() {
        Some(&"pcx") if parts.len() >= 4 => parts[2],
        Some(&"pcxu") if parts.len() >= 5 => parts[3],
        _ => return false,
    };
    kind == ServiceKind::Acp.as_key_segment()
}

/// First `acp.<code>` found in the error chain (T16).
pub fn pcx_code(error: &anyhow::Error) -> Option<String> {
    error
        .chain()
        .find_map(|e| rpc::pcx_code_of(&e.to_string()).map(str::to_string))
}

/// One connection's handles, shared by the event loop and the operations.
pub(super) struct Ctx {
    /// Service key.
    pub service_key: String,
    ws_url: String,
    /// Explicit meta base URL (tests); `None` resolves through `meta`.
    pub meta_url: Option<Url>,
    client: Mutex<Option<Arc<AppClient>>>,
    shared: Mutex<Shared>,
    tx: broadcast::Sender<AppEvent>,
}

impl Ctx {
    /// The live client, or an error while reconnecting.
    pub fn client(&self) -> Result<Arc<AppClient>> {
        lock(&self.client)
            .clone()
            .filter(|c| c.is_alive())
            .ok_or_else(|| anyhow!("the ACP hub connection is reconnecting; try again"))
    }

    fn alive(&self) -> bool {
        lock(&self.client).as_ref().is_some_and(|c| c.is_alive())
    }

    /// Send a request to the hub; hub errors keep their `[acp.<code>]`
    /// prefix (no added context).
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.client()?.request(method, params).await
    }

    /// Answer a hub request.
    pub async fn respond(&self, token: &str, result: Value) -> Result<()> {
        self.client()?.respond(token, result).await
    }

    /// The shared state.
    pub fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Broadcast events.
    pub fn emit(&self, events: Vec<AppEvent>) {
        for event in events {
            let _ = self.tx.send(event);
        }
    }
}

struct Conn {
    ctx: Arc<Ctx>,
    task: JoinHandle<()>,
}

fn conns() -> MutexGuard<'static, HashMap<String, Conn>> {
    static CONNS: OnceCell<Mutex<HashMap<String, Conn>>> = OnceCell::new();
    lock(CONNS.get_or_init(|| Mutex::new(HashMap::new())))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The connection of `service_key`.
pub(super) fn ctx(service_key: &str) -> Result<Arc<Ctx>> {
    conns()
        .get(service_key)
        .map(|c| c.ctx.clone())
        .ok_or_else(|| anyhow!("not connected to {service_key}"))
}

/// `initialize` params of a controller (§4.4.2).
fn initialize_params() -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false,
            "elicitation": {"form": {}, "url": {}},
            "session": {"configOptions": {"boolean": {}}}
        },
        "clientInfo": {"name": "pocket-codex-controller", "version": env!("CARGO_PKG_VERSION")}
    })
}

type Opened = (Arc<AppClient>, mpsc::UnboundedReceiver<Inbound>, Option<HubMeta>);

/// Connect and `initialize` (each step bounded by [`CONNECT_TIMEOUT`]).
async fn open(ws_url: &str) -> Result<Opened> {
    let (client, rx) = tokio::time::timeout(CONNECT_TIMEOUT, AppClient::connect(ws_url))
        .await
        .map_err(|_| anyhow!("the ACP hub did not answer in time"))??;
    let result =
        tokio::time::timeout(CONNECT_TIMEOUT, client.request("initialize", initialize_params()))
            .await
            .map_err(|_| anyhow!("the ACP hub did not finish initialize in time"))??;
    let meta = serde_json::from_value(result["_meta"]["pcx"].clone()).ok();
    Ok((Arc::new(client), rx, meta))
}

/// The hub URL: loopback when hosted here, otherwise a relay tunnel.
fn hub_url(
    service_key: &str,
    local_port: u16,
    transport: &Transport,
    transient: bool,
) -> Result<(String, Option<JoinHandle<()>>)> {
    if let Some((ws, _)) = serve::local_endpoints(service_key) {
        return Ok((format!("ws://{ws}/acp"), None));
    }
    if transient {
        let (addr, handle) =
            runtime::subscribe_transient(service_key.to_string(), local_port, transport)?;
        return Ok((format!("ws://{addr}/acp"), Some(handle)));
    }
    let sub = runtime::subscribe_service(service_key.to_string(), local_port, transport)?;
    Ok((format!("ws://{}/acp", sub.local_addr), None))
}

/// Connect (idempotent while the connection is healthy).
pub fn connect(service_key: String, local_port: u16, transport: &Transport) -> Result<()> {
    if is_connected(&service_key) {
        return Ok(());
    }
    disconnect(&service_key);
    let (url, _) = hub_url(&service_key, local_port, transport, false)?;
    connect_url(&service_key, &url, None)
}

/// Connect straight to `ws_url`, skipping `local_endpoints` and the relay
/// (T19); `meta_url` replaces the meta lookup for the default project.
pub(crate) fn connect_url(service_key: &str, ws_url: &str, meta_url: Option<Url>) -> Result<()> {
    let (client, rx, meta) = runtime::runtime().block_on(open(ws_url))?;
    let (tx, _) = broadcast::channel(EVENT_CAPACITY);
    let ctx = Arc::new(Ctx {
        service_key: service_key.to_string(),
        ws_url: ws_url.to_string(),
        meta_url,
        client: Mutex::new(Some(client)),
        shared: Mutex::new(Shared {
            meta,
            ..Shared::default()
        }),
        tx,
    });
    let task = runtime::runtime().spawn(event_loop(ctx.clone(), rx));
    if let Some(old) = conns().insert(service_key.to_string(), Conn {
        ctx: ctx.clone(),
        task,
    }) {
        old.task.abort();
    }
    ctx.emit(vec![events::capabilities_event()]);
    Ok(())
}

/// Whether a live connection exists.
pub fn is_connected(service_key: &str) -> bool {
    conns()
        .get(service_key)
        .is_some_and(|c| !c.task.is_finished() && c.ctx.alive())
}

/// Drop the connection and its relay subscription.
pub fn disconnect(service_key: &str) {
    let removed = conns().remove(service_key);
    if let Some(conn) = removed {
        conn.task.abort();
        lock(&conn.ctx.client).take();
        runtime::unsubscribe_service(service_key);
    }
}

/// Why the service is unreachable, or `None` when it answers.
pub fn probe_reason(service_key: String, transport: &Transport) -> Option<String> {
    if is_connected(&service_key) {
        return None;
    }
    let (url, handle) = match hub_url(&service_key, 0, transport, true) {
        Ok(v) => v,
        Err(e) => return Some(format!("{e:#}")),
    };
    let reason = runtime::runtime()
        .block_on(open(&url))
        .err()
        .map(|e| format!("{e:#}"));
    if let Some(handle) = handle {
        handle.abort();
    }
    reason
}

/// Subscribe to translated live events. Every new subscriber first gets
/// `acp/capabilities` and the current `acp/hub/state`, because the UI
/// subscribes only after `app_connect` returned.
pub fn subscribe_events(service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
    let ctx = ctx(service_key)?;
    let rx = ctx.tx.subscribe();
    let meta = ctx.shared().meta.clone();
    let mut replay = vec![events::capabilities_event()];
    replay.extend(meta.as_ref().map(events::hub_state_event));
    ctx.emit(replay);
    Ok(rx)
}

/// What the UI may offer for `service_key` (conservative before connect).
pub fn capabilities(service_key: &str) -> Capabilities {
    let Ok(ctx) = ctx(service_key) else { return Capabilities::default() };
    let s = ctx.shared();
    let Some(meta) = &s.meta else { return Capabilities::default() };
    let plan_mode = mapping::has_plan_mode(&meta.default_config_options)
        || s.sessions
            .values()
            .any(|v| mapping::has_plan_mode(&v.config_options));
    Capabilities {
        connected: true,
        agent_name: meta.agent.name.clone(),
        images: meta.caps.image,
        config_options: meta.caps.config_options,
        slash_commands: meta.caps.commands,
        plan_mode,
    }
}

fn handle(ctx: &Arc<Ctx>, inbound: &Inbound) {
    let outcome = events::on_inbound(&mut ctx.shared(), inbound);
    ctx.emit(outcome.events);
    for session in outcome.reattach {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Err(e) = history::attach(&ctx, &session, None, history::TAIL, false).await {
                tracing::debug!(error = %e, "re-attaching after a missed notification failed");
            }
        });
    }
}

/// Apply hub messages in order; after a disconnect, reconnect and resync.
async fn event_loop(ctx: Arc<Ctx>, mut rx: mpsc::UnboundedReceiver<Inbound>) {
    loop {
        while let Some(inbound) = rx.recv().await {
            handle(&ctx, &inbound);
        }
        rx = reconnect(&ctx).await;
        // Requests the hub re-sent during the resync are queued already
        // (the resync ends with a round trip); whatever was not re-sent was
        // resolved while we were away.
        while let Ok(inbound) = rx.try_recv() {
            handle(&ctx, &inbound);
        }
        let stale = {
            let mut s = ctx.shared();
            let current = s.conn_gen;
            let stale: Vec<(String, Option<String>)> = s
                .pending
                .iter()
                .filter(|(_, p)| p.conn_gen < current)
                .map(|(id, p)| (id.clone(), p.session.clone()))
                .collect();
            for (id, _) in &stale {
                s.pending.remove(id);
            }
            stale
        };
        ctx.emit(
            stale
                .iter()
                .map(|(id, session)| events::resolved(session.as_deref(), id))
                .collect(),
        );
    }
}

/// Reconnect with exponential backoff, then resync (§4.4.2 "重连").
async fn reconnect(ctx: &Arc<Ctx>) -> mpsc::UnboundedReceiver<Inbound> {
    let mut delay = Duration::from_secs(1);
    loop {
        tokio::time::sleep(delay).await;
        match open(&ctx.ws_url).await {
            Ok((client, rx, meta)) => {
                *lock(&ctx.client) = Some(client);
                let events = {
                    let mut s = ctx.shared();
                    s.conn_gen += 1;
                    let mut events: Vec<AppEvent> =
                        meta.iter().map(events::hub_state_event).collect();
                    if meta.is_some() {
                        s.meta = meta;
                    }
                    events.push(events::capabilities_event());
                    events
                };
                ctx.emit(events);
                resync(ctx).await;
                return rx;
            },
            Err(e) => {
                tracing::debug!(error = %format!("{e:#}"), service = %ctx.service_key, "ACP hub reconnect failed");
                delay = (delay * 2).min(MAX_BACKOFF);
            },
        }
    }
}

/// Re-attach every known session and re-submit unacknowledged prompts.
async fn resync(ctx: &Arc<Ctx>) {
    let known: Vec<(String, String, bool, String)> = ctx
        .shared()
        .sessions
        .iter()
        .map(|(id, v)| (id.clone(), v.generation.clone(), v.running, v.cwd.clone()))
        .collect();
    for (id, generation, was_running, cwd) in known {
        let cwd = Some(cwd).filter(|c| !c.is_empty());
        match history::attach(ctx, &id, cwd.as_deref(), history::TAIL, false).await {
            Ok(result) => {
                let mut out = Vec::new();
                if !generation.is_empty() && result.generation != generation {
                    out.push(crate::engine::app_events::event(
                        "acp/session/generation",
                        &id,
                        json!({"threadId": id}),
                    ));
                }
                if was_running && !result.running {
                    if let Some(last) = result.turns.last() {
                        let stop = last
                            .stop_reason
                            .clone()
                            .unwrap_or_else(|| "end_turn".into());
                        out.push(events::turn_completed(&id, last.turn, &stop, None));
                    }
                }
                ctx.emit(out);
            },
            Err(e) => {
                tracing::debug!(error = %e, session = %id, "re-attach after reconnect failed")
            },
        }
    }
    let unacked: Vec<(String, (String, Vec<pocket_codex_core::acp::ContentBlock>))> = ctx
        .shared()
        .unacked
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (key, (session, prompt)) in unacked {
        let params = json!({"sessionId": session, "prompt": prompt, "clientSubmissionId": key});
        match ctx
            .call(pocket_codex_core::acp::pcx::methods::SESSION_SUBMIT, params)
            .await
        {
            Ok(_) => {
                ctx.shared().unacked.remove(&key);
            },
            Err(e) if !ctx.alive() => {
                tracing::debug!(error = %e, "re-submit interrupted; retrying after the next reconnect");
            },
            Err(e) => {
                ctx.shared().unacked.remove(&key);
                let text = prompt
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                ctx.emit(vec![crate::engine::app_events::event(
                    "acp/queue/failed",
                    &session,
                    json!({"threadId": session, "reason": e.to_string(), "prompts": [{"text": text}]}),
                )]);
            },
        }
    }
    // A final round trip: every request the hub re-sent before answering it is
    // in our queue now.
    let _ = ctx
        .call(pocket_codex_core::acp::pcx::methods::SESSIONS_RUNNING, json!({}))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_keys_are_recognized_in_both_namespaces() {
        assert!(is_acp("pcx:mac:acp:claude"));
        assert!(is_acp("pcxu:alice:mac:acp:work"));
        assert!(!is_acp("pcx:mac:opencode:claude"));
        assert!(!is_acp("pcx:mac:app:acp"));
        assert!(!is_acp("garbage"));
    }
}

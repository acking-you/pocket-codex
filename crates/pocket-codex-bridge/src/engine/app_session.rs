//! App-server remote-control sessions.
//!
//! One [`Session`] per subscribed `pcx:*:app:*` service: it owns the
//! WebSocket JSON-RPC [`AppClient`] (already `initialize`d) and a broadcast
//! channel carrying mapped [`AppEvent`]s so multiple UI listeners (or a
//! reconnecting stream) can observe the same notification feed. The raw
//! pb-mapper subscription that materialises the local ws endpoint is owned by
//! [`crate::engine::runtime`]; we layer the JSON-RPC client on top of it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_codex::client::{AppClient, Inbound};
use serde_json::{json, Value};
use tokio::{sync::broadcast, task::JoinHandle};

use crate::engine::runtime;

/// A UI-facing app-server event, flattened from a JSON-RPC notification.
///
/// `kind` is the raw JSON-RPC method (e.g. `turn/started`,
/// `item/agentMessage/delta`, `turn/completed`); `raw` is the full params JSON
/// so the UI can stay resilient to fields we don't model explicitly.
#[derive(Clone, Debug)]
pub struct AppEvent {
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
    /// One-line human summary for tool/activity items (command, query, tool
    /// name, file count, …).
    pub title: Option<String>,
    /// Text payload: a streaming delta or an item's body/detail.
    pub text: Option<String>,
    /// Opaque token to answer a server request (e.g. an approval prompt) via
    /// [`respond_approval`]; `None` for ordinary notifications.
    pub request_id: Option<String>,
    /// Full params JSON for fields not modelled above.
    pub raw: String,
}

/// One thread's summary metadata.
#[derive(Clone, Debug)]
pub struct ThreadMeta {
    /// Thread id.
    pub id: String,
    /// Preview (usually the first user message).
    pub preview: String,
    /// Working directory (the "project" the thread controls).
    pub cwd: String,
    /// Unix seconds of last update.
    pub updated_at: i64,
}

/// One model offered by the app-server.
#[derive(Clone, Debug)]
pub struct ModelInfo {
    /// Model id used as the `model` param.
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// Short description.
    pub description: String,
    /// Reasoning efforts this model supports (`none`/`minimal`/`low`/`medium`/
    /// `high`/`xhigh`), so the UI offers only the levels the model accepts.
    pub supported_reasoning_efforts: Vec<String>,
    /// The model's default reasoning effort, if any.
    pub default_reasoning_effort: Option<String>,
}

/// One materialised conversation item (from `thread/read`).
#[derive(Clone, Debug)]
pub struct ThreadItem {
    /// Item id.
    pub id: String,
    /// Item type tag: `userMessage` / `agentMessage` / `commandExecution` /
    /// `webSearch` / `mcpToolCall` / `fileChange` / `reasoning` / `plan` / ….
    pub item_type: String,
    /// One-line summary for tool/activity items (command, query, tool name…).
    pub title: String,
    /// Body / detail text (message content, command output, tool result…).
    pub text: String,
}

struct Session {
    client: Arc<AppClient>,
    events: broadcast::Sender<AppEvent>,
    forwarder: JoinHandle<()>,
    /// Latest in-flight `turnId` per `threadId`, learned from the live
    /// `turn/started` / `turn/completed` / `turn/failed` notifications the
    /// forwarder sees. `turn/interrupt` needs the turnId, and the UI can't
    /// always supply it (e.g. opening a thread that was already running, or
    /// switching sessions), so the engine tracks it authoritatively here.
    active_turns: Arc<Mutex<HashMap<String, Value>>>,
    /// Latest reasoning effort ("thinking level") per `threadId`, captured from
    /// the `thread/resume` response (which carries a top-level
    /// `reasoningEffort`; `thread/read` does not expose it). Lets
    /// [`thread_read`] surface the effort a re-opened thread will run with
    /// so the UI can display it.
    reasoning_effort: Arc<Mutex<HashMap<String, String>>>,
    /// Requested `permissions` of any in-flight
    /// `item/permissions/requestApproval` request, keyed by its
    /// `request_id`. A permissions approval answers with a
    /// `PermissionsRequestApprovalResponse` (`{permissions, scope}`), not the
    /// plain `{decision}` a command/file approval takes, so
    /// [`respond_approval`] needs the original grant to echo back on
    /// accept. See [`track_pending_approval`].
    pending_approvals: Arc<Mutex<HashMap<String, Value>>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.forwarder.abort();
    }
}

static SESSIONS: OnceCell<Mutex<HashMap<String, Session>>> = OnceCell::new();

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Subscribe to `service_key` (materialising the local ws endpoint), open a
/// JSON-RPC client over it and run the `initialize` handshake. Idempotent: a
/// live session for the same key is reused.
pub fn connect(service_key: String, local_port: u16, relay: String) -> Result<()> {
    {
        let map = sessions().lock().expect("sessions poisoned");
        if let Some(s) = map.get(&service_key) {
            // Reuse only a *live* session. The forwarder task ends when the
            // websocket closes, so `is_finished()` means the socket is dead —
            // reusing it would make every request fail with "closed
            // connection" (the service still shows registered/online on the
            // relay, hiding the dead socket). Fall through to reconnect.
            if !s.forwarder.is_finished() {
                return Ok(());
            }
        }
    }
    // No live session: drop any stale one (and its pb-mapper subscription) so we
    // reconnect cleanly rather than reusing a closed socket.
    disconnect(&service_key);
    // Materialise the local ws endpoint via pb-mapper (kind-agnostic subscribe).
    let sub = runtime::subscribe_service(service_key.clone(), local_port, relay)?;
    let ws_url = format!("ws://{}", sub.local_addr);

    let (client, mut notify_rx) = runtime::runtime()
        .block_on(AppClient::connect(&ws_url))
        .context("connecting app-server")?;
    let client = Arc::new(client);

    // Handshake. The app-server rejects every other method until initialized.
    runtime::runtime()
        .block_on(client.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "pocket-codex",
                    "title": "Pocket-Codex",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                // `experimentalApi` unlocks v2 features the UI relies on, notably
                // `turn/start.collaborationMode` (plan mode). Without it the
                // server rejects plan turns with
                // "turn/start.collaborationMode requires experimentalApi capability".
                "capabilities": { "experimentalApi": true },
            }),
        ))
        .context("app-server initialize")?;

    let (events_tx, _) = broadcast::channel::<AppEvent>(512);
    let forward_tx = events_tx.clone();
    let active_turns: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
    let turns_for_forwarder = Arc::clone(&active_turns);
    let pending_approvals: Arc<Mutex<HashMap<String, Value>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let approvals_for_forwarder = Arc::clone(&pending_approvals);
    let forwarder = runtime::runtime().spawn(async move {
        while let Some(inbound) = notify_rx.recv().await {
            // Learn the active turnId per thread before mapping, so interrupt
            // works even when the UI never saw the turn/started event.
            track_active_turn(&turns_for_forwarder, &inbound);
            // Remember a permissions request's grant so its protocol-specific
            // response can be built when the user answers.
            track_pending_approval(&approvals_for_forwarder, &inbound);
            // Ignore send errors: no current subscribers is fine, the event
            // is simply dropped (the UI re-reads thread state on attach).
            let _ = forward_tx.send(map_event(inbound));
        }
    });

    sessions()
        .lock()
        .expect("sessions poisoned")
        .insert(service_key, Session {
            client,
            events: events_tx,
            forwarder,
            active_turns,
            reasoning_effort: Arc::new(Mutex::new(HashMap::new())),
            pending_approvals,
        });
    Ok(())
}

/// Whether a *live* session exists for `service_key` (the websocket forwarder
/// is still running). A session whose socket has closed reports `false` so the
/// UI doesn't show a dead connection as "connected".
pub fn is_connected(service_key: &str) -> bool {
    sessions()
        .lock()
        .expect("sessions poisoned")
        .get(service_key)
        .map(|s| !s.forwarder.is_finished())
        .unwrap_or(false)
}

/// Drop the session for `service_key` and its pb-mapper subscription.
pub fn disconnect(service_key: &str) {
    sessions()
        .lock()
        .expect("sessions poisoned")
        .remove(service_key);
    runtime::unsubscribe_service(service_key);
}

/// Record the active `turnId` for `thread_id` on `service_key` (no-op if the
/// session is gone). Lets [`thread_read`] seed the turn id on a cold open.
fn record_active_turn(service_key: &str, thread_id: &str, turn_id: Value) {
    if let Some(s) = sessions()
        .lock()
        .expect("sessions poisoned")
        .get(service_key)
    {
        s.active_turns
            .lock()
            .expect("active_turns poisoned")
            .insert(thread_id.to_string(), turn_id);
    }
}

/// Cache the reasoning effort `thread/resume` reported for `thread_id` (no-op
/// if the session is gone). Read back by [`thread_read`] to display current
/// effort. `None` (or empty) clears the entry, so an effort cleared on the
/// thread (e.g. by another client) isn't served stale from a previous resume.
fn record_reasoning_effort(service_key: &str, thread_id: &str, effort: Option<&str>) {
    if let Some(s) = sessions()
        .lock()
        .expect("sessions poisoned")
        .get(service_key)
    {
        let mut map = s
            .reasoning_effort
            .lock()
            .expect("reasoning_effort poisoned");
        match effort.filter(|e| !e.is_empty()) {
            Some(e) => {
                map.insert(thread_id.to_string(), e.to_string());
            },
            None => {
                map.remove(thread_id);
            },
        }
    }
}

/// The reasoning effort last seen for `thread_id` (from a prior
/// `thread/resume`).
fn cached_reasoning_effort(service_key: &str, thread_id: &str) -> Option<String> {
    sessions()
        .lock()
        .expect("sessions poisoned")
        .get(service_key)
        .and_then(|s| {
            s.reasoning_effort
                .lock()
                .expect("reasoning_effort poisoned")
                .get(thread_id)
                .cloned()
        })
}

/// A fresh broadcast receiver for `service_key`'s event feed.
pub fn subscribe_events(service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
    let map = sessions().lock().expect("sessions poisoned");
    let s = map
        .get(service_key)
        .ok_or_else(|| anyhow!("not connected to {service_key}"))?;
    Ok(s.events.subscribe())
}

fn client_for(service_key: &str) -> Result<Arc<AppClient>> {
    let map = sessions().lock().expect("sessions poisoned");
    map.get(service_key)
        .map(|s| Arc::clone(&s.client))
        .ok_or_else(|| anyhow!("not connected to {service_key}"))
}

/// List threads known to the app-server, most-recently-updated first.
///
/// The server defaults (`limit = 25`, `sortKey = created_at`) would hide older
/// threads once a user has more than 25 and would not float a recently-used but
/// older thread to the top, so request a generous page sorted by `updated_at`
/// and follow `nextCursor`. The page count and total are capped so a very large
/// history can't loop unboundedly.
pub fn thread_list(service_key: &str) -> Result<Vec<ThreadMeta>> {
    const PAGE_LIMIT: u64 = 100;
    const MAX_THREADS: usize = 500;
    let client = client_for(service_key)?;
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut params = serde_json::Map::new();
        params.insert("limit".into(), json!(PAGE_LIMIT));
        params.insert("sortKey".into(), json!("updated_at"));
        if let Some(c) = &cursor {
            params.insert("cursor".into(), json!(c));
        }
        let res =
            runtime::runtime().block_on(client.request("thread/list", Value::Object(params)))?;
        let data = res
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let page_len = data.len();
        out.extend(data.iter().filter_map(parse_thread_meta));
        cursor = res
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if cursor.is_none() || page_len == 0 || out.len() >= MAX_THREADS {
            break;
        }
    }
    Ok(out)
}

/// Parse one `thread/list` entry into [`ThreadMeta`]; skips entries with no id.
fn parse_thread_meta(t: &Value) -> Option<ThreadMeta> {
    let id = t.get("id")?.as_str()?.to_string();
    Some(ThreadMeta {
        id,
        preview: t
            .get("preview")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        cwd: t
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        updated_at: t.get("updatedAt").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// List the models the app-server offers (hidden ones filtered out).
pub fn model_list(service_key: &str) -> Result<Vec<ModelInfo>> {
    let client = client_for(service_key)?;
    let res = runtime::runtime().block_on(client.request("model/list", json!({})))?;
    let data = res
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(data
        .iter()
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            let supported_reasoning_efforts = parse_supported_efforts(m);
            let default_reasoning_effort = m
                .get("defaultReasoningEffort")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Some(ModelInfo {
                display_name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                description: m
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                supported_reasoning_efforts,
                default_reasoning_effort,
                id,
            })
        })
        .collect())
}

/// Pull a model's supported reasoning-effort ids out of
/// `supportedReasoningEfforts`.
///
/// Each entry is a `ReasoningEffortOption` object (`{reasoningEffort,
/// description}`), so read the `reasoningEffort` field; a bare string is also
/// accepted in case an older server sends the legacy shape. An empty / absent
/// list yields `vec![]`, which the UI reads as "offer all known levels".
fn parse_supported_efforts(model: &Value) -> Vec<String> {
    model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    v.get("reasoningEffort")
                        .and_then(Value::as_str)
                        .or_else(|| v.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Start a new thread with optional model / working dir / approval policy /
/// sandbox mode. `approval_policy` and `sandbox` are the wire strings
/// (`untrusted`/`on-failure`/`on-request`/`never` and
/// `read-only`/`workspace-write`/`danger-full-access`). Returns the thread id.
pub fn thread_start(
    service_key: &str,
    model: Option<String>,
    cwd: Option<String>,
    approval_policy: Option<String>,
    sandbox: Option<String>,
) -> Result<String> {
    let client = client_for(service_key)?;
    let mut params = serde_json::Map::new();
    if let Some(m) = model {
        params.insert("model".into(), json!(m));
    }
    if let Some(c) = cwd.filter(|c| !c.trim().is_empty()) {
        params.insert("cwd".into(), json!(c));
    }
    if let Some(a) = approval_policy {
        params.insert("approvalPolicy".into(), json!(a));
    }
    if let Some(s) = sandbox {
        params.insert("sandbox".into(), json!(s));
    }
    let res = runtime::runtime().block_on(client.request("thread/start", Value::Object(params)))?;
    res.get("thread")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("thread/start: missing thread id in response"))
}

/// Answer a server approval request (token from an [`AppEvent::request_id`]).
///
/// `decision` is the wire value the UI sends: `accept` / `acceptForSession` /
/// `decline`. Command-execution and file-change approvals take a plain
/// `{decision}` ([`CommandExecutionApprovalDecision`] / [`FileChangeApproval`-
/// `Decision`]). A `item/permissions/requestApproval` is different: it expects
/// a `PermissionsRequestApprovalResponse` (`{permissions, scope}`) — we echo
/// the requested grant on accept (scope `turn`, or `session` for
/// `acceptForSession`) and grant nothing on decline. Sending `{decision}` there
/// fails upstream deserialization and silently grants no permissions, so branch
/// on whether the request was a tracked permissions prompt.
pub fn respond_approval(service_key: &str, request_id: &str, decision: &str) -> Result<()> {
    let (client, pending) = {
        let map = sessions().lock().expect("sessions poisoned");
        let session = map
            .get(service_key)
            .ok_or_else(|| anyhow!("not connected to {service_key}"))?;
        let pending = session
            .pending_approvals
            .lock()
            .expect("pending_approvals poisoned")
            .remove(request_id);
        (Arc::clone(&session.client), pending)
    };
    let result = approval_result(pending, decision);
    runtime::runtime().block_on(client.respond(request_id, result))?;
    Ok(())
}

/// Build the JSON-RPC result for an approval response. `pending` is the cached
/// requested `permissions` when this was a `item/permissions/requestApproval`
/// (`None` for command / file-change approvals, which just carry the decision).
fn approval_result(pending: Option<Value>, decision: &str) -> Value {
    match pending {
        Some(permissions) => {
            // Permissions prompt → PermissionsRequestApprovalResponse. Echo the
            // requested grant on accept; an empty grant ({}) means "denied".
            let (granted, scope) = match decision {
                "accept" => (permissions, "turn"),
                "acceptForSession" => (permissions, "session"),
                _ => (json!({}), "turn"),
            };
            json!({ "permissions": granted, "scope": scope })
        },
        // Command / file-change approval → plain decision.
        None => json!({ "decision": decision }),
    }
}

/// Cache the requested `permissions` of a permissions approval request, keyed
/// by its `request_id`, so [`respond_approval`] can answer with the protocol's
/// `PermissionsRequestApprovalResponse` instead of a plain `{decision}`. Other
/// inbound messages (notifications, command/file approvals) are ignored.
fn track_pending_approval(pending: &Mutex<HashMap<String, Value>>, inbound: &Inbound) {
    if inbound.method != "item/permissions/requestApproval" {
        return;
    }
    let (Some(request_id), Some(params)) = (inbound.request_id.as_deref(), inbound.params.as_ref())
    else {
        return;
    };
    let permissions = params
        .get("permissions")
        .cloned()
        .unwrap_or_else(|| json!({}));
    pending
        .lock()
        .expect("pending_approvals poisoned")
        .insert(request_id.to_string(), permissions);
}

/// Resume an existing thread, loading it from disk into the live session.
///
/// `thread/list` enumerates persisted threads, but `thread/read` and
/// `turn/start` only see threads loaded into the server's thread manager — on
/// an unresumed thread they fail with "thread not found". Call this first when
/// opening an existing conversation.
pub fn thread_resume(service_key: &str, thread_id: &str) -> Result<()> {
    let client = client_for(service_key)?;
    let res = runtime::runtime()
        .block_on(client.request("thread/resume", json!({ "threadId": thread_id })))?;
    // The resume response carries the thread's current reasoning effort as a
    // top-level `reasoningEffort` (thread/read does NOT expose it anywhere).
    // Refresh the cache unconditionally — the server sends `reasoningEffort:
    // null` when no effort is set, which must clear any prior cached value (e.g.
    // after another client cleared it) rather than leave it stale.
    record_reasoning_effort(
        service_key,
        thread_id,
        res.get("reasoningEffort").and_then(Value::as_str),
    );
    Ok(())
}

/// A thread's recovered history plus whether a turn is still running, so the
/// UI can restore the "thinking" state when re-opening an in-flight thread.
/// Also carries the thread metadata the status bar / git chip seed from.
#[derive(Clone, Debug)]
pub struct ThreadHistory {
    /// Conversation items, oldest first.
    pub items: Vec<ThreadItem>,
    /// Whether the most recent turn is still in progress.
    pub running: bool,
    /// Current git branch of the thread's cwd, if it's a repo.
    pub branch: Option<String>,
    /// The thread's resolved working directory (for git diff / status).
    pub cwd: Option<String>,
    /// Tokens currently occupying the model context window (latest turn).
    pub tokens_used: Option<i64>,
    /// The model's context-window size in tokens.
    pub context_window: Option<i64>,
    /// The thread's sticky collaboration mode (`"plan"` / `"default"`), so the
    /// UI's plan-mode state reflects the server truth rather than guessing from
    /// the last item.
    pub collaboration_mode: Option<String>,
    /// The thread's current reasoning effort (`"low"`/`"medium"`/`"high"`), so
    /// the UI can display the "thinking level" a re-opened thread runs with.
    /// Sourced from the cached `thread/resume` response (see
    /// [`thread_resume`]).
    pub reasoning_effort: Option<String>,
}

/// Read a thread's materialised conversation items (oldest first) and whether
/// a turn is currently running.
pub fn thread_read(service_key: &str, thread_id: &str) -> Result<ThreadHistory> {
    let client = client_for(service_key)?;
    let res = runtime::runtime().block_on(
        client.request("thread/read", json!({ "threadId": thread_id, "includeTurns": true })),
    )?;
    let thread = res.get("thread");
    let turns = thread
        .and_then(|t| t.get("turns"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for turn in &turns {
        let turn_items = turn.get("items").and_then(Value::as_array);
        let Some(turn_items) = turn_items else { continue };
        for item in turn_items {
            if let Some(parsed) = parse_item(item) {
                items.push(parsed);
            }
        }
    }
    // A turn is live if the last turn's status is still in progress.
    let running = turns
        .last()
        .and_then(|t| t.get("status"))
        .and_then(Value::as_str)
        .map(|s| s == "inProgress" || s == "in_progress")
        .unwrap_or(false);
    // On a cold open the forwarder may not have seen this turn's `turn/started`,
    // so seed the active turn id from the last (running) turn for interrupt.
    if running {
        if let Some(turn_id) = turns.last().and_then(extract_turn_id) {
            record_active_turn(service_key, thread_id, turn_id);
        }
    }
    let branch = thread
        .and_then(|t| t.get("gitInfo"))
        .and_then(|g| g.get("branch"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let usage = thread.and_then(|t| t.get("tokenUsage"));
    let (tokens_used, context_window) = parse_token_usage(usage);
    let cwd = thread
        .and_then(|t| t.get("cwd"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // Current `thread/read` responses don't expose the sticky collaboration
    // mode (it's not on the thread, its status, or the turns), so this is null
    // in practice today — kept as a forward-compatible read in case a future
    // server version surfaces it. The UI falls back to its own per-thread plan
    // memory when this is absent.
    let collaboration_mode = thread
        .and_then(|t| {
            t.get("collaborationMode")
                .or_else(|| t.get("status").and_then(|s| s.get("collaborationMode")))
                .or_else(|| t.get("settings").and_then(|s| s.get("collaborationMode")))
        })
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // Reasoning effort isn't on the thread/read response either; it's captured
    // from the thread/resume response (cached). Fall back to a forward-compat
    // read of the response in case a future server version surfaces it here.
    let reasoning_effort = cached_reasoning_effort(service_key, thread_id).or_else(|| {
        thread
            .and_then(|t| t.get("reasoningEffort"))
            .or_else(|| res.get("reasoningEffort"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    Ok(ThreadHistory {
        items,
        running,
        branch,
        cwd,
        tokens_used,
        context_window,
        collaboration_mode,
        reasoning_effort,
    })
}

/// Extract `(tokens_in_context, context_window)` from a `tokenUsage` value
/// shaped `{ total, last: {totalTokens, ...}, modelContextWindow }`. Context
/// occupancy is the most recent turn's total (falling back to the cumulative
/// total); both are best-effort since the server's exact shape can drift.
pub fn parse_token_usage(usage: Option<&Value>) -> (Option<i64>, Option<i64>) {
    let Some(usage) = usage else {
        return (None, None);
    };
    let window = usage
        .get("modelContextWindow")
        .and_then(Value::as_i64)
        .filter(|w| *w > 0);
    let total_of = |k: &str| {
        usage
            .get(k)
            .and_then(|b| b.get("totalTokens"))
            .and_then(Value::as_i64)
    };
    let used = total_of("last")
        .or_else(|| total_of("total"))
        .or_else(|| usage.get("totalTokens").and_then(Value::as_i64));
    (used, window)
}

/// Read the account's rate-limit / quota snapshot (5h + weekly windows). The
/// shape is nested and volatile, so the raw JSON is returned for Dart to parse.
pub fn rate_limits(service_key: &str) -> Result<String> {
    let client = client_for(service_key)?;
    // No-params method: the server types `params` as `Option<()>` and rejects an
    // empty `{}` body, so omit `params` entirely.
    let res = runtime::runtime().block_on(client.request_no_params("account/rateLimits/read"))?;
    Ok(res.to_string())
}

/// Unified diff of the repo at `cwd` vs its remote default branch
/// (`gitDiffToRemote`). Returns the diff text, or an empty string when the cwd
/// isn't a git repo / there are no changes.
pub fn git_diff(service_key: &str, cwd: &str) -> Result<String> {
    let client = client_for(service_key)?;
    let res =
        runtime::runtime().block_on(client.request("gitDiffToRemote", json!({ "cwd": cwd })))?;
    // The diff string lives under `diff`; also accept a bare string response.
    if let Some(s) = res.as_str() {
        return Ok(s.to_string());
    }
    Ok(res
        .get("diff")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

/// Start a manual conversation compaction (`thread/compact/start`). The server
/// emits `thread/compacted` when done; the UI reloads history on that event.
pub fn compact(service_key: &str, thread_id: &str) -> Result<()> {
    let client = client_for(service_key)?;
    runtime::runtime()
        .block_on(client.request("thread/compact/start", json!({ "threadId": thread_id })))?;
    Ok(())
}

/// Send a user text message, starting a model turn. `model`, `approval_policy`
/// and `sandbox` are optional per-turn overrides (they apply to this turn *and
/// subsequent turns*, so the UI can switch model / permission
/// mid-conversation). The reply streams back as events; this returns once the
/// turn is accepted.
#[allow(clippy::too_many_arguments)]
pub fn turn_start(
    service_key: &str,
    thread_id: &str,
    text: String,
    model: Option<String>,
    approval_policy: Option<String>,
    sandbox: Option<String>,
    collaboration_mode: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    let client = client_for(service_key)?;
    let mut params = serde_json::Map::new();
    params.insert("threadId".into(), json!(thread_id));
    params.insert("input".into(), json!([{ "type": "text", "text": text }]));
    if let Some(m) = &model {
        params.insert("model".into(), json!(m));
    }
    // Reasoning effort ("thinking level") as the top-level `effort` field. This
    // applies when no collaborationMode is sent (the common case), letting the
    // user dial effort without selecting a concrete model. NOTE: the server
    // ignores this field when a collaborationMode IS sent and reads effort from
    // collaborationMode.settings.reasoning_effort instead — so the caller passes
    // the *effective* effort (current value re-asserted), and we mirror it into
    // the settings block below, ensuring a plan/permission turn never wipes it.
    // Values are the lowercase `ReasoningEffort` names (`low`/`medium`/`high`).
    if let Some(eff) = reasoning_effort.as_deref().filter(|e| !e.is_empty()) {
        params.insert("effort".into(), json!(eff));
    }
    if let Some(a) = approval_policy {
        params.insert("approvalPolicy".into(), json!(a));
    }
    // turn/start takes a structured `sandboxPolicy` (vs thread/start's plain
    // `sandbox` string); map the preset's mode to the tagged object.
    if let Some(p) = sandbox.and_then(|s| sandbox_policy(&s)) {
        params.insert("sandboxPolicy".into(), p);
    }
    // Collaboration mode ("plan" / "default") is sticky on the thread: once a
    // turn sets "plan", later turns stay in plan mode until one explicitly sends
    // "default". So the UI passes "default" to leave plan mode (implement the
    // plan), not just omits it. Either mode requires a concrete model in its
    // settings, so it's only sent when a model id is available.
    if let Some(mode) = collaboration_mode {
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            params.insert(
                "collaborationMode".into(),
                json!({
                    "mode": mode,
                    "settings": {
                        "model": m,
                        "reasoning_effort": reasoning_effort,
                        "developer_instructions": null,
                    },
                }),
            );
        }
    }
    runtime::runtime().block_on(client.request("turn/start", Value::Object(params)))?;
    Ok(())
}

/// Map a kebab sandbox mode to a `turn/start` `sandboxPolicy` tagged object.
fn sandbox_policy(mode: &str) -> Option<Value> {
    match mode {
        "read-only" => Some(json!({ "type": "readOnly" })),
        "workspace-write" => Some(json!({ "type": "workspaceWrite" })),
        "danger-full-access" => Some(json!({ "type": "dangerFullAccess" })),
        _ => None,
    }
}

/// Pull the turn id out of an object, tolerating the shapes the codex server
/// uses: `turnId` or `id` at the top level, or a nested `turn.id`. Mirrors the
/// UI's `_parseTurnId`. Preserves the JSON type (string or number) so it's
/// re-sent as-is.
fn extract_turn_id(obj: &Value) -> Option<Value> {
    let pick = |v: Option<&Value>| match v {
        Some(s @ Value::String(t)) if !t.is_empty() => Some(s.clone()),
        Some(n @ Value::Number(_)) => Some(n.clone()),
        _ => None,
    };
    pick(obj.get("turnId"))
        .or_else(|| pick(obj.get("id")))
        .or_else(|| pick(obj.get("turn").and_then(|t| t.get("id"))))
}

/// Update the per-thread active-turn map from a live notification: remember the
/// turn id on `turn/started`, forget it on `turn/completed` / `turn/failed`.
fn track_active_turn(turns: &Mutex<HashMap<String, Value>>, inbound: &Inbound) {
    let Some(params) = inbound.params.as_ref() else {
        return;
    };
    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return;
    };
    match inbound.method.as_str() {
        "turn/started" => {
            if let Some(turn_id) = extract_turn_id(params) {
                turns
                    .lock()
                    .expect("active_turns poisoned")
                    .insert(thread_id.to_string(), turn_id);
            }
        },
        "turn/completed" | "turn/failed" => {
            turns
                .lock()
                .expect("active_turns poisoned")
                .remove(thread_id);
        },
        _ => {},
    }
}

/// Interrupt the running turn. `turn/interrupt` requires the `turnId`; the UI
/// passes the one it captured from `turn/started` when it has it, but falls
/// back to the engine-tracked turnId (see [`Session::active_turns`]) so
/// stopping works for a thread that was already running when its screen opened,
/// or after switching sessions. `threadId` alone is rejected by the server.
pub fn turn_interrupt(service_key: &str, thread_id: &str, turn_id: Option<String>) -> Result<()> {
    let (client, tracked) = {
        let map = sessions().lock().expect("sessions poisoned");
        let s = map
            .get(service_key)
            .ok_or_else(|| anyhow!("not connected to {service_key}"))?;
        let tracked = s
            .active_turns
            .lock()
            .expect("active_turns poisoned")
            .get(thread_id)
            .cloned();
        (Arc::clone(&s.client), tracked)
    };
    // Prefer the UI-supplied turnId; otherwise use the engine-tracked one.
    let turn_id_value = turn_id
        .filter(|t| !t.is_empty())
        .map(Value::from)
        .or(tracked);
    let mut params = serde_json::Map::new();
    params.insert("threadId".into(), json!(thread_id));
    if let Some(t) = turn_id_value {
        params.insert("turnId".into(), t);
    }
    runtime::runtime().block_on(client.request("turn/interrupt", Value::Object(params)))?;
    Ok(())
}

/// Map an inbound server message to a flattened [`AppEvent`].
fn map_event(inbound: Inbound) -> AppEvent {
    let params = inbound.params.unwrap_or(Value::Null);
    let item = params.get("item");
    let summary = item.map(summarize_item);
    let (item_type, title) = match &summary {
        Some((t, ti, _)) => (Some(t.clone()), Some(ti.clone())),
        None => (None, None),
    };
    let text = if inbound.method.contains("failed") || inbound.method.contains("error") {
        error_message(&params).or_else(|| summary.as_ref().map(|(_, _, tx)| tx.clone()))
    } else if let Some(d) = params.get("delta").and_then(Value::as_str) {
        // Streaming delta chunk (e.g. item/agentMessage/delta).
        Some(d.to_string())
    } else {
        summary
            .as_ref()
            .map(|(_, _, tx)| tx.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                params
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    };
    // Delta events have no `item`; infer the item type from the method name.
    let item_type = item_type.or_else(|| {
        inbound
            .method
            .contains("agentMessage")
            .then(|| "agentMessage".to_string())
    });
    AppEvent {
        kind: inbound.method.clone(),
        thread_id: params
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::to_string),
        item_id: params
            .get("itemId")
            .and_then(Value::as_str)
            .or_else(|| item.and_then(|i| i.get("id")).and_then(Value::as_str))
            .map(str::to_string),
        item_type,
        title,
        text,
        request_id: inbound.request_id,
        raw: params.to_string(),
    }
}

/// Pull a human error string out of `{error:{message}}` / `{error}` /
/// `{message}`.
fn error_message(params: &Value) -> Option<String> {
    if let Some(m) = params
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
    {
        return Some(m.to_string());
    }
    if let Some(e) = params.get("error").and_then(Value::as_str) {
        return Some(e.to_string());
    }
    params
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Parse one `ThreadItem` JSON value into a [`ThreadItem`].
fn parse_item(item: &Value) -> Option<ThreadItem> {
    item.get("type").and_then(Value::as_str)?;
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (item_type, title, text) = summarize_item(item);
    Some(ThreadItem {
        id,
        item_type,
        title,
        text,
    })
}

/// Reduce a `ThreadItem` JSON value to `(type, one-line title, detail text)`
/// for the UI. Messages return an empty title (their `text` is the body); tool
/// / activity items return a human title (command, query, tool name, …) and a
/// detail body (output, args, result, paths) shown in an expandable card.
fn summarize_item(item: &Value) -> (String, String, String) {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let s = |k: &str| {
        item.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let (title, text) = match item_type.as_str() {
        "agentMessage" => (String::new(), s("text")),
        // A plan is a structured checklist (`{explanation, plan:[{step,status}]}`).
        // Encode it as `explanation` + one `- [x|~| ] step` line per step so the
        // UI can render a status-iconed checklist; fall back to plain text.
        "plan" => {
            let explanation = item
                .get("explanation")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let steps: Vec<String> = item
                .get("plan")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            let step = p.get("step").and_then(Value::as_str)?;
                            let mark = match p.get("status").and_then(Value::as_str) {
                                Some("completed") => "x",
                                Some("in_progress") => "~",
                                _ => " ",
                            };
                            Some(format!("- [{mark}] {step}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let body = if steps.is_empty() {
                if explanation.is_empty() {
                    s("text")
                } else {
                    explanation
                }
            } else if explanation.is_empty() {
                steps.join("\n")
            } else {
                format!("{explanation}\n{}", steps.join("\n"))
            };
            (String::new(), body)
        },
        "userMessage" => (
            String::new(),
            item.get("content")
                .and_then(Value::as_array)
                .map(|c| {
                    c.iter()
                        .filter_map(|p| p.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default(),
        ),
        "reasoning" => {
            let join = |k: &str| {
                item.get(k)
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            };
            let content = join("content");
            let body = if content.is_empty() { join("summary") } else { content };
            (String::new(), body)
        },
        "commandExecution" => {
            let mut detail = item
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(code) = item.get("exitCode").and_then(Value::as_i64) {
                detail = format!("{detail}\n[exit {code}]");
            }
            (s("command"), detail.trim().to_string())
        },
        "webSearch" => (s("query"), String::new()),
        "fileChange" => {
            let changes = item
                .get("changes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let paths: Vec<String> = changes
                .iter()
                .filter_map(|c| c.get("path").and_then(Value::as_str).map(str::to_string))
                .collect();
            let title = match paths.as_slice() {
                [one] => one.clone(),
                _ => format!("{} files", changes.len()),
            };
            // Each change carries its own unified `diff`; concatenate them into a
            // multi-file diff the UI can render (colored hunks + ±counts) and
            // expand for review. Fall back to the path list when no diff is
            // present (e.g. a not-yet-applied change).
            let diffs: Vec<String> = changes
                .iter()
                .filter_map(|c| {
                    c.get("diff")
                        .and_then(Value::as_str)
                        .filter(|d| !d.trim().is_empty())
                        .map(str::to_string)
                })
                .collect();
            let detail = if diffs.is_empty() { paths.join("\n") } else { diffs.join("\n") };
            (title, detail)
        },
        "mcpToolCall" => {
            let title = format!("{}.{}", s("server"), s("tool"));
            let mut detail = item
                .get("arguments")
                .map(|a| a.to_string())
                .unwrap_or_default();
            if let Some(r) = item.get("result").filter(|r| !r.is_null()) {
                detail = format!("{detail}\n\n{r}");
            }
            if let Some(e) = item.get("error").filter(|e| !e.is_null()) {
                detail = format!("{detail}\n\nerror: {e}");
            }
            (title, detail)
        },
        "dynamicToolCall" => (
            s("tool"),
            item.get("arguments")
                .map(|a| a.to_string())
                .unwrap_or_default(),
        ),
        // Unknown / other item types: best-effort text grab.
        _ => (
            String::new(),
            item.get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| recursive_text(item))
                .unwrap_or_default(),
        ),
    };
    (item_type, title, text)
}

fn recursive_text(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(t)) = m.get("text") {
                if !t.trim().is_empty() {
                    return Some(t.clone());
                }
            }
            m.values().find_map(recursive_text)
        },
        Value::Array(a) => a.iter().find_map(recursive_text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_agent_delta_text() {
        let inbound = Inbound {
            method: "item/agentMessage/delta".into(),
            params: Some(json!({"threadId":"t1","itemId":"i1","delta":"hel"})),
            request_id: None,
        };
        let ev = map_event(inbound);
        assert_eq!(ev.kind, "item/agentMessage/delta");
        assert_eq!(ev.thread_id.as_deref(), Some("t1"));
        assert_eq!(ev.item_type.as_deref(), Some("agentMessage"));
        assert_eq!(ev.text.as_deref(), Some("hel"));
    }

    #[test]
    fn summarizes_tool_items() {
        let cmd = json!({"type":"commandExecution","id":"c1","command":"ls -la",
            "aggregatedOutput":"a\nb","exitCode":0});
        let (ty, title, text) = summarize_item(&cmd);
        assert_eq!(ty, "commandExecution");
        assert_eq!(title, "ls -la");
        assert!(text.contains("exit 0"));

        let search = json!({"type":"webSearch","id":"w1","query":"rust tokio"});
        let (ty, title, _) = summarize_item(&search);
        assert_eq!(ty, "webSearch");
        assert_eq!(title, "rust tokio");

        let mcp = json!({"type":"mcpToolCall","id":"m1","server":"skills","tool":"run","arguments":{"x":1}});
        let (ty, title, detail) = summarize_item(&mcp);
        assert_eq!(ty, "mcpToolCall");
        assert_eq!(title, "skills.run");
        assert!(detail.contains("\"x\""));

        // A file change with a single path titles itself with that path and
        // exposes each change's unified `diff` as the detail (for the +/- view).
        let edit = json!({"type":"fileChange","id":"e1","changes":[
            {"path":"lib/x.dart","diff":"@@ -1 +1 @@\n-old\n+new\n","status":"completed"}]});
        let (ty, title, detail) = summarize_item(&edit);
        assert_eq!(ty, "fileChange");
        assert_eq!(title, "lib/x.dart");
        assert!(detail.contains("+new"));

        // Multiple changes: title summarises the count; diffs are concatenated.
        let edits = json!({"type":"fileChange","id":"e2","changes":[
            {"path":"a.rs","diff":"@@\n+a\n"},{"path":"b.rs","diff":"@@\n+b\n"}]});
        let (_, title, detail) = summarize_item(&edits);
        assert_eq!(title, "2 files");
        assert!(detail.contains("+a") && detail.contains("+b"));

        // No diff present (not-yet-applied): fall back to the path list.
        let pending = json!({"type":"fileChange","id":"e3","changes":[{"path":"c.rs"}]});
        let (_, _, detail) = summarize_item(&pending);
        assert_eq!(detail, "c.rs");

        // A tool item flows through map_event with item_type + title set.
        let ev = map_event(Inbound {
            method: "item/completed".into(),
            params: Some(json!({"threadId":"t1","item":search})),
            request_id: None,
        });
        assert_eq!(ev.item_type.as_deref(), Some("webSearch"));
        assert_eq!(ev.title.as_deref(), Some("rust tokio"));
        assert_eq!(ev.item_id.as_deref(), Some("w1"));
    }

    #[test]
    fn tracks_active_turn_per_thread() {
        let turns = Mutex::new(HashMap::new());
        let inbound = |method: &str, params: Value| Inbound {
            method: method.into(),
            params: Some(params),
            request_id: None,
        };
        // turn/started records the turn id from any of the shapes the server
        // uses (turnId | id | turn.id), preserving its JSON type.
        track_active_turn(
            &turns,
            &inbound("turn/started", json!({"threadId":"t1","turnId":"turn-9"})),
        );
        track_active_turn(&turns, &inbound("turn/started", json!({"threadId":"t2","id":"turn-7"})));
        track_active_turn(
            &turns,
            &inbound("turn/started", json!({"threadId":"t3","turn":{"id":"turn-3"}})),
        );
        track_active_turn(&turns, &inbound("turn/started", json!({"threadId":"t4","turnId":42})));
        assert_eq!(turns.lock().unwrap().get("t1"), Some(&json!("turn-9")));
        assert_eq!(turns.lock().unwrap().get("t2"), Some(&json!("turn-7")));
        assert_eq!(turns.lock().unwrap().get("t3"), Some(&json!("turn-3")));
        assert_eq!(turns.lock().unwrap().get("t4"), Some(&json!(42)));
        // Unrelated events don't touch the map.
        track_active_turn(&turns, &inbound("item/completed", json!({"threadId":"t1","item":{}})));
        assert_eq!(turns.lock().unwrap().get("t1"), Some(&json!("turn-9")));
        // Completion / failure clears the entry for that thread only.
        track_active_turn(&turns, &inbound("turn/completed", json!({"threadId":"t1"})));
        assert!(!turns.lock().unwrap().contains_key("t1"));
        assert_eq!(turns.lock().unwrap().get("t2"), Some(&json!("turn-7")));
        track_active_turn(&turns, &inbound("turn/failed", json!({"threadId":"t2"})));
        assert!(!turns.lock().unwrap().contains_key("t2"));
        // Other threads remain tracked.
        assert_eq!(turns.lock().unwrap().get("t3"), Some(&json!("turn-3")));
        assert_eq!(turns.lock().unwrap().get("t4"), Some(&json!(42)));
    }

    #[test]
    fn extracts_turn_failed_error_and_request_id() {
        let failed = map_event(Inbound {
            method: "turn/failed".into(),
            params: Some(json!({"threadId":"t1","error":{"message":"model overloaded"}})),
            request_id: None,
        });
        assert_eq!(failed.text.as_deref(), Some("model overloaded"));

        let approval = map_event(Inbound {
            method: "execCommandApproval".into(),
            params: Some(json!({"approvalId":"a1","command":["ls"]})),
            request_id: Some("7".into()),
        });
        assert_eq!(approval.request_id.as_deref(), Some("7"));
    }

    #[test]
    fn tracks_only_permissions_approval_requests() {
        let pending = Mutex::new(HashMap::new());
        // A permissions request caches its requested grant under the request id.
        track_pending_approval(&pending, &Inbound {
            method: "item/permissions/requestApproval".into(),
            params: Some(json!({"permissions":{"network":{"enabled":true}}})),
            request_id: Some("11".into()),
        });
        assert_eq!(pending.lock().unwrap().get("11"), Some(&json!({"network":{"enabled":true}})));
        // Command / file-change approvals and plain notifications are ignored —
        // they answer with a plain {decision}, no cached grant needed.
        track_pending_approval(&pending, &Inbound {
            method: "item/commandExecution/requestApproval".into(),
            params: Some(json!({"command":"ls"})),
            request_id: Some("12".into()),
        });
        assert!(!pending.lock().unwrap().contains_key("12"));
    }

    #[test]
    fn permissions_response_echoes_grant_with_scope() {
        let grant = json!({"network":{"enabled":true},"fileSystem":{"read":["/x"]}});
        // Accept → echo the requested grant, turn scope.
        assert_eq!(
            approval_result(Some(grant.clone()), "accept"),
            json!({"permissions": grant, "scope": "turn"})
        );
        // Accept for session → same grant, session scope.
        assert_eq!(
            approval_result(Some(grant.clone()), "acceptForSession"),
            json!({"permissions": grant, "scope": "session"})
        );
        // Decline → grant nothing (empty profile), turn scope.
        assert_eq!(
            approval_result(Some(grant), "decline"),
            json!({"permissions": {}, "scope": "turn"})
        );
    }

    #[test]
    fn non_permissions_response_is_a_plain_decision() {
        // No cached grant ⇒ command / file-change approval ⇒ {decision}.
        assert_eq!(approval_result(None, "accept"), json!({"decision": "accept"}));
        assert_eq!(approval_result(None, "decline"), json!({"decision": "decline"}));
    }

    #[test]
    fn parses_supported_reasoning_efforts() {
        // Real protocol shape: array of {reasoningEffort, description} objects.
        let model = json!({"supportedReasoningEfforts": [
            {"reasoningEffort": "low", "description": "fast"},
            {"reasoningEffort": "high", "description": "thorough"},
        ]});
        assert_eq!(parse_supported_efforts(&model), vec!["low", "high"]);
        // Legacy bare-string shape still parses (backward compatibility).
        let legacy = json!({"supportedReasoningEfforts": ["minimal", "xhigh"]});
        assert_eq!(parse_supported_efforts(&legacy), vec!["minimal", "xhigh"]);
        // Absent / empty → empty (the UI reads that as "offer all levels").
        assert!(parse_supported_efforts(&json!({})).is_empty());
    }

    #[test]
    fn parses_thread_meta_and_skips_idless_entries() {
        let t = json!({"id":"t1","preview":"hi","cwd":"/repo","updatedAt":42});
        let meta = parse_thread_meta(&t).expect("entry has an id");
        assert_eq!(meta.id, "t1");
        assert_eq!(meta.preview, "hi");
        assert_eq!(meta.cwd, "/repo");
        assert_eq!(meta.updated_at, 42);
        // An entry with no id is skipped (filter_map drops it).
        assert!(parse_thread_meta(&json!({"preview":"x"})).is_none());
    }

    #[test]
    fn parses_user_and_agent_items() {
        let user =
            json!({"type":"userMessage","id":"u1","content":[{"type":"text","text":"hi there"}]});
        let agent = json!({"type":"agentMessage","id":"a1","text":"hello"});
        assert_eq!(parse_item(&user).unwrap().item_type, "userMessage");
        assert_eq!(parse_item(&user).unwrap().text, "hi there");
        assert_eq!(parse_item(&agent).unwrap().item_type, "agentMessage");
        assert_eq!(parse_item(&agent).unwrap().text, "hello");
    }
}

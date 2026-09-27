//! OpenCode session state, independent of the Codex JSON-RPC controller.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{ensure, Context, Result};
use futures::StreamExt;
use once_cell::sync::OnceCell;
use pocket_codex_host_svc::opencode::{
    connection::{
        Connection, EventStream, NativeEvent, NativeMessage, NativePermission, NativeQuestion,
    },
    PermissionReply, PromptInput,
};
use tokio::sync::Mutex;

/// An authoritative native message window for the selected session.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Upstream session identity.
    pub session_id: String,
    /// Native messages, ordered from oldest to newest.
    pub messages: Vec<NativeMessage>,
    /// Cursor for the next earlier page, when available.
    pub next_cursor: Option<String>,
    /// Monotonic local view revision.
    pub revision: u64,
    /// Authoritative upstream execution state: idle, busy, or retry.
    pub status: String,
    /// Live permission requests for this session only.
    pub permissions: Vec<NativePermission>,
    /// Live question requests for this session only.
    pub questions: Vec<NativeQuestion>,
}

#[derive(Default)]
struct State {
    selection: u64,
    snapshot: Option<Snapshot>,
    live: BTreeMap<(String, String, usize), LiveText>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
}

#[derive(Default)]
struct LiveText {
    text: String,
    complete: bool,
}

struct ConnectionEntry {
    controller: Arc<OpenCodeController>,
    relay_key: Option<String>,
    event_tasks: Vec<tokio::task::JoinHandle<()>>,
}

static CONNECTIONS: OnceCell<std::sync::Mutex<HashMap<String, ConnectionEntry>>> = OnceCell::new();

const EVENT_SNAPSHOT_WINDOW: Duration = Duration::from_millis(50);

/// Consume one fixed event window so a high-frequency OpenCode stream produces
/// at most one authoritative snapshot refresh per window.
pub(crate) async fn drain_event_burst(
    events: &mut EventStream,
) -> Result<Option<Vec<NativeEvent>>> {
    let Some(event) = events.next().await else {
        return Ok(None);
    };
    let event = event?;
    let mut burst = Vec::new();
    if !matches!(event.kind(), "server.connected" | "server.heartbeat") {
        burst.push(event);
    }
    let deadline = Instant::now() + EVENT_SNAPSHOT_WINDOW;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(Some(burst));
        }
        match tokio::time::timeout(remaining, events.next()).await {
            Ok(Some(Ok(event))) => {
                if !matches!(event.kind(), "server.connected" | "server.heartbeat") {
                    burst.push(event);
                }
                if burst.len() >= 4096 {
                    return Ok(Some(burst));
                }
            },
            Ok(Some(Err(error))) => return Err(error.into()),
            Ok(None) | Err(_) => return Ok(Some(burst)),
        }
    }
}

fn connections() -> &'static std::sync::Mutex<HashMap<String, ConnectionEntry>> {
    CONNECTIONS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn lock_connections() -> std::sync::MutexGuard<'static, HashMap<String, ConnectionEntry>> {
    match connections().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("OpenCode connection registry lock was poisoned; recovering state");
            poisoned.into_inner()
        },
    }
}

/// One connection's selected conversation and bounded history.
pub struct OpenCodeController {
    client: Connection,
    state: Mutex<State>,
}

impl OpenCodeController {
    /// Construct a read-only controller without taking ownership of the server.
    pub fn new(client: impl Into<Connection>) -> Self {
        Self {
            client: client.into(),
            state: Mutex::new(State::default()),
        }
    }

    /// List sessions in the selected directory, bounded by the upstream API.
    pub async fn sessions(
        &self,
        search: Option<&str>,
    ) -> Result<Vec<pocket_codex_host_svc::opencode::Session>> {
        Ok(self.client.sessions(search).await?)
    }

    /// Create an empty session and select it without sending a prompt.
    pub async fn create(&self, title: Option<&str>) -> Result<Snapshot> {
        let session = self.client.create(title).await?;
        self.open_session(&session.id).await
    }

    /// Return the currently selected session ID, if any.
    pub async fn selected_session(&self) -> Option<String> {
        self.state
            .lock()
            .await
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_id.clone())
    }

    /// Select a session and read its bounded tail. A newer selection wins over
    /// any response already in flight for the previous session.
    pub async fn open_session(&self, session_id: &str) -> Result<Snapshot> {
        let selection = {
            let mut state = self.state.lock().await;
            state.selection += 1;
            state.live.clear();
            state.seen.clear();
            state.seen_order.clear();
            state.selection
        };
        let page = self.client.history(session_id, 20, None).await?;
        let statuses = self.client.status().await?;
        let status = match statuses.get(session_id) {
            Some(status) => status["type"]
                .as_str()
                .context("invalid OpenCode execution status")?,
            None => "idle",
        };
        ensure!(
            matches!(status, "idle" | "busy" | "retry"),
            "unsupported OpenCode execution status"
        );
        let permissions = self
            .client
            .permissions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let questions = self
            .client
            .questions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let mut state = self.state.lock().await;
        ensure!(state.selection == selection, "OpenCode session selection changed");
        let snapshot = Snapshot {
            session_id: session_id.into(),
            messages: page.messages,
            next_cursor: page.next_cursor,
            revision: selection,
            status: status.into(),
            permissions,
            questions,
        };
        state.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// Read the last accepted snapshot without contacting the upstream server.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.state
            .lock()
            .await
            .snapshot
            .clone()
            .context("no OpenCode session selected")
    }

    /// Reconcile a native event burst without dropping retained older history.
    pub async fn refresh(&self, session_id: &str, events: Vec<NativeEvent>) -> Result<Snapshot> {
        let (selection, retained) = {
            let state = self.state.lock().await;
            (
                state.selection,
                state
                    .snapshot
                    .clone()
                    .context("no OpenCode session selected")?,
            )
        };
        ensure!(retained.session_id == session_id, "OpenCode session selection changed");
        let mut page = self.client.history(session_id, 20, None).await?;
        let mut reads = 1;
        while !retained.messages.is_empty()
            && !page.messages.is_empty()
            && !page
                .messages
                .iter()
                .any(|message| retained.messages.iter().any(|old| old.id() == message.id()))
        {
            let Some(cursor) = page.next_cursor.clone() else {
                break;
            };
            ensure!(reads < 5, "OpenCode refresh gap exceeds the bounded window; reopen required");
            let older = self.client.history(session_id, 20, Some(&cursor)).await?;
            ensure!(
                older.next_cursor.as_deref() != Some(&cursor),
                "OpenCode history cursor did not advance"
            );
            let mut messages = older.messages;
            messages.extend(page.messages);
            page.messages = messages;
            page.next_cursor = older.next_cursor;
            reads += 1;
        }
        let statuses = self.client.status().await?;
        let status = statuses
            .get(session_id)
            .and_then(|status| status["type"].as_str())
            .unwrap_or("idle");
        ensure!(
            matches!(status, "idle" | "busy" | "retry"),
            "unsupported OpenCode execution status"
        );
        let permissions = self
            .client
            .permissions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let questions = self
            .client
            .questions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let mut state = self.state.lock().await;
        ensure!(state.selection == selection, "OpenCode session selection changed");
        let mut current = state
            .snapshot
            .clone()
            .context("no OpenCode session selected")?;
        for message in page.messages {
            match current
                .messages
                .iter()
                .position(|existing| existing.id() == message.id())
            {
                Some(index) => current.messages[index] = message,
                None => current.messages.push(message),
            }
        }
        ensure!(
            current.messages.len() <= 2048,
            "OpenCode retained history exceeds the resource limit"
        );
        for event in events {
            match event {
                NativeEvent::V1(event)
                    if event.kind == "message.removed"
                        && event.properties["sessionID"].as_str() == Some(session_id) =>
                {
                    if let Some(id) = event.properties["messageID"].as_str() {
                        current.messages.retain(|message| message.id() != id);
                    }
                },
                NativeEvent::V2(event) if event.data["sessionID"].as_str() == Some(session_id) => {
                    record_live_text(&mut state, event)?
                },
                _ => {},
            }
        }
        apply_live_text(&mut current.messages, &state.live);
        ensure!(
            serde_json::to_vec(&current.messages)?.len() <= 32 * 1024 * 1024,
            "OpenCode retained history exceeds the resource limit"
        );
        current.status = status.into();
        current.permissions = permissions;
        current.questions = questions;
        current.revision += 1;
        state.snapshot = Some(current.clone());
        Ok(current)
    }

    /// Prepend one earlier page. Failed reads retain the last confirmed cursor
    /// so only an explicit retry can advance it.
    pub async fn older(&self, session_id: &str) -> Result<Snapshot> {
        let (selection, retained) = {
            let state = self.state.lock().await;
            (
                state.selection,
                state
                    .snapshot
                    .clone()
                    .context("no OpenCode session selected")?,
            )
        };
        ensure!(retained.session_id == session_id, "OpenCode session selection changed");
        let Some(cursor) = retained.next_cursor.as_deref() else { return Ok(retained) };
        let page = self.client.history(session_id, 20, Some(cursor)).await?;
        ensure!(
            page.next_cursor.as_deref() != Some(cursor),
            "OpenCode history cursor did not advance"
        );
        let mut state = self.state.lock().await;
        ensure!(state.selection == selection, "OpenCode session selection changed");
        let current = state
            .snapshot
            .as_mut()
            .context("no OpenCode session selected")?;
        if current.next_cursor != retained.next_cursor {
            return Ok(current.clone());
        }
        let mut messages: Vec<_> = page
            .messages
            .into_iter()
            .filter(|message| {
                !current
                    .messages
                    .iter()
                    .any(|existing| existing.id() == message.id())
            })
            .collect();
        messages.extend(current.messages.iter().cloned());
        ensure!(
            messages.len() <= 2048 && serde_json::to_vec(&messages)?.len() <= 32 * 1024 * 1024,
            "OpenCode retained history exceeds the resource limit"
        );
        current.messages = messages;
        current.next_cursor = page.next_cursor;
        current.revision += 1;
        Ok(current.clone())
    }

    /// Submit a text continuation for the selected session.
    ///
    /// OpenCode's `204` response means only that the request was accepted. A
    /// transport failure is returned as an error and callers must reconcile
    /// history before offering a retry; this method never retries implicitly.
    pub async fn send(&self, session_id: &str, text: &str) -> Result<()> {
        let selected = self.state.lock().await.snapshot.clone();
        ensure!(
            selected
                .as_ref()
                .is_some_and(|snapshot| snapshot.session_id == session_id),
            "OpenCode session is not selected"
        );
        self.client
            .prompt(session_id, &PromptInput::text(text, None))
            .await?;
        Ok(())
    }

    /// Answer one currently pending permission request.
    pub async fn reply_permission(
        &self,
        request_id: &str,
        reply: PermissionReply,
        message: Option<&str>,
    ) -> Result<()> {
        let snapshot = self.snapshot().await?;
        ensure!(
            snapshot
                .permissions
                .iter()
                .any(|request| request.id() == request_id
                    && request.session_id() == snapshot.session_id),
            "OpenCode permission is not pending in the selected session"
        );
        self.client
            .reply_permission(request_id, reply, message)
            .await?;
        self.refresh_interactions().await
    }

    /// Answer one currently pending question request in the upstream order.
    pub async fn reply_question(&self, request_id: &str, answers: Vec<Vec<String>>) -> Result<()> {
        self.require_question(request_id).await?;
        self.client.reply_question(request_id, answers).await?;
        self.refresh_interactions().await
    }

    /// Reject one currently pending question request.
    pub async fn reject_question(&self, request_id: &str) -> Result<()> {
        self.require_question(request_id).await?;
        self.client.reject_question(request_id).await?;
        self.refresh_interactions().await
    }

    /// Answer only a current typed form owned by the selected session.
    pub async fn reply_form(&self, request_id: &str, answers: serde_json::Value) -> Result<()> {
        self.require_question(request_id).await?;
        self.client.reply_form(request_id, answers).await?;
        self.refresh_interactions().await
    }

    async fn require_question(&self, request_id: &str) -> Result<()> {
        let snapshot = self.snapshot().await?;
        ensure!(
            snapshot
                .questions
                .iter()
                .any(|request| request.id() == request_id
                    && request.session_id() == snapshot.session_id),
            "OpenCode interaction is not pending in the selected session"
        );
        Ok(())
    }

    /// Abort execution for the selected session without stopping OpenCode.
    pub async fn abort(&self, session_id: &str) -> Result<()> {
        let selected = self.state.lock().await.snapshot.clone();
        ensure!(
            selected
                .as_ref()
                .is_some_and(|snapshot| snapshot.session_id == session_id),
            "OpenCode session is not selected"
        );
        self.client.abort(session_id).await?;
        Ok(())
    }

    /// Subscribe to the scoped upstream SSE stream.
    ///
    /// The stream is owned by the caller. Dropping it only closes this
    /// controller's HTTP connection and does not abort the OpenCode session.
    pub async fn events(&self) -> Result<EventStream> {
        let mut state = self.state.lock().await;
        state.live.clear();
        state.seen.clear();
        state.seen_order.clear();
        drop(state);
        Ok(self.client.events().await?)
    }

    async fn refresh_interactions(&self) -> Result<()> {
        let (session_id, revision) = {
            let state = self.state.lock().await;
            let snapshot = state
                .snapshot
                .as_ref()
                .context("no OpenCode session selected")?;
            (snapshot.session_id.clone(), snapshot.revision)
        };
        let permissions = self
            .client
            .permissions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let questions = self
            .client
            .questions()
            .await?
            .into_iter()
            .filter(|request| request.session_id() == session_id)
            .collect();
        let mut state = self.state.lock().await;
        if let Some(snapshot) = state.snapshot.as_mut() {
            if snapshot.session_id == session_id && snapshot.revision == revision {
                snapshot.permissions = permissions;
                snapshot.questions = questions;
                snapshot.revision += 1;
            }
        }
        Ok(())
    }
}

fn record_live_text(
    state: &mut State,
    event: pocket_codex_host_svc::opencode::v2::Event,
) -> Result<()> {
    let Some((kind, action)) = event
        .kind
        .strip_prefix("session.")
        .and_then(|name| name.split_once('.'))
    else {
        return Ok(());
    };
    if !matches!(kind, "text" | "reasoning") || !matches!(action, "started" | "delta" | "ended") {
        return Ok(());
    }
    if let Some(id) = event.id {
        if !state.seen.insert(id.clone()) {
            return Ok(());
        }
        state.seen_order.push_back(id);
        if state.seen_order.len() > 2048 {
            if let Some(id) = state.seen_order.pop_front() {
                state.seen.remove(&id);
            }
        }
    }
    let id = event.data["assistantMessageID"]
        .as_str()
        .context("invalid OpenCode live message identity")?;
    let ordinal = event.data["ordinal"]
        .as_u64()
        .filter(|value| *value < 1024)
        .context("invalid OpenCode live ordinal")? as usize;
    let key = (id.into(), kind.into(), ordinal);
    ensure!(
        state.live.contains_key(&key) || state.live.len() < 256,
        "OpenCode live content exceeds the resource limit"
    );
    match action {
        "started" => {
            state.live.entry(key).or_default();
        },
        "delta" => {
            if let Some(part) = state.live.get_mut(&key) {
                if !part.complete {
                    let delta = event.data["delta"]
                        .as_str()
                        .context("invalid OpenCode live delta")?;
                    ensure!(
                        part.text.len().saturating_add(delta.len()) <= 8 * 1024 * 1024,
                        "OpenCode live content exceeds the resource limit"
                    );
                    part.text.push_str(delta);
                }
            }
        },
        "ended" => {
            let text = event.data["text"]
                .as_str()
                .context("invalid OpenCode live text")?;
            ensure!(
                text.len() <= 8 * 1024 * 1024,
                "OpenCode live content exceeds the resource limit"
            );
            state.live.insert(key, LiveText {
                text: text.into(),
                complete: true,
            });
        },
        _ => {},
    }
    Ok(())
}

fn apply_live_text(
    messages: &mut [NativeMessage],
    live: &BTreeMap<(String, String, usize), LiveText>,
) {
    for ((id, kind, ordinal), part) in live {
        let Some(NativeMessage::V2(message)) =
            messages.iter_mut().find(|message| message.id() == id)
        else {
            continue;
        };
        if message.kind != "assistant" {
            continue;
        }
        let Some(content) = message
            .extra
            .get_mut("content")
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        let matching: Vec<_> = content
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (item["type"].as_str() == Some(kind)).then_some(index))
            .collect();
        if let Some(index) = matching.get(*ordinal) {
            let previous = content[*index]["text"].as_str().unwrap_or_default();
            if part.complete || part.text.len() >= previous.len() {
                content[*index]["text"] = serde_json::Value::String(part.text.clone());
            }
        } else if *ordinal == matching.len() {
            content.push(serde_json::json!({"type":kind,"text":part.text}));
        }
    }
}

/// Register an in-memory controller and return an opaque connection ID.
pub fn register(client: impl Into<Connection>, relay_key: Option<String>) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    lock_connections().insert(id.clone(), ConnectionEntry {
        controller: Arc::new(OpenCodeController::new(client)),
        relay_key,
        event_tasks: Vec::new(),
    });
    id
}

/// Attach an SSE task to a connection so disconnect can release it.
pub fn track_event_task(id: &str, task: tokio::task::JoinHandle<()>) -> bool {
    let mut connections = lock_connections();
    let Some(entry) = connections.get_mut(id) else {
        task.abort();
        return false;
    };
    entry.event_tasks.push(task);
    true
}

/// Get a live controller by connection ID.
pub fn get(id: &str) -> Result<Arc<OpenCodeController>> {
    lock_connections()
        .get(id)
        .map(|entry| Arc::clone(&entry.controller))
        .context("OpenCode connection not found")
}

/// Remove a controller without touching the external OpenCode process.
pub fn disconnect(id: &str) {
    if let Some(entry) = lock_connections().remove(id) {
        for task in entry.event_tasks {
            task.abort();
        }
        if let Some(key) = entry.relay_key {
            crate::engine::runtime::unsubscribe_service(&key);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_v2;

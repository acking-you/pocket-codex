//! Controller-side OpenCode engine: the `app_*` bridge surface for services of
//! kind `opencode`, producing the same items, histories and events as the
//! Codex engine so the session UI is shared.
//!
//! One connection per service key talks HTTP to the host's OpenCode gateway
//! (loopback when this process hosts it, otherwise a relay tunnel) and keeps
//! one resilient event-stream task that reconnects and reconciles.

#[cfg(test)]
mod engine_tests;
mod events;
mod history;
#[cfg(test)]
mod live_dual;
#[cfg(test)]
mod live_requests;
#[cfg(test)]
mod live_tests;
mod mapping;
mod ops;
mod turns;

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
pub use history::{thread_older_page, thread_read, thread_turn_page};
use once_cell::sync::OnceCell;
pub use ops::{model_list, running_sessions, thread_list, thread_resume, thread_start};
use pocket_codex_core::service::ServiceKind;
use pocket_codex_host_svc::opencode::{Client, Form, Message};
use tokio::{sync::broadcast, task::JoinHandle};
pub use turns::*;

use crate::engine::{app_session::AppEvent, runtime, transport::Transport};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Most messages retained per thread for paging and turn assignment.
const MAX_RETAINED: usize = 2048;

/// Whether `service_key` names an OpenCode session service.
pub fn is_opencode(service_key: &str) -> bool {
    let parts: Vec<&str> = service_key.split(':').collect();
    let kind = match parts.first() {
        Some(&"pcx") if parts.len() >= 4 => parts[2],
        Some(&"pcxu") if parts.len() >= 5 => parts[3],
        _ => return false,
    };
    kind == ServiceKind::OpenCode.as_key_segment()
}

/// What a pending request id refers to.
#[derive(Clone)]
enum Pending {
    Permission { session: String },
    Form(Form),
}

/// Per-thread history bookkeeping.
#[derive(Default)]
struct Window {
    /// Retained messages, oldest first.
    messages: Vec<Message>,
    /// Cursor to the next older message page, `None` at the beginning.
    older: Option<String>,
    /// Whether at least one page was read.
    read: bool,
    /// Known turn starts (user messages), oldest first.
    starts: Vec<mapping::TurnStart>,
}

#[derive(Default)]
struct Shared {
    translator: events::Translator,
    active: BTreeSet<String>,
    pending: HashMap<String, Pending>,
    windows: HashMap<String, Window>,
    /// session → working directory.
    directories: HashMap<String, String>,
    config: HashMap<String, crate::engine::app_session::ThreadRuntimeConfig>,
}

struct Conn {
    client: Client,
    tx: broadcast::Sender<AppEvent>,
    task: JoinHandle<()>,
    shared: Arc<Mutex<Shared>>,
}

fn conns() -> std::sync::MutexGuard<'static, HashMap<String, Conn>> {
    static CONNS: OnceCell<Mutex<HashMap<String, Conn>>> = OnceCell::new();
    CONNS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

/// The connection handles for `service_key`, or an error when not connected.
fn conn(service_key: &str) -> Result<(Client, Arc<Mutex<Shared>>, broadcast::Sender<AppEvent>)> {
    conns()
        .get(service_key)
        .map(|c| (c.client.clone(), c.shared.clone(), c.tx.clone()))
        .ok_or_else(|| anyhow!("not connected to {service_key}"))
}

/// Resolve the gateway origin: loopback when hosted here, otherwise a relay
/// tunnel.
fn origin(
    service_key: &str,
    local_port: u16,
    transport: &Transport,
    transient: bool,
) -> Result<(String, Option<JoinHandle<()>>)> {
    if let Some((addr, _)) = crate::engine::serve::local_endpoints(service_key) {
        return Ok((format!("http://{addr}/"), None));
    }
    if transient {
        let (addr, handle) =
            runtime::subscribe_transient(service_key.to_string(), local_port, transport)?;
        return Ok((format!("http://{addr}/"), Some(handle)));
    }
    let sub = runtime::subscribe_service(service_key.to_string(), local_port, transport)?;
    Ok((format!("http://{}/", sub.local_addr), None))
}

async fn handshake(client: &Client) -> Result<()> {
    tokio::time::timeout(CONNECT_TIMEOUT, client.connect())
        .await
        .context("OpenCode did not answer in time")?
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// Connect (idempotent while the connection is healthy).
pub fn connect(service_key: String, local_port: u16, transport: &Transport) -> Result<()> {
    if is_connected(&service_key) {
        return Ok(());
    }
    disconnect(&service_key);
    let (origin, _) = origin(&service_key, local_port, transport, false)?;
    let client = Client::new(&origin, None).map_err(|e| anyhow!("{e}"))?;
    runtime::runtime().block_on(handshake(&client))?;
    let (tx, _) = broadcast::channel(1024);
    let shared = Arc::new(Mutex::new(Shared::default()));
    let task = runtime::runtime().spawn(event_loop(client.clone(), tx.clone(), shared.clone()));
    conns().insert(service_key, Conn {
        client,
        tx,
        task,
        shared,
    });
    Ok(())
}

/// Whether a live connection exists.
pub fn is_connected(service_key: &str) -> bool {
    conns()
        .get(service_key)
        .is_some_and(|c| !c.task.is_finished())
}

/// Drop the connection and its relay subscription.
pub fn disconnect(service_key: &str) {
    if let Some(conn) = conns().remove(service_key) {
        conn.task.abort();
        runtime::unsubscribe_service(service_key);
    }
}

/// Why the service is unreachable, or `None` when it answers.
pub fn probe_reason(service_key: String, transport: &Transport) -> Option<String> {
    if is_connected(&service_key) {
        return None;
    }
    let (origin, handle) = match origin(&service_key, 0, transport, true) {
        Ok(v) => v,
        Err(e) => return Some(format!("{e:#}")),
    };
    let reason = Client::new(&origin, None)
        .map_err(|e| anyhow!("{e}"))
        .and_then(|client| runtime::runtime().block_on(handshake(&client)))
        .err()
        .map(|e| format!("{e:#}"));
    if let Some(handle) = handle {
        handle.abort();
    }
    reason
}

/// Subscribe to translated live events.
pub fn subscribe_events(service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
    conns()
        .get(service_key)
        .map(|c| c.tx.subscribe())
        .ok_or_else(|| anyhow!("not connected to {service_key}"))
}

/// Keep one event stream open; after a gap, reconcile execution state so no
/// turn spins forever and no finished request lingers.
async fn event_loop(client: Client, tx: broadcast::Sender<AppEvent>, shared: Arc<Mutex<Shared>>) {
    let mut backoff = Duration::from_secs(1);
    let mut first = true;
    loop {
        match client.events().await {
            Ok(mut stream) => {
                if !first {
                    reconcile(&client, &tx, &shared).await;
                }
                first = false;
                backoff = Duration::from_secs(1);
                while let Some(item) = stream.next().await {
                    let Ok(ev) = item else { break };
                    let out = {
                        let mut s = lock(&shared);
                        observe(&mut s, &ev);
                        s.translator.translate(&ev)
                    };
                    for app in out {
                        let _ = tx.send(app);
                    }
                }
            },
            Err(e) => tracing::debug!(error = %e, "OpenCode event stream unavailable"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Track execution, pending requests and directories from raw events.
fn observe(s: &mut Shared, ev: &pocket_codex_host_svc::opencode::Event) {
    let Some(session) = ev.session_id().map(str::to_string) else { return };
    if let Some(location) = &ev.location {
        s.directories
            .entry(session.clone())
            .or_insert_with(|| location.directory.clone());
    }
    let name = ev.kind.strip_prefix("session.").unwrap_or(&ev.kind);
    match name {
        "execution.started" => {
            s.active.insert(session.clone());
        },
        "execution.succeeded" | "execution.failed" | "execution.interrupted" => {
            s.active.remove(&session);
        },
        "permission.asked" => {
            if let Some(id) = ev.data["id"].as_str() {
                s.pending.insert(id.to_string(), Pending::Permission {
                    session: session.clone(),
                });
            }
        },
        "form.created" => {
            if let Ok(form) = serde_json::from_value::<Form>(ev.data["form"].clone()) {
                s.pending.insert(form.id.clone(), Pending::Form(form));
            }
        },
        "permission.replied" => {
            if let Some(id) = ev.data["requestID"].as_str() {
                s.pending.remove(id);
            }
        },
        "form.replied" | "form.cancelled" => {
            if let Some(id) = ev.data["id"].as_str() {
                s.pending.remove(id);
            }
        },
        _ => {},
    }
    // Any change to a thread's content makes its retained window stale.
    if name.starts_with("execution.") || name == "compaction.ended" {
        if let Some(window) = s.windows.get_mut(&session) {
            window.read = false;
        }
    }
}

/// After a stream gap: finish turns that ended meanwhile.
async fn reconcile(client: &Client, tx: &broadcast::Sender<AppEvent>, shared: &Mutex<Shared>) {
    let Ok(active) = client.active().await else { return };
    let ended: Vec<(String, String)> = {
        let mut s = lock(shared);
        let ended: Vec<String> = s.active.difference(&active).cloned().collect();
        s.active = active;
        ended
            .into_iter()
            .map(|id| {
                let turn = s.translator.turn_id(&id);
                (id, turn)
            })
            .collect()
    };
    for (session, turn) in ended {
        let raw =
            serde_json::json!({"threadId": session, "turn": {"id": turn, "status": "completed"}});
        let _ = tx.send(AppEvent {
            kind: "turn/completed".into(),
            thread_id: Some(session),
            item_id: None,
            item_type: None,
            title: None,
            text: None,
            images: Vec::new(),
            request_id: None,
            raw: raw.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_keys_are_recognized_in_both_namespaces() {
        assert!(is_opencode("pcx:mac:opencode:opencode"));
        assert!(is_opencode("pcxu:alice:mac:opencode:work"));
        assert!(!is_opencode("pcx:mac:app:opencode"));
        assert!(!is_opencode("garbage"));
    }
}

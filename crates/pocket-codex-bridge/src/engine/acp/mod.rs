//! Controller-side engine for `acp` services: any ACP agent hosted behind
//! the Pocket-Codex gateway (`/acp/v1`), on this device or another.
//!
//! # Ownership
//!
//! A connection belongs to one transport [`Context`] (account and relay, or
//! self-host relay and key) *and* one logical service key: the same `pcx:`
//! key under another account or relay is another service, with its own
//! client, relay subscription, capabilities and permissions. A namespaced
//! key of another account is refused. When the context changes (sign-in,
//! sign-out, another relay or key), every connection of the old one is
//! dropped, and a connection still being established then is discarded
//! instead of registered (see [`transport::epoch`]). A host this process
//! runs is reached on loopback only when it was published under the same
//! context.
//!
//! Every operation acts through one captured [`Link`] — the connection's
//! client and view — and carries the host incarnation and generation that
//! view shows, so the host refuses it if either changed. Its result is
//! applied only to that same connection, and only while it still shows that
//! host and generation: a late answer never lands in a newer connection.
//!
//! # Consistency
//!
//! One event task per connection streams events strictly after the
//! snapshot watermark. On a `reset` (generation change, replaced host
//! incarnation, replay overflow, lag) it reads a fresh snapshot, completes
//! turns that ended meanwhile with their final text and recorded outcome,
//! announces turns it had not seen begin, and rebuilds the live folds of
//! running turns from history — whose answers carry their own watermark. A
//! turn whose fold cannot be read yet stays explicitly *unsynchronized*: its
//! updates are not folded (that would start it without its prefix), and the
//! read is retried with backoff — on the next event of that turn or on a
//! timer — until it succeeds or the turn ends, which then says its items
//! could not be read. So item ids and text match the host's, without
//! duplicates, missing prefixes or invented history.
//!
//! # Health
//!
//! A connection is healthy only while its event stream is open; a snapshot
//! alone proves nothing. A refused stream (the host's stream limit) or a
//! failed one makes capabilities fall back to the conservative unnegotiated
//! set, withdraws pending permissions from view (they come back on
//! recovery) and tells the UI the host is unreachable. A local host replaced
//! under the same key (a new loopback port) ends the connection, so the next
//! connect reaches the new one. Disconnecting drops only this controller's
//! view: turns and permissions belong to the host and keep running.

#[cfg(test)]
mod live_tests;
mod ops;
mod translate;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use anyhow::{anyhow, bail, Context as _, Result};
use futures::StreamExt;
use once_cell::sync::OnceCell;
pub use ops::*;
use pocket_codex_host_svc::acp::{
    client::EventStream, ClientError, GatewayClient, Identity, Replica,
};
use serde_json::Value;
use tokio::{sync::broadcast, task::JoinHandle, time::Instant};

use super::{
    app_session::AppEvent,
    runtime,
    session_engine::{logical_key, Capabilities},
    transport::{self, Context, Owned},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const EVENT_BUFFER: usize = 1024;
/// First wait before retrying an unreadable running fold.
const FOLD_RETRY: Duration = Duration::from_millis(250);
/// Longest wait between retries of an unreadable running fold.
const MAX_FOLD_RETRY: Duration = Duration::from_secs(5);

/// When an unsynchronized turn's fold is read again.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Retry {
    due: Instant,
    delay: Duration,
}

/// The controller's view of one host, kept consistent with its snapshots.
#[derive(Default)]
pub(crate) struct Shared {
    /// Host incarnation the view belongs to.
    host_id: String,
    /// Host generation the view belongs to.
    generation: u64,
    /// Host info (phase, profile, negotiated capabilities).
    info: Value,
    /// Session → running host turn.
    running: HashMap<String, String>,
    /// Session → latest session-state body.
    sessions: HashMap<String, Value>,
    /// Handle → pending permission body.
    permissions: HashMap<String, Value>,
    /// Live folds, aligned with the host's.
    live: Replica,
    /// Running turns whose fold could not be read yet: `(session, turn)`.
    unsynced: HashMap<(String, String), Retry>,
    /// Session → oldest loaded turn (paging cursor).
    oldest: HashMap<String, String>,
    /// The event stream is open.
    healthy: bool,
}

impl Shared {
    /// The identity this view acts on: its host incarnation and generation.
    fn identity(&self) -> Identity {
        Identity {
            host_id: Some(self.host_id.clone()).filter(|id| !id.is_empty()),
            generation: Some(self.generation),
        }
    }

    /// Whether this view still shows `identity`.
    fn shows(&self, identity: &Identity) -> bool {
        *identity == self.identity()
    }

    /// Record that `turn`'s fold could not be read: retry later, with
    /// backoff.
    fn unreadable(&mut self, session: &str, turn: &str) {
        let key = (session.to_string(), turn.to_string());
        let delay = self
            .unsynced
            .get(&key)
            .map_or(FOLD_RETRY, |retry| (retry.delay * 2).min(MAX_FOLD_RETRY));
        self.unsynced.insert(key, Retry {
            due: Instant::now() + delay,
            delay,
        });
    }

    /// Whether `turn`'s fold may be read now: never tried, or its retry is
    /// due.
    fn may_read(&self, session: &str, turn: &str) -> bool {
        self.unsynced
            .get(&(session.to_string(), turn.to_string()))
            .is_none_or(|retry| retry.due <= Instant::now())
    }

    /// The earliest due retry.
    fn next_retry(&self) -> Option<Instant> {
        self.unsynced.values().map(|retry| retry.due).min()
    }
}

/// Registry key: transport context id and logical service key.
type Key = (String, String);

struct Conn {
    client: GatewayClient,
    tx: broadcast::Sender<AppEvent>,
    task: JoinHandle<()>,
    shared: Arc<Mutex<Shared>>,
    /// Registry key of the relay subscription this connection owns.
    subscription: Option<String>,
    /// The loopback gateway address, when this process hosts the service.
    local: Option<String>,
}

fn conns() -> MutexGuard<'static, HashMap<Key, Conn>> {
    static CONNS: OnceCell<Mutex<HashMap<Key, Conn>>> = OnceCell::new();
    CONNS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

pub(crate) fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Serializes connect/disconnect of one key without holding the registry
/// across network I/O.
fn lifecycle(key: &Key) -> Arc<Mutex<()>> {
    type Locks = Mutex<HashMap<Key, Weak<Mutex<()>>>>;
    static LOCKS: OnceCell<Locks> = OnceCell::new();
    let mut locks = LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key.clone(), Arc::downgrade(&lock));
    lock
}

/// The registry key of `service_key` in `context`. A namespaced key must
/// belong to the context's own account.
fn key_in(context: &Context, service_key: &str) -> Result<Key> {
    if !context.admits(service_key) {
        bail!("this service belongs to another account or relay");
    }
    let logical = logical_key(service_key)
        .ok_or_else(|| anyhow!("not a Pocket-Codex service key: {service_key}"))?;
    Ok((context.id().to_string(), logical))
}

/// The registry key of `service_key` in the current context.
fn current_key(service_key: &str) -> Result<Key> {
    let context =
        transport::current_context().ok_or_else(|| anyhow!("not connected to {service_key}"))?;
    key_in(&context, service_key)
}

fn subscription_key(key: &Key) -> String {
    format!("acp\u{1f}{}\u{1f}{}", key.0, key.1)
}

/// One connection as an operation captured it (see the module docs).
pub(crate) struct Link {
    pub(crate) client: GatewayClient,
    pub(crate) shared: Arc<Mutex<Shared>>,
    pub(crate) tx: broadcast::Sender<AppEvent>,
    key: Key,
}

impl Link {
    /// The host incarnation and generation this connection shows now.
    pub(crate) fn identity(&self) -> Identity {
        lock(&self.shared).identity()
    }

    /// Apply `commit` to this connection's view if it is still the
    /// registered connection and still shows `identity`; `None` otherwise
    /// (the answer belongs to a connection or host that is gone).
    pub(crate) fn commit<T>(
        &self,
        identity: &Identity,
        commit: impl FnOnce(&mut Shared) -> T,
    ) -> Option<T> {
        let conns = conns();
        let registered = conns
            .get(&self.key)
            .is_some_and(|conn| Arc::ptr_eq(&conn.shared, &self.shared));
        if !registered {
            return None;
        }
        let mut shared = lock(&self.shared);
        shared.shows(identity).then(|| commit(&mut shared))
    }
}

/// The current connection of `service_key`.
pub(crate) fn conn(service_key: &str) -> Result<Link> {
    let key = current_key(service_key)?;
    let conns = conns();
    let conn = conns
        .get(&key)
        .ok_or_else(|| anyhow!("not connected to {}", key.1))?;
    Ok(Link {
        client: conn.client.clone(),
        shared: conn.shared.clone(),
        tx: conn.tx.clone(),
        key,
    })
}

pub(crate) fn block<T>(
    future: impl std::future::Future<Output = Result<T, ClientError>>,
) -> Result<T> {
    runtime::runtime()
        .block_on(future)
        .map_err(|error| anyhow!("{error}"))
}

/// Where a connection reaches its gateway.
struct Origin {
    url: String,
    /// The relay subscription made for it (registry key), if any.
    subscription: Option<String>,
    /// The loopback gateway, when hosted here.
    local: Option<String>,
    /// A transient tunnel to abort when done (probes).
    transient: Option<JoinHandle<()>>,
}

impl Origin {
    fn release(&self) {
        if let Some(subscription) = &self.subscription {
            runtime::unsubscribe_service(subscription);
        }
    }
}

/// Resolve the gateway origin. A host this process owns under the same
/// context is reached on loopback — decided before any relay work; anything
/// else through a relay subscription scoped to the context.
fn origin(
    key: &Key,
    context: &Context,
    local_port: u16,
    owned: &Owned,
    transient: bool,
) -> Result<Origin> {
    let transport = &owned.transport;
    if let Some(gateway) = super::serve_acp::local_gateway(&key.1, context) {
        return Ok(Origin {
            url: format!("http://{gateway}/"),
            subscription: None,
            local: Some(gateway),
            transient: None,
        });
    }
    if transient {
        let (addr, handle) = runtime::subscribe_transient(key.1.clone(), local_port, transport)?;
        return Ok(Origin {
            url: format!("http://{addr}/"),
            subscription: None,
            local: None,
            transient: Some(handle),
        });
    }
    let registry = subscription_key(key);
    let epoch = owned.epoch;
    let sub =
        runtime::subscribe_service_as(registry.clone(), &key.1, local_port, transport, || {
            transport::is_current(epoch)
        })?;
    Ok(Origin {
        url: format!("http://{}/", sub.local_addr),
        subscription: Some(registry),
        local: None,
        transient: None,
    })
}

fn check_api(snapshot: &Value) -> Result<()> {
    match snapshot["info"]["api"].as_u64() {
        Some(1) => Ok(()),
        Some(other) => bail!("this agent host speaks gateway API {other}; update Pocket-Codex"),
        None => bail!("the service did not answer like a Pocket-Codex ACP gateway"),
    }
}

/// A fresh snapshot and an event stream opened at its watermark: a view
/// that is healthy from its first moment.
async fn open(client: &GatewayClient) -> Result<(Value, EventStream)> {
    let snapshot = client
        .snapshot()
        .await
        .map_err(|error| anyhow!("{error}"))?;
    check_api(&snapshot)?;
    let host_id = snapshot["info"]["hostId"].as_str().map(str::to_string);
    let generation = snapshot["generation"].as_u64().unwrap_or(0);
    let seq = snapshot["seq"].as_u64().unwrap_or(0);
    let stream = client
        .events(host_id.as_deref(), generation, seq)
        .await
        .map_err(|error| match error {
            ClientError::Host {
                ..
            } => anyhow!(
                "the agent host refused another live connection (too many are open); close one \
                 and try again"
            ),
            other => anyhow!("{other}"),
        })?;
    Ok((snapshot, stream))
}

/// Connect (idempotent while the connection is healthy and current). The
/// connection belongs to the context `owned` was resolved in and is
/// registered only while that context is current.
pub fn connect(service_key: &str, local_port: u16, owned: &Owned) -> Result<()> {
    let context = Context::of(&owned.transport);
    let epoch = owned.epoch;
    let key = key_in(&context, service_key)?;
    let lifecycle = lifecycle(&key);
    let _guard = lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if is_current(&key, &context) {
        return Ok(());
    }
    disconnect_inner(&key);
    let origin = origin(&key, &context, local_port, owned, false)?;
    let client = match GatewayClient::new(&origin.url) {
        Ok(client) => client,
        Err(error) => {
            origin.release();
            return Err(anyhow!("{error}"));
        },
    };
    let opened = runtime::runtime()
        .block_on(async { tokio::time::timeout(CONNECT_TIMEOUT, open(&client)).await })
        .context("the agent host did not answer in time")
        .and_then(|opened| opened);
    let (snapshot, stream) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            origin.release();
            return Err(error);
        },
    };
    let (tx, _) = broadcast::channel(EVENT_BUFFER);
    let shared = Arc::new(Mutex::new(Shared::default()));
    // Seed the view now, so capabilities and state are there when this
    // returns; nobody listens yet, so its events go nowhere. Its stream is
    // already open, so it is healthy.
    {
        let mut view = lock(&shared);
        let _ = translate::reconcile(&mut view, &snapshot);
        view.healthy = true;
    }
    let watch = Watch {
        logical: key.1.clone(),
        context: context.clone(),
        local: origin.local.clone(),
    };
    // The event task recovers running folds from this snapshot before it
    // streams, so a controller joining mid-turn starts aligned.
    let task = runtime::runtime().spawn(event_loop(
        client.clone(),
        tx.clone(),
        shared.clone(),
        snapshot,
        Some(stream),
        watch,
    ));
    register(
        key,
        Conn {
            client,
            tx,
            task,
            shared,
            subscription: origin.subscription,
            local: origin.local,
        },
        epoch,
    )
}

/// Register `conn` unless the context changed since `epoch` was read: then
/// it belongs to the old context, is released instead and must not outlive
/// the change (see [`context_changed`]).
fn register(key: Key, conn: Conn, epoch: u64) -> Result<()> {
    let mut registry = conns();
    if transport::epoch() != epoch {
        drop(registry);
        release(conn);
        bail!("the account or relay changed while connecting; try again");
    }
    registry.insert(key, conn);
    Ok(())
}

/// Whether `key`'s connection is alive, its stream healthy, and — for a
/// local host — still pointing at the host currently published here.
fn is_current(key: &Key, context: &Context) -> bool {
    let conns = conns();
    let Some(conn) = conns.get(key) else { return false };
    if conn.task.is_finished() || !lock(&conn.shared).healthy {
        return false;
    }
    match &conn.local {
        Some(local) => super::serve_acp::local_gateway(&key.1, context).as_ref() == Some(local),
        None => true,
    }
}

/// Whether a live, healthy connection exists in the current context.
pub fn is_connected(service_key: &str) -> bool {
    let Some(context) = transport::current_context() else { return false };
    let Ok(key) = key_in(&context, service_key) else { return false };
    is_current(&key, &context)
}

fn disconnect_inner(key: &Key) {
    let removed = conns().remove(key);
    if let Some(conn) = removed {
        release(conn);
    }
}

fn release(conn: Conn) {
    conn.task.abort();
    if let Some(subscription) = &conn.subscription {
        runtime::unsubscribe_service(subscription);
    }
    let _ = conn.tx.send(translate::host_unreachable());
}

/// Drop this controller's connection. Host-owned turns keep running.
pub fn disconnect(service_key: &str) {
    if let Ok(key) = current_key(service_key) {
        let lifecycle = lifecycle(&key);
        let _guard = lifecycle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        disconnect_inner(&key);
    }
}

/// The transport context changed: every connection belongs to the old one.
/// Runs after [`transport::epoch`] moved, under the registry lock, so a
/// connection still being established either is drained here or sees the
/// new epoch and discards itself.
pub fn context_changed() {
    let all: Vec<Conn> = conns().drain().map(|(_, conn)| conn).collect();
    for conn in all {
        release(conn);
    }
}

/// Why the service is unreachable or unusable, or `None` when it is ready.
pub fn probe_reason(service_key: &str, owned: &Owned) -> Option<String> {
    let context = Context::of(&owned.transport);
    let key = match key_in(&context, service_key) {
        Ok(key) => key,
        Err(error) => return Some(format!("{error:#}")),
    };
    if is_current(&key, &context) {
        return None;
    }
    let origin = match origin(&key, &context, 0, owned, true) {
        Ok(found) => found,
        Err(error) => return Some(format!("{error:#}")),
    };
    let reason = GatewayClient::new(&origin.url)
        .map_err(|error| anyhow!("{error}"))
        .and_then(|client| {
            runtime::runtime()
                .block_on(async { tokio::time::timeout(CONNECT_TIMEOUT, client.info()).await })
                .context("the agent host did not answer in time")?
                .map_err(|error| anyhow!("{error}"))
        })
        .map(|info| match info["phase"]["state"].as_str() {
            Some("ready") => None,
            Some("failed") => Some(
                info["phase"]["reason"]
                    .as_str()
                    .unwrap_or("the agent failed to start")
                    .to_string(),
            ),
            Some("stopped") => Some("the agent is stopped on its host".to_string()),
            _ => Some("the agent is starting".to_string()),
        })
        .unwrap_or_else(|error| Some(format!("{error:#}")));
    if let Some(handle) = origin.transient {
        handle.abort();
    }
    reason
}

/// Subscribe to translated live events.
pub fn subscribe_events(service_key: &str) -> Result<broadcast::Receiver<AppEvent>> {
    Ok(conn(service_key)?.tx.subscribe())
}

/// Capabilities negotiated by the host's current agent generation;
/// conservative (nothing optional) until connected, healthy and ready.
pub fn capabilities(service_key: &str) -> Capabilities {
    let Ok(link) = conn(service_key) else {
        return Capabilities::acp_unnegotiated();
    };
    let shared = lock(&link.shared);
    if !shared.healthy {
        return Capabilities::acp_unnegotiated();
    }
    capabilities_of(&shared.info, shared.generation)
}

/// Capabilities from a host info body.
pub fn capabilities_of(info: &Value, generation: u64) -> Capabilities {
    let mut caps = Capabilities::acp_unnegotiated();
    caps.generation = generation;
    if let Some(name) = info["profile"]["displayName"].as_str() {
        caps.provider_name = name.to_string();
    }
    caps.auth_required = info["authRequired"] == true;
    let negotiated = &info["capabilities"];
    if info["phase"]["state"] == "ready" && negotiated.is_object() {
        caps.negotiated = true;
        caps.image_input = negotiated["image"] == true;
        caps.session_config = true;
        caps.session_reopen = if negotiated["loadSession"] == true {
            "load"
        } else if negotiated["resumeSession"] == true {
            "resume"
        } else {
            "none"
        }
        .to_string();
        caps.session_list =
            if negotiated["listSessions"] == true { "agent" } else { "host" }.to_string();
    }
    caps
}

/// What the event task needs to notice a replaced local host.
struct Watch {
    logical: String,
    context: Context,
    local: Option<String>,
}

impl Watch {
    /// Whether a local host was stopped or replaced (another port).
    fn replaced(&self) -> bool {
        self.local.as_ref().is_some_and(|local| {
            super::serve_acp::local_gateway(&self.logical, &self.context).as_ref() != Some(local)
        })
    }
}

fn send_all(tx: &broadcast::Sender<AppEvent>, events: Vec<AppEvent>) {
    for event in events {
        let _ = tx.send(event);
    }
}

/// Read `turn` of `thread` as a fold, if the answer comes from the host
/// incarnation the view shows and from `generation` (a replacement behind
/// the same endpoint may reuse generation numbers and session ids).
async fn read_turn(
    client: &GatewayClient,
    shared: &Mutex<Shared>,
    thread: &str,
    turn: &str,
    generation: u64,
) -> Option<Value> {
    let host = lock(shared).host_id.clone();
    let history = client.history(thread, None, Some(turn), None).await.ok()?;
    (history["generation"].as_u64() == Some(generation)
        && history["hostId"].as_str() == Some(host.as_str()))
    .then_some(history)
}

/// Read and install the fold of running `turn`, or mark it unsynchronized
/// (retried later). `false` when it could not be read.
async fn sync_turn(
    client: &GatewayClient,
    tx: &broadcast::Sender<AppEvent>,
    shared: &Mutex<Shared>,
    thread: &str,
    turn: &str,
    generation: u64,
) -> bool {
    let history = read_turn(client, shared, thread, turn, generation).await;
    let mut view = lock(shared);
    if view.generation != generation {
        return false;
    }
    let installed = history.is_some_and(|history| {
        let events = translate::install(&mut view, thread, turn, &history);
        let installed = !view.live.needs_fold(thread, Some(turn));
        send_all(tx, events);
        installed
    });
    if installed {
        view.unsynced
            .remove(&(thread.to_string(), turn.to_string()));
    } else if view.running.get(thread).map(String::as_str) == Some(turn) {
        view.unreadable(thread, turn);
    }
    installed
}

/// Resynchronize from `snapshot` (see the module docs for the order).
/// Returns the watermark.
async fn recover(
    client: &GatewayClient,
    tx: &broadcast::Sender<AppEvent>,
    shared: &Mutex<Shared>,
    snapshot: &Value,
) -> u64 {
    let plan = translate::reconcile(&mut lock(shared), snapshot);
    let generation = plan.generation;
    // A turn that ended is completed before anything newer is announced, so
    // its completion can never end its successor.
    for (thread, turn, outcome) in &plan.ended {
        let history = read_turn(client, shared, thread, turn, generation).await;
        send_all(tx, translate::finished(thread, turn, history.as_ref(), outcome.as_ref()));
    }
    for (thread, turn) in &plan.started {
        send_all(tx, vec![translate::started(thread, turn)]);
    }
    for (thread, turn) in &plan.running {
        sync_turn(client, tx, shared, thread, turn, generation).await;
    }
    send_all(tx, plan.events);
    send_all(tx, vec![plan.host_state]);
    snapshot["seq"].as_u64().unwrap_or(0)
}

/// Retry the unsynchronized folds that are due.
async fn retry_due(
    client: &GatewayClient,
    tx: &broadcast::Sender<AppEvent>,
    shared: &Mutex<Shared>,
) {
    let (due, generation): (Vec<(String, String)>, u64) = {
        let view = lock(shared);
        let now = Instant::now();
        let due = view
            .unsynced
            .iter()
            .filter(|(_, retry)| retry.due <= now)
            .map(|(key, _)| key.clone())
            .collect();
        (due, view.generation)
    };
    for (thread, turn) in due {
        sync_turn(client, tx, shared, &thread, &turn, generation).await;
    }
}

/// Resolves when the earliest unsynchronized fold is due (never when none
/// is).
async fn retry_timer(shared: &Mutex<Shared>) {
    let next = lock(shared).next_retry();
    match next {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

/// Why a stream stopped being followed.
enum Ended {
    /// The host asked for a resync.
    Reset,
    /// The stream ended or failed.
    Closed,
}

/// Apply events from `stream` after `watermark` until it resets or ends,
/// retrying unsynchronized folds when they are due.
async fn follow(
    client: &GatewayClient,
    tx: &broadcast::Sender<AppEvent>,
    shared: &Mutex<Shared>,
    stream: &mut EventStream,
    watermark: &mut u64,
) -> Ended {
    loop {
        let next = tokio::select! {
            next = stream.next() => next,
            () = retry_timer(shared) => {
                retry_due(client, tx, shared).await;
                continue;
            },
        };
        let Some(Ok(body)) = next else { return Ended::Closed };
        if body["type"] == "reset" {
            return Ended::Reset;
        }
        let seq = body["seq"].as_u64().unwrap_or(0);
        if seq <= *watermark {
            continue;
        }
        *watermark = seq;
        apply(client, tx, shared, &body).await;
    }
}

async fn fresh_snapshot(client: &GatewayClient) -> Option<Value> {
    let snapshot = client.snapshot().await.ok()?;
    check_api(&snapshot).is_ok().then_some(snapshot)
}

/// Stream events after the watermark; resynchronize from a snapshot on
/// reset or after an outage; track health; end when a local host was
/// replaced. `opened` is a stream already opened at `first`'s watermark.
async fn event_loop(
    client: GatewayClient,
    tx: broadcast::Sender<AppEvent>,
    shared: Arc<Mutex<Shared>>,
    first: Value,
    mut opened: Option<EventStream>,
    watch: Watch,
) {
    let mut watermark = recover(&client, &tx, &shared, &first).await;
    let mut backoff = Duration::from_secs(1);
    loop {
        let (host_id, generation) = {
            let view = lock(&shared);
            (view.host_id.clone(), view.generation)
        };
        let stream = match opened.take() {
            Some(stream) => Ok(stream),
            None => client.events(Some(&host_id), generation, watermark).await,
        };
        match stream {
            Ok(mut stream) => {
                let reopened = !lock(&shared).healthy;
                // Back after an outage: catch up from a fresh snapshot while
                // the stream is open (events up to its watermark are skipped).
                let caught_up = if reopened {
                    match fresh_snapshot(&client).await {
                        Some(snapshot) => {
                            watermark = recover(&client, &tx, &shared, &snapshot).await;
                            true
                        },
                        None => false,
                    }
                } else {
                    true
                };
                if caught_up {
                    lock(&shared).healthy = true;
                    backoff = Duration::from_secs(1);
                    if let Ended::Reset =
                        follow(&client, &tx, &shared, &mut stream, &mut watermark).await
                    {
                        if let Some(snapshot) = fresh_snapshot(&client).await {
                            watermark = recover(&client, &tx, &shared, &snapshot).await;
                            continue;
                        }
                    }
                }
            },
            // Refused (the host's stream limit) or unreachable: either way
            // there is no live view, whatever a snapshot would say.
            Err(error) => tracing::debug!(%error, "ACP gateway event stream unavailable"),
        }
        let events = translate::disconnected(&mut lock(&shared));
        send_all(&tx, events);
        if watch.replaced() {
            // Another host serves this key here now; the next connect
            // reaches it instead of retrying a dead port.
            return;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Translate one live event; a turn this controller never saw begin gets
/// its fold from the host first (or stays unsynchronized when it cannot be
/// read yet).
async fn apply(
    client: &GatewayClient,
    tx: &broadcast::Sender<AppEvent>,
    shared: &Mutex<Shared>,
    body: &Value,
) {
    let thread = body["sessionId"].as_str().unwrap_or("");
    let turn = body["turnId"].as_str();
    let missing = {
        let view = lock(shared);
        let content = body["type"] == "update" || body["type"] == "turn_ended";
        turn.filter(|turn| {
            content && view.live.needs_fold(thread, Some(turn)) && view.may_read(thread, turn)
        })
        .map(|turn| (turn.to_string(), view.generation))
    };
    if let Some((turn, generation)) = missing {
        sync_turn(client, tx, shared, thread, &turn, generation).await;
    }
    let begun_unsynced = {
        let mut view = lock(shared);
        if body["type"] == "turn_ended" {
            if let Some(turn) = turn {
                view.unsynced
                    .remove(&(thread.to_string(), turn.to_string()));
            }
        }
        let events = translate::gateway_event(&mut view, body);
        send_all(tx, events);
        // A turn whose prompt carried images is begun from the host's fold
        // only (see `Replica::apply`): read it now, after announcing it.
        turn.filter(|turn| {
            body["type"] == "turn_started" && view.live.needs_fold(thread, Some(turn))
        })
        .map(|turn| (turn.to_string(), view.generation))
    };
    if let Some((turn, generation)) = begun_unsynced {
        sync_turn(client, tx, shared, thread, &turn, generation).await;
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, net::SocketAddr};

    use pocket_codex_host_svc::acp::fold::Transcript;
    use pocket_codex_pb::RelaySession;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::*;
    use crate::engine::transport::Transport;

    fn transport(namespace: Option<&str>, relay: &str) -> Transport {
        Transport {
            session: RelaySession::for_test(relay),
            namespace: namespace.map(str::to_string),
        }
    }

    /// A connection to nowhere, registered directly.
    fn fake_conn(key: &Key, generation: u64) -> Arc<Mutex<Shared>> {
        fake_conn_at(key, generation, "http://127.0.0.1:9/")
    }

    /// A connection to the gateway at `url`, registered directly.
    fn fake_conn_at(key: &Key, generation: u64, url: &str) -> Arc<Mutex<Shared>> {
        let shared = Arc::new(Mutex::new(Shared {
            generation,
            healthy: true,
            ..Shared::default()
        }));
        let (tx, _) = broadcast::channel(8);
        conns().insert(key.clone(), Conn {
            client: GatewayClient::new(url).expect("client"),
            tx,
            task: runtime::runtime().spawn(std::future::pending::<()>()),
            shared: shared.clone(),
            subscription: None,
            local: None,
        });
        shared
    }

    fn link_of(key: &Key) -> Link {
        let conns = conns();
        let conn = conns.get(key).expect("registered");
        Link {
            client: conn.client.clone(),
            shared: conn.shared.clone(),
            tx: conn.tx.clone(),
            key: key.clone(),
        }
    }

    #[test]
    fn the_same_logical_key_in_two_contexts_is_two_connections() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let alice = Context::of(&transport(Some("alice"), "relay:1"));
        let own = Context::of(&transport(None, "relay:1"));
        let service = "pcx:studio:acp:same-key-test";
        let a = key_in(&alice, service).expect("alice key");
        let b = key_in(&own, service).expect("self-host key");
        assert_ne!(a, b);
        let alice_view = fake_conn(&a, 3);
        let own_view = fake_conn(&b, 9);
        lock(&alice_view)
            .permissions
            .insert("h".into(), serde_json::json!({}));
        assert!(is_current(&a, &alice) && is_current(&b, &own));
        assert!(lock(&own_view).permissions.is_empty(), "permissions stay apart");
        {
            let view = lock(&alice_view);
            assert_eq!(capabilities_of(&view.info, view.generation).generation, 3);
        }
        disconnect_inner(&a);
        assert!(!is_current(&a, &alice));
        assert!(is_current(&b, &own), "disconnecting one leaves the other");
        disconnect_inner(&b);
    }

    #[test]
    fn namespaced_keys_must_match_the_account() {
        let alice = Context::of(&transport(Some("alice"), "relay:1"));
        let own = Context::of(&transport(None, "relay:1"));
        assert!(key_in(&alice, "pcxu:alice:studio:acp:a").is_ok());
        assert!(key_in(&alice, "pcxu:bob:studio:acp:a").is_err(), "another account");
        assert!(key_in(&own, "pcxu:alice:studio:acp:a").is_err(), "not a self-host key");
        assert_eq!(
            key_in(&alice, "pcxu:alice:studio:acp:a").expect("key"),
            key_in(&alice, "pcx:studio:acp:a").expect("key"),
            "both shapes name one service"
        );
    }

    #[test]
    fn an_unhealthy_connection_is_not_current_and_offers_nothing_optional() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let own = Context::of(&transport(None, "relay:health"));
        let key = key_in(&own, "pcx:studio:acp:health-test").expect("key");
        let shared = fake_conn(&key, 1);
        {
            let mut view = lock(&shared);
            view.info =
                serde_json::json!({"phase": {"state": "ready"}, "capabilities": {"image": true}});
        }
        assert!(is_current(&key, &own));
        translate::disconnected(&mut lock(&shared));
        assert!(!is_current(&key, &own), "a failed stream is not a healthy connection");
        disconnect_inner(&key);
    }

    #[test]
    fn a_context_change_drops_every_connection() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let own = Context::of(&transport(None, "relay:change"));
        let key = key_in(&own, "pcx:studio:acp:change-test").expect("key");
        fake_conn(&key, 1);
        transport::context_changed();
        assert!(!is_current(&key, &own));
    }

    /// An answer that arrives after its connection was replaced (another
    /// account, a reconnect, a replaced host) must not land in the new view.
    #[test]
    fn a_late_answer_never_lands_in_a_newer_connection() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let own = Context::of(&transport(None, "relay:late"));
        let key = key_in(&own, "pcx:studio:acp:late-test").expect("key");
        let old_view = fake_conn(&key, 1);
        lock(&old_view).host_id = "old-host".into();
        let old = link_of(&key);
        let identity = old.identity();
        // The request is in flight; the connection is replaced meanwhile.
        disconnect_inner(&key);
        let new_view = fake_conn(&key, 1);
        lock(&new_view).host_id = "new-host".into();
        assert!(old
            .commit(&identity, |view| view.sessions.insert("s".into(), json!({})))
            .is_none());
        assert!(lock(&new_view).sessions.is_empty(), "the new view is untouched");
        // The same connection, but the host moved to another generation.
        let current = link_of(&key);
        let seen = current.identity();
        lock(&new_view).generation = 2;
        assert!(current.commit(&seen, |_| ()).is_none());
        assert!(current.commit(&current.identity(), |_| ()).is_some());
        disconnect_inner(&key);
    }

    /// A connection established while the context changes must not be
    /// registered afterwards (it would serve the old account), and its
    /// resources are released.
    #[test]
    fn a_connection_that_races_a_context_change_is_discarded() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let own = Context::of(&transport(None, "relay:race"));
        let key = key_in(&own, "pcx:studio:acp:race-test").expect("key");
        let epoch = transport::epoch();
        // The connection is being established (network I/O) ...
        let task = runtime::runtime().spawn(std::future::pending::<()>());
        let (tx, mut rx) = broadcast::channel(8);
        let conn = Conn {
            client: GatewayClient::new("http://127.0.0.1:9/").expect("client"),
            tx,
            task,
            shared: Arc::new(Mutex::new(Shared {
                healthy: true,
                ..Shared::default()
            })),
            subscription: None,
            local: None,
        };
        // ... when the account changes.
        transport::context_changed();
        assert!(register(key.clone(), conn, epoch).is_err());
        assert!(conns().get(&key).is_none(), "never registered");
        assert!(rx.try_recv().is_ok(), "its listeners are told it is gone");
        // Registering in the current epoch works.
        let fresh = fake_conn(&key, 1);
        assert!(lock(&fresh).healthy);
        disconnect_inner(&key);
    }

    // ---- A scripted gateway over real HTTP -------------------------------

    #[derive(Default)]
    struct Script {
        snapshot: Value,
        /// Turn → history answer; `None` → the read fails.
        history: HashMap<String, Value>,
        history_fails: bool,
        refuse_events: bool,
        /// Events the next stream sends after its headers.
        queued: VecDeque<Value>,
        live: Option<broadcast::Sender<Value>>,
        streams: usize,
    }

    struct FakeGateway {
        addr: SocketAddr,
        script: Arc<Mutex<Script>>,
        live: broadcast::Sender<Value>,
    }

    fn script(gateway: &FakeGateway) -> MutexGuard<'_, Script> {
        gateway
            .script
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    async fn fake_gateway(snapshot: Value) -> FakeGateway {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (live, _) = broadcast::channel(64);
        let script = Arc::new(Mutex::new(Script {
            snapshot,
            live: Some(live.clone()),
            ..Script::default()
        }));
        tokio::spawn({
            let script = script.clone();
            async move {
                while let Ok((socket, _)) = listener.accept().await {
                    tokio::spawn(serve_one(socket, script.clone()));
                }
            }
        });
        FakeGateway {
            addr,
            script,
            live,
        }
    }

    fn query(path: &str, name: &str) -> Option<String> {
        path.split_once('?')?
            .1
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }

    async fn reply(socket: &mut TcpStream, status: &str, body: &Value) {
        let body = body.to_string();
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: \
             {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(body.as_bytes()).await;
    }

    async fn serve_one(mut socket: TcpStream, script: Arc<Mutex<Script>>) {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if socket.read(&mut byte).await.unwrap_or(0) == 0 {
                return;
            }
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head).to_string();
        let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
        let lock = |script: &Mutex<Script>| {
            script
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .snapshot
                .clone()
        };
        if path.starts_with("/acp/v1/snapshot") {
            let snapshot = lock(&script);
            reply(&mut socket, "200 OK", &snapshot).await;
        } else if path.starts_with("/acp/v1/sessions/history") {
            let answer = {
                let script = script.lock().unwrap_or_else(|poison| poison.into_inner());
                let turn = query(&path, "turn").unwrap_or_default();
                (!script.history_fails)
                    .then(|| script.history.get(&turn).cloned())
                    .flatten()
            };
            match answer {
                Some(history) => reply(&mut socket, "200 OK", &history).await,
                None => {
                    reply(&mut socket, "503 Service Unavailable", &json!({"code": "down"})).await;
                },
            }
        } else if path.starts_with("/acp/v1/events") {
            let (refuse, queued, live) = {
                let mut script = script.lock().unwrap_or_else(|poison| poison.into_inner());
                script.streams += 1;
                let queued: Vec<Value> = script.queued.drain(..).collect();
                (script.refuse_events, queued, script.live.clone())
            };
            if refuse {
                reply(&mut socket, "503 Service Unavailable", &json!({})).await;
                return;
            }
            let Some(live) = live else { return };
            let mut live = live.subscribe();
            let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: \
                        no-cache\r\nconnection: close\r\n\r\n";
            if socket.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            for event in queued {
                let frame = format!("data: {event}\n\n");
                if socket.write_all(frame.as_bytes()).await.is_err() {
                    return;
                }
            }
            loop {
                let frame = match tokio::time::timeout(Duration::from_secs(1), live.recv()).await {
                    Ok(Ok(event)) => format!("data: {event}\n\n"),
                    Ok(Err(_)) => return,
                    Err(_) => ": keep\n\n".to_string(),
                };
                if socket.write_all(frame.as_bytes()).await.is_err() {
                    return;
                }
            }
        } else {
            reply(&mut socket, "404 Not Found", &json!({})).await;
        }
    }

    fn snapshot(sessions: Value, recent: Value) -> Value {
        json!({
            "info": {"api": 1, "hostId": "h", "phase": {"state": "ready"},
                "capabilities": {"image": true}},
            "generation": 1, "seq": 5, "sessions": sessions, "permissions": [],
            "recentTurns": recent,
        })
    }

    fn chunk(text: &str) -> Value {
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
    }

    /// The host's fold of turn `turn` after `chunks`, as a history answer at
    /// watermark `seq`.
    fn history_of(turn: &str, chunks: &[&str], seq: u64, running: bool) -> Value {
        let mut host = Transcript::default();
        host.begin_turn(turn, "q", &[]);
        for text in chunks {
            host.apply(Some(turn), &chunk(text));
        }
        let running_turn = if running { json!(turn) } else { Value::Null };
        json!({"window": [host.turns[0]], "truncated": false, "seq": seq, "generation": 1,
            "hostId": "h", "session": {"runningTurnId": running_turn}})
    }

    async fn drain(rx: &mut broadcast::Receiver<AppEvent>) -> Vec<AppEvent> {
        let mut out = Vec::new();
        while let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await
        {
            out.push(event);
        }
        out
    }

    fn no_watch() -> Watch {
        Watch {
            logical: String::new(),
            context: Context::of(&transport(None, "relay:none")),
            local: None,
        }
    }

    /// A turn A finished while this controller was away and B now runs in
    /// the same session: A completes first, B is announced after, and every
    /// recovered item names its own turn.
    #[tokio::test]
    async fn recovery_completes_the_old_turn_before_announcing_its_successor() {
        let gateway = fake_gateway(snapshot(
            json!([{"sessionId": "s", "runningTurnId": "B"}]),
            json!([{"sessionId": "s", "turnId": "A", "outcome": {"kind": "stopped", "stopReason": "end_turn"}}]),
        ))
        .await;
        {
            let mut script = script(&gateway);
            script
                .history
                .insert("A".into(), history_of("A", &["old answer"], 5, false));
            script
                .history
                .insert("B".into(), history_of("B", &["new"], 5, true));
        }
        let client = GatewayClient::new(&format!("http://{}/", gateway.addr)).expect("client");
        let shared = Mutex::new(Shared::default());
        lock(&shared).running.insert("s".into(), "A".into());
        let (tx, mut rx) = broadcast::channel(64);
        let snapshot = script(&gateway).snapshot.clone();
        recover(&client, &tx, &shared, &snapshot).await;
        let events = drain(&mut rx).await;
        let position = |pred: &dyn Fn(&AppEvent) -> bool| {
            events.iter().position(pred).expect("expected event")
        };
        let turn_of = |event: &AppEvent| {
            serde_json::from_str::<Value>(&event.raw).expect("json")["turnId"]
                .as_str()
                .map(str::to_string)
        };
        let completed_a = position(&|e| e.kind == "turn/completed" && e.raw.contains("\"A\""));
        let started_b = position(&|e| e.kind == "turn/started" && e.raw.contains("\"B\""));
        let item_b = position(&|e| e.item_id.as_deref() == Some("B:m1"));
        assert!(completed_a < started_b, "A completes before B is announced");
        assert!(started_b < item_b);
        let item_a = &events[position(&|e| e.item_id.as_deref() == Some("A:m1"))];
        assert_eq!(turn_of(item_a).as_deref(), Some("A"));
        assert_eq!(turn_of(&events[item_b]).as_deref(), Some("B"));
        assert_eq!(events.last().map(|e| e.kind.as_str()), Some(translate::HOST_STATE));
        assert_eq!(lock(&shared).running.get("s").map(String::as_str), Some("B"));
    }

    /// History cannot be read when the controller joins mid-turn, and an
    /// update arrives meanwhile. Nothing partial is shown; once history is
    /// readable again the fold is installed on a timer — without another
    /// event or reconnect — and the turn ends with the host's text and ids.
    #[tokio::test]
    async fn an_unreadable_fold_stays_unsynchronized_and_recovers_by_itself() {
        let gateway =
            fake_gateway(snapshot(json!([{"sessionId": "s", "runningTurnId": "t"}]), json!([])))
                .await;
        script(&gateway).history_fails = true;
        let client = GatewayClient::new(&format!("http://{}/", gateway.addr)).expect("client");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, mut rx) = broadcast::channel(256);
        let first = script(&gateway).snapshot.clone();
        let task = tokio::spawn(event_loop(client, tx, shared.clone(), first, None, no_watch()));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = gateway
            .live
            .send(json!({"type": "update", "sessionId": "s", "turnId": "t",
            "seq": 6, "update": chunk(" world")}));
        let early = drain(&mut rx).await;
        assert!(
            early.iter().all(|e| e.item_id.is_none()),
            "no item is shown from an unseeded fold: {:?}",
            early.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
        assert!(lock(&shared).live.needs_fold("s", Some("t")), "explicitly unsynchronized");
        // History is readable again, reflecting everything up to seq 6.
        {
            let mut script = script(&gateway);
            script.history_fails = false;
            script
                .history
                .insert("t".into(), history_of("t", &["Hello", " world"], 6, true));
        }
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        let synced = || {
            let view = lock(&shared);
            view.unsynced.is_empty() && !view.live.needs_fold("s", Some("t"))
        };
        while !synced() {
            assert!(Instant::now() < deadline, "the fold was never installed");
            seen.extend(drain(&mut rx).await);
        }
        let _ = gateway
            .live
            .send(json!({"type": "update", "sessionId": "s", "turnId": "t",
            "seq": 7, "update": chunk("!")}));
        let _ = gateway
            .live
            .send(json!({"type": "turn_ended", "sessionId": "s", "turnId": "t",
            "seq": 8, "outcome": {"kind": "stopped", "stopReason": "end_turn"}}));
        seen.extend(drain(&mut rx).await);
        task.abort();
        let mut text = String::new();
        for event in seen.iter().filter(|e| e.item_id.as_deref() == Some("t:m1")) {
            let piece = event.text.clone().unwrap_or_default();
            if event.kind.contains("delta") {
                text.push_str(&piece);
            } else if !piece.is_empty() {
                text = piece;
            }
        }
        assert_eq!(text, "Hello world!", "the host's full text under the host's id");
        assert!(seen.iter().any(|e| e.kind == "turn/completed"));
    }

    /// The host was replaced behind the same endpoint (same generation
    /// number, same session id) and this view's stream has not seen the
    /// reset yet, so the connection still shows the old host. Older pages,
    /// windows and turn reads answered by the replacement are refused: no
    /// item of B is shown as A's history and the paging cursor stays. An
    /// answer of the right host for a connection replaced meanwhile is
    /// refused too, even when it is empty.
    #[test]
    fn history_answered_by_a_replacement_behind_the_same_endpoint_is_refused() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let rt = runtime::runtime();
        let gateway = rt.block_on(fake_gateway(snapshot(json!([]), json!([]))));
        let url = format!("http://{}/", gateway.addr);
        let mut from_b = history_of("b-turn", &["from B"], 9, false);
        from_b["hostId"] = json!("replacement");
        from_b["hasOlder"] = json!(false);
        script(&gateway)
            .history
            .insert(String::new(), from_b.clone());
        script(&gateway).history.insert("a-turn".into(), from_b);
        let own = Context::of(&transport(None, "relay:replaced"));
        let key = key_in(&own, "pcx:studio:acp:replaced-test").expect("key");
        let view = fake_conn_at(&key, 1, &url);
        {
            let mut view = lock(&view);
            view.host_id = "h".into();
            view.oldest.insert("s".into(), "a-turn".into());
        }
        let link = link_of(&key);
        assert!(ops::older_page(&link, "s").is_err(), "B's page is not A's older history");
        assert_eq!(
            lock(&view).oldest.get("s").map(String::as_str),
            Some("a-turn"),
            "the paging cursor did not move"
        );
        assert!(ops::read_window(&link, "s").is_err());
        assert_eq!(lock(&view).oldest.get("s").map(String::as_str), Some("a-turn"));

        // The right host and generation, but the connection was replaced
        // while the (empty) answer was in flight.
        let mut empty = json!({"available": false, "sessionId": "s", "seq": 9,
            "generation": 1, "hostId": "h"});
        script(&gateway)
            .history
            .insert(String::new(), empty.clone());
        disconnect_inner(&key);
        let newer = fake_conn_at(&key, 1, &url);
        lock(&newer).host_id = "h".into();
        assert!(ops::read_window(&link, "s").is_err(), "a stale completion, even empty");
        // Through the current connection the same answer is accepted.
        let current = link_of(&key);
        let read = ops::read_window(&current, "s").expect("current view");
        assert!(read.items.iter().all(|item| item.item_type == "historyGap"));
        empty["hostId"] = json!("replacement");
        script(&gateway).history.insert(String::new(), empty);
        assert!(ops::read_window(&current, "s").is_err(), "and refused from another host");
        disconnect_inner(&key);
    }

    /// The host refuses the event stream (its limit is reached): a snapshot
    /// is still readable, but the view must stay unhealthy — no negotiated
    /// capabilities — until a stream actually opens.
    #[tokio::test]
    async fn a_refused_stream_keeps_the_view_unhealthy_until_a_slot_opens() {
        let gateway = fake_gateway(snapshot(json!([]), json!([]))).await;
        script(&gateway).refuse_events = true;
        let client = GatewayClient::new(&format!("http://{}/", gateway.addr)).expect("client");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, _rx) = broadcast::channel(64);
        let first = script(&gateway).snapshot.clone();
        let task = tokio::spawn(event_loop(client, tx, shared.clone(), first, None, no_watch()));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        {
            let view = lock(&shared);
            assert!(!view.healthy, "a snapshot without a stream is not healthy");
            assert!(script(&gateway).streams >= 1, "the stream was asked for");
        }
        script(&gateway).refuse_events = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !lock(&shared).healthy {
            assert!(Instant::now() < deadline, "never became healthy once a slot opened");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let view = lock(&shared);
        assert!(capabilities_of(&view.info, view.generation).negotiated);
        drop(view);
        task.abort();
    }
}

//! The host-owned ACP client for one agent.
//!
//! [`AgentHost`] owns the agent process for its whole life. One lifecycle
//! lock serializes start, stop, restart and exit handling; whoever holds it
//! and takes the running process is its only owner and performs the full
//! teardown: invalidate the generation (ending turns with their real
//! outcome and withdrawing permissions), close the writer (and with it the
//! agent's input), give the output a moment to end, terminate the process
//! group and reap the leader. [`AgentHost::stop`] is final and bounded: it
//! first cuts short a launch still negotiating, so stopping never waits for
//! a slow handshake.
//!
//! An unexpected exit is handled by the host's one supervisor task: each
//! generation's watcher only reports "gone" for its generation — when the
//! connection closes *or* the leader exits while a descendant still holds
//! its output open — and the supervisor takes the lifecycle lock, tears
//! that generation down (unless a stop or restart already did) and
//! relaunches within the bounded restart budget. The leader is detected
//! without being reaped, so its group is still signalled safely. Launching
//! never waits on the supervisor, so there is no recursion between launch,
//! exit handling and relaunch.
//!
//! # Turns
//!
//! Turns are admitted under the state lock and acknowledged immediately; a
//! separate task dispatches `session/prompt` and awaits the answer. Every
//! prompt dispatch and every `session/cancel` of this host is written under
//! one *dispatch lane*, and each re-checks under it which turn is running.
//! `session/cancel` names only a session, so this is what keeps a cancel
//! meant for turn A from reaching a turn B: if A ended and B was admitted
//! meanwhile, either the cancel is queued before B's prompt (and the agent
//! sees it while nothing runs) or it sees that A is gone and is not sent. A
//! turn cancelled before dispatch is never sent.
//!
//! Every write is bounded. A prompt's dispatch budget starts when the turn
//! is admitted and covers waiting for the lane, for queue room and for the
//! complete write (see `peer`'s delivery rules); only a prompt the agent has
//! received in full then waits — without limit — for its answer. A cancel's
//! or permission answer's control budget likewise starts before the lane and
//! covers every reply it sends. A prompt that was never queued fails without
//! touching the connection. A control exchange that cannot complete in time
//! has already consumed what it answers (a permission request, a running
//! turn's withdrawn requests), so it closes the generation instead of
//! leaving the agent waiting on a live connection: the supervisor tears the
//! generation down (ending its turns truthfully) and restarts within the
//! restart budget. A retry then sees that the turn or request is gone.
//!
//! # Identity
//!
//! A mutation may carry the [`Identity`] (host incarnation and agent
//! generation) its caller acted on; when either changed, it is refused before
//! any side effect. A replacement host behind the same relay key restarts its
//! generations and may even see the same session ids.
//!
//! # Sessions
//!
//! Reopening a session is provisional until the agent confirms it: the
//! session accepts no prompt and grants no new file authority meanwhile, and
//! a failed reopen is revoked. A session's persisted working directory always
//! wins over what the agent lists later. Only [`MAX_TRANSCRIPTS`] sessions
//! hold a transcript; when all of them are running or reopening, another
//! session is refused instead of exceeding the pool.
//!
//! Admissions and titles are persisted off the event path: they are queued,
//! coalesced per session, under the state lock — so their order matches the
//! state's — and one worker writes them.
//!
//! [`MAX_TRANSCRIPTS`]: super::state::MAX_TRANSCRIPTS

use std::{
    collections::VecDeque,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};

use super::{
    fold::{session_signal, SessionSignal},
    jsonrpc::{RequestId, RpcError},
    peer::{CallError, CloseReason, Inbound, Peer},
    process::{self, AgentProcess},
    schema::{self, AgentCapabilities, SessionSettings, TurnOutcome},
    spec::AgentSpec,
    state::{ActiveTurn, LogEntry, Permission, Phase, State, MAX_PERMISSIONS},
    store::{Change, Pending, SessionStore},
};

const INIT_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Budget for delivering a prompt: the lane, queue room and the write.
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Budget for a cancel or a permission answer: the lane (for a cancel),
/// queue room and every write.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);

/// The delivery budgets (see the module docs); tests shorten them.
#[derive(Debug, Clone, Copy)]
struct Budgets {
    dispatch: Duration,
    control: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            dispatch: DISPATCH_TIMEOUT,
            control: CONTROL_TIMEOUT,
        }
    }
}
/// Budget for pending session-record writes when stopping.
const FLUSH_GRACE: Duration = Duration::from_secs(2);
const WRITER_GRACE: Duration = Duration::from_secs(1);
const OUTPUT_GRACE: Duration = Duration::from_secs(2);
const TERM_GRACE: Duration = Duration::from_millis(300);
const RESTART_DELAY: Duration = Duration::from_secs(2);
const RESTART_WINDOW: Duration = Duration::from_secs(600);
const MAX_RESTARTS: usize = 3;
/// How often a running generation checks whether its leader exited.
const EXIT_POLL: Duration = Duration::from_millis(200);
/// Largest prompt text.
pub const MAX_PROMPT_TEXT: usize = 1024 * 1024;
/// Most images per prompt.
pub const MAX_PROMPT_IMAGES: usize = 16;
/// Largest image data URL.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ID: usize = 1024;
const DEFAULT_WINDOW_TURNS: usize = 20;
const MAX_WINDOW_TURNS: usize = 100;

/// Retained events followed by the live receiver from the same log snapshot.
pub type EventSubscription = (Vec<Arc<LogEntry>>, broadcast::Receiver<Arc<LogEntry>>);

/// A failed host operation. Messages are safe to show: they never contain
/// agent arguments or stderr.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// The agent is not running (or the caller's generation is gone).
    #[error("the agent is not running")]
    NotReady,
    /// The caller acted on another host incarnation or agent generation.
    #[error("the agent host changed; refresh and try again")]
    Stale,
    /// The session is not known to this host.
    #[error("unknown session")]
    UnknownSession,
    /// A turn is running in the session.
    #[error("a turn is already running in this session")]
    Busy,
    /// The session is still being reopened.
    #[error("the session is still loading")]
    Loading,
    /// Every retained transcript belongs to a running or reopening session.
    #[error("too many sessions are busy on this agent host; wait for one to finish")]
    Capacity,
    /// The permission handle is unknown.
    #[error("unknown permission request")]
    UnknownPermission,
    /// The permission was already answered, withdrawn or cancelled.
    #[error("this permission request was already resolved")]
    AlreadyResolved,
    /// The option is not one the request offered; the request stays pending.
    #[error("that option was not offered for this request")]
    InvalidOption,
    /// Malformed input.
    #[error("invalid request: {0}")]
    InvalidInput(String),
    /// The agent does not support what was asked.
    #[error("{0}")]
    Unsupported(String),
    /// The agent answered with ACP `auth_required`.
    #[error("the agent requires authentication; sign in with the agent on the host")]
    AuthRequired,
    /// The agent supports neither `session/load` nor `session/resume`.
    #[error("this agent cannot reopen earlier sessions")]
    NotReopenable,
    /// The agent rejected the request.
    #[error("the agent rejected the request: {0}")]
    Agent(String),
    /// No answer in time.
    #[error("the agent did not answer in time")]
    Timeout,
    /// Too much pending work.
    #[error("too many pending operations")]
    Limit,
    /// The agent could not be started.
    #[error("{0}")]
    Start(String),
}

impl HostError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotReady => "not_ready",
            Self::Stale => "stale_host",
            Self::UnknownSession => "unknown_session",
            Self::Busy => "busy",
            Self::Loading => "loading",
            Self::Capacity => "capacity",
            Self::UnknownPermission => "unknown_permission",
            Self::AlreadyResolved => "already_resolved",
            Self::InvalidOption => "invalid_option",
            Self::InvalidInput(_) => "invalid_input",
            Self::Unsupported(_) => "unsupported",
            Self::AuthRequired => "auth_required",
            Self::NotReopenable => "not_reopenable",
            Self::Agent(_) => "agent_error",
            Self::Timeout => "timeout",
            Self::Limit => "limit",
            Self::Start(_) => "start_failed",
        }
    }
}

/// The host incarnation and agent generation a caller acted on (see the
/// module docs). An absent field is not checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// [`AgentHost::host_id`] the caller saw.
    #[serde(default)]
    pub host_id: Option<String>,
    /// The agent generation the caller saw.
    #[serde(default)]
    pub generation: Option<u64>,
}

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// What to host.
pub struct HostOptions {
    /// Agent profile and command line.
    pub spec: AgentSpec,
    /// Resolved executable.
    pub program: PathBuf,
    /// `PATH` for the child only, when it must differ from the inherited one.
    pub path: Option<OsString>,
    /// Admitted-session record of this agent source.
    pub store: SessionStore,
}

struct Running {
    generation: u64,
    peer: Peer,
    process: AgentProcess,
    watcher: JoinHandle<()>,
}

/// Pending session-record writes and flush waiters.
type Queue = Arc<Mutex<(Pending, Vec<oneshot::Sender<()>>)>>;
/// Applies a batch of changes (blocking).
type Apply = Arc<dyn Fn(Vec<Change>) + Send + Sync>;

/// The coalescing, bounded persistence queue (see the module docs).
struct Persist {
    queue: Queue,
    /// Wakes the worker; capacity one, so wake-ups coalesce too.
    wake: mpsc::Sender<()>,
}

impl Persist {
    fn spawn(apply: Apply) -> Self {
        let queue: Queue = Arc::default();
        let (wake, signals) = mpsc::channel(1);
        tokio::spawn(persist_loop(queue.clone(), signals, apply));
        Self {
            queue,
            wake,
        }
    }

    fn submit(&self, change: impl FnOnce(&mut Pending)) {
        change(&mut lock(&self.queue).0);
        let _ = self.wake.try_send(());
    }

    /// Wait up to `limit` until everything submitted so far is applied.
    async fn flush(&self, limit: Duration) -> bool {
        let (done, wait) = oneshot::channel();
        lock(&self.queue).1.push(done);
        let _ = self.wake.try_send(());
        matches!(tokio::time::timeout(limit, wait).await, Ok(Ok(())))
    }

    #[cfg(test)]
    fn pending(&self) -> usize {
        lock(&self.queue).0.len()
    }
}

/// Apply pending writes until the queue's owner is gone. Each round takes
/// everything pending, applies it as one batch, then answers the flushes
/// that were waiting for it.
async fn persist_loop(queue: Queue, mut wake: mpsc::Receiver<()>, apply: Apply) {
    while wake.recv().await.is_some() {
        loop {
            let (changes, flushed) = {
                let mut queue = lock(&queue);
                (queue.0.take(), std::mem::take(&mut queue.1))
            };
            if changes.is_empty() && flushed.is_empty() {
                break;
            }
            if !changes.is_empty() {
                let apply = apply.clone();
                let _ = tokio::task::spawn_blocking(move || apply(changes)).await;
            }
            for done in flushed {
                let _ = done.send(());
            }
        }
    }
}

struct Inner {
    host_id: String,
    spec: AgentSpec,
    program: PathBuf,
    path: Option<OsString>,
    store: Arc<SessionStore>,
    persist: Persist,
    state: Mutex<State>,
    lifecycle: tokio::sync::Mutex<Option<Running>>,
    /// Orders every prompt dispatch and `session/cancel` (see the module
    /// docs).
    dispatch: tokio::sync::Mutex<()>,
    /// Set once by [`AgentHost::stop`]: no launch starts or completes after.
    stopping: watch::Sender<bool>,
    restarts: Mutex<VecDeque<std::time::Instant>>,
    budgets: Mutex<Budgets>,
    /// Generations that ended on their own, and why, for the supervisor. At
    /// most one message per launched generation, so the channel stays small.
    closed: mpsc::UnboundedSender<(u64, &'static str)>,
    /// Test hook: where a cancel waits between its last check and its write.
    #[cfg(test)]
    cancel_pause: Mutex<Option<Pause>>,
}

/// "Reached" and "resume" barriers of a test pause.
#[cfg(test)]
type Pause = (Arc<tokio::sync::Barrier>, Arc<tokio::sync::Barrier>);

/// One hosted ACP agent.
#[derive(Clone)]
pub struct AgentHost {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Close the connection and clean up the owned process (see the module
/// docs for the order).
async fn dispose(peer: Peer, process: AgentProcess) {
    peer.shutdown(WRITER_GRACE).await;
    peer.wait_output_closed(OUTPUT_GRACE).await;
    process.terminate(TERM_GRACE).await;
    peer.abort_reader();
}

/// The host's single exit handler: one generation at a time, in order.
async fn supervise(host: Weak<Inner>, mut closures: mpsc::UnboundedReceiver<(u64, &'static str)>) {
    while let Some((generation, reason)) = closures.recv().await {
        let Some(inner) = host.upgrade() else { return };
        AgentHost {
            inner,
        }
        .recover(generation, reason)
        .await;
    }
}

/// Why a generation ended on its own, for its phase.
fn end_reason(closed: Option<CloseReason>) -> &'static str {
    match closed {
        Some(CloseReason::Stalled) => "the agent stopped reading its input",
        Some(CloseReason::Protocol) => "the agent broke the ACP protocol",
        _ => "the agent exited",
    }
}

/// Resolves once the leader `pid` exited (without reaping it).
async fn leader_exit(pid: Option<u32>) {
    let Some(pid) = pid else { return std::future::pending().await };
    loop {
        tokio::time::sleep(EXIT_POLL).await;
        if process::leader_exited(pid) {
            return;
        }
    }
}

fn valid_id(id: &str) -> Result<(), HostError> {
    if id.is_empty() || id.len() > MAX_ID {
        return Err(HostError::InvalidInput("invalid id".into()));
    }
    Ok(())
}

/// An existing directory, as an absolute canonical UTF-8 path.
async fn validate_cwd(cwd: &str) -> Result<String, HostError> {
    let path = Path::new(cwd);
    if !path.is_absolute() {
        return Err(HostError::InvalidInput(
            "the working directory must be an absolute path".into(),
        ));
    }
    let real = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| HostError::InvalidInput("the working directory does not exist".into()))?;
    if !tokio::fs::metadata(&real)
        .await
        .is_ok_and(|meta| meta.is_dir())
    {
        return Err(HostError::InvalidInput("the working directory is not a folder".into()));
    }
    real.to_str()
        .map(str::to_string)
        .ok_or_else(|| HostError::InvalidInput("the working directory is not valid UTF-8".into()))
}

/// Turn a `data:<mime>;base64,<data>` URL into an ACP image block.
fn image_block(url: &str) -> Result<Value, HostError> {
    let invalid = || HostError::InvalidInput("unsupported image attachment".into());
    if url.len() > MAX_IMAGE_BYTES {
        return Err(HostError::InvalidInput("an image attachment is too large".into()));
    }
    let rest = url.strip_prefix("data:").ok_or_else(invalid)?;
    let (mime, data) = rest.split_once(";base64,").ok_or_else(invalid)?;
    if !mime.starts_with("image/") || data.is_empty() {
        return Err(invalid());
    }
    Ok(json!({"type": "image", "mimeType": mime, "data": data}))
}

impl AgentHost {
    /// Create the host and launch its first generation. The host exists even
    /// when the launch fails, so its status can explain the failure and a
    /// restart can be requested.
    pub async fn start(options: HostOptions) -> (Self, Result<(), HostError>) {
        let host = Self::create(options);
        let launched = host.launch().await;
        (host, launched)
    }

    /// Create the host without launching the agent (see [`AgentHost::launch`]),
    /// so an owner can register it — and stop it — before the launch begins.
    /// Must run inside a Tokio runtime.
    pub fn create(options: HostOptions) -> Self {
        let (closed, closures) = mpsc::unbounded_channel();
        let store = Arc::new(options.store);
        let apply: Apply = {
            let store = store.clone();
            Arc::new(move |changes: Vec<Change>| store.apply(&changes))
        };
        let host = Self {
            inner: Arc::new(Inner {
                host_id: uuid::Uuid::new_v4().to_string(),
                spec: options.spec,
                program: options.program,
                path: options.path,
                store,
                persist: Persist::spawn(apply),
                state: Mutex::new(State::default()),
                lifecycle: tokio::sync::Mutex::new(None),
                dispatch: tokio::sync::Mutex::new(()),
                stopping: watch::channel(false).0,
                restarts: Mutex::new(VecDeque::new()),
                budgets: Mutex::new(Budgets::default()),
                closed,
                #[cfg(test)]
                cancel_pause: Mutex::new(None),
            }),
        };
        // Holds only a weak reference: the supervisor never keeps a host
        // alive, and ends once the host and every watcher are gone.
        tokio::spawn(supervise(Arc::downgrade(&host.inner), closures));
        host
    }

    /// Launch the first generation of a [created](AgentHost::create) host.
    pub async fn launch(&self) -> Result<(), HostError> {
        let mut slot = self.inner.lifecycle.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        self.launch_locked(&mut slot).await
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }

    /// The agent profile.
    pub fn spec(&self) -> &AgentSpec {
        &self.inner.spec
    }

    /// This host's incarnation: a fresh id for every [`AgentHost::create`],
    /// so a controller never mistakes a replacement host (whose
    /// generations and sequence numbers start over) for the one it knew.
    pub fn host_id(&self) -> &str {
        &self.inner.host_id
    }

    fn stopping(&self) -> bool {
        *self.inner.stopping.borrow()
    }

    fn budgets(&self) -> Budgets {
        *lock(&self.inner.budgets)
    }

    /// Deliver a control frame's outcome: an exchange that did not complete
    /// closes the generation (see the module docs).
    fn control_outcome(
        &self,
        generation: u64,
        peer: &Peer,
        delivered: Result<(), CallError>,
    ) -> Result<(), HostError> {
        delivered.map_err(|error| {
            if error != CallError::Closed {
                peer.abandon();
            }
            self.call_error(generation, error)
        })
    }

    /// Whether a caller that saw `expected` acted on this host in
    /// `generation`.
    fn is_current(&self, expected: Option<&Identity>, generation: u64) -> bool {
        expected.is_none_or(|expected| {
            expected
                .host_id
                .as_deref()
                .is_none_or(|host| host == self.inner.host_id)
                && expected.generation.is_none_or(|g| g == generation)
        })
    }

    /// The running generation's connection, if `expected` still holds.
    fn ready_for(
        &self,
        expected: Option<&Identity>,
    ) -> Result<(u64, Peer, AgentCapabilities), HostError> {
        let ready = self.ready()?;
        if !self.is_current(expected, ready.0) {
            return Err(HostError::Stale);
        }
        Ok(ready)
    }

    async fn launch_locked(&self, slot: &mut Option<Running>) -> Result<(), HostError> {
        if self.stopping() {
            return Err(HostError::NotReady);
        }
        let generation = self.state().begin_generation();
        let spawned = match process::spawn(
            &self.inner.program,
            &self.inner.spec.args,
            self.inner.path.as_ref(),
        ) {
            Ok(spawned) => spawned,
            Err(error) => {
                let reason = error.to_string();
                self.fail(generation, &reason);
                return Err(HostError::Start(reason));
            },
        };
        let leader = spawned.process.id();
        let inbound = Arc::new(AgentInbound {
            host: Arc::downgrade(&self.inner),
            generation,
        });
        let peer = Peer::spawn(spawned.stdout, spawned.stdin, inbound);
        let handshake = peer.request("initialize", schema::initialize_params(), Some(INIT_TIMEOUT));
        let mut stopping = self.inner.stopping.subscribe();
        let negotiated = tokio::select! {
            answer = handshake => match answer {
                Ok(result) => schema::negotiate(&result).map_err(|error| error.to_string()),
                Err(CallError::Timeout) => {
                    Err("the agent did not complete the ACP handshake in time".to_string())
                },
                Err(CallError::Rpc(error)) => Err(format!(
                    "the agent refused the ACP handshake: {}",
                    clip(&error.message, 200)
                )),
                Err(_) => Err("the agent exited during the ACP handshake".to_string()),
            },
            () = leader_exit(leader) => Err("the agent exited during the ACP handshake".to_string()),
            // A stop never waits for a slow handshake.
            _ = stopping.wait_for(|stop| *stop) => Err("hosting was stopped".to_string()),
        };
        let negotiated = match negotiated {
            Ok(negotiated) => negotiated,
            Err(reason) => {
                dispose(peer, spawned.process).await;
                self.fail(generation, &reason);
                return Err(HostError::Start(reason));
            },
        };
        let published = {
            let mut state = self.state();
            if state.generation == generation && !self.stopping() {
                state.phase = Phase::Ready;
                state.negotiated = Some(negotiated);
                state.peer = Some(peer.clone());
                state.auth_required = false;
                state.host_event();
                true
            } else {
                false
            }
        };
        if !published {
            dispose(peer, spawned.process).await;
            return Err(HostError::NotReady);
        }
        // Report the end of this generation to the supervisor — the
        // connection closed, or the leader exited while something else keeps
        // its output open. Never handled here, so this task neither holds the
        // host alive nor re-enters the lifecycle. Aborted before any reap.
        let watcher = tokio::spawn({
            let closed = self.inner.closed.clone();
            let peer = peer.clone();
            async move {
                let reason = tokio::select! {
                    reason = peer.closed() => Some(reason),
                    () = leader_exit(leader) => None,
                };
                let _ = closed.send((generation, end_reason(reason)));
            }
        });
        *slot = Some(Running {
            generation,
            peer,
            process: spawned.process,
            watcher,
        });
        Ok(())
    }

    fn fail(&self, generation: u64, reason: &str) {
        let mut state = self.state();
        if state.generation == generation {
            state.phase = if self.stopping() {
                Phase::Stopped
            } else {
                Phase::Failed {
                    reason: reason.to_string(),
                }
            };
            state.host_event();
        }
    }

    async fn teardown(&self, running: Running, phase: Phase) {
        self.state().invalidate(running.generation, phase);
        dispose(running.peer, running.process).await;
    }

    /// Generation `generation` ended on its own (exit, crash, protocol
    /// violation). Runs only on the supervisor task: tear the generation
    /// down unless a stop or restart already owns it, then relaunch while
    /// the bounded restart budget allows. A stop or manual restart during
    /// the delay wins: relaunching requires that nothing is running and the
    /// phase is still `Failed`.
    async fn recover(&self, generation: u64, reason: &str) {
        {
            let mut slot = self.inner.lifecycle.lock().await;
            let Some(running) = slot.take_if(|running| running.generation == generation) else {
                return;
            };
            self.teardown(running, Phase::Failed {
                reason: reason.into(),
            })
            .await;
        }
        while self.allow_restart() {
            tokio::time::sleep(RESTART_DELAY).await;
            let mut slot = self.inner.lifecycle.lock().await;
            let failed = matches!(self.state().phase, Phase::Failed { .. });
            if slot.is_some() || !failed || self.stopping() {
                return;
            }
            if self.launch_locked(&mut slot).await.is_ok() {
                return;
            }
        }
    }

    fn allow_restart(&self) -> bool {
        let mut restarts = lock(&self.inner.restarts);
        let now = std::time::Instant::now();
        while restarts
            .front()
            .is_some_and(|at| now.duration_since(*at) > RESTART_WINDOW)
        {
            restarts.pop_front();
        }
        if restarts.len() >= MAX_RESTARTS {
            return false;
        }
        restarts.push_back(now);
        true
    }

    /// Stop the agent and clean up its process tree, for good: a launch
    /// still negotiating is cut short and none starts afterwards. Bounded —
    /// pending session-record writes get [`FLUSH_GRACE`]. Idempotent.
    pub async fn stop(&self) {
        self.inner.stopping.send_replace(true);
        let mut slot = self.inner.lifecycle.lock().await;
        match slot.take() {
            Some(running) => {
                running.watcher.abort();
                self.teardown(running, Phase::Stopped).await;
            },
            None => {
                let mut state = self.state();
                if state.phase != Phase::Stopped {
                    state.phase = Phase::Stopped;
                    state.host_event();
                }
            },
        }
        drop(slot);
        if !self.inner.persist.flush(FLUSH_GRACE).await {
            tracing::warn!("saving the ACP session record did not finish while stopping");
        }
    }

    /// Stop (when running) and launch a fresh generation. Refused once the
    /// host was stopped.
    pub async fn restart(&self) -> Result<(), HostError> {
        if self.stopping() {
            return Err(HostError::NotReady);
        }
        let mut slot = self.inner.lifecycle.lock().await;
        if let Some(running) = slot.take() {
            running.watcher.abort();
            self.teardown(running, Phase::Starting).await;
        }
        lock(&self.inner.restarts).clear();
        self.launch_locked(&mut slot).await
    }

    /// Wait up to `limit` until every session-record write submitted so far
    /// is applied. `false` when it did not finish in time.
    pub async fn flush_store(&self, limit: Duration) -> bool {
        self.inner.persist.flush(limit).await
    }

    /// The running generation's connection. A closed connection is not
    /// ready, even before the supervisor has torn its generation down.
    fn ready(&self) -> Result<(u64, Peer, AgentCapabilities), HostError> {
        let state = self.state();
        match (&state.phase, &state.peer, &state.negotiated) {
            (Phase::Ready, Some(peer), Some(negotiated)) if !peer.is_closed() => {
                Ok((state.generation, peer.clone(), negotiated.capabilities.clone()))
            },
            _ => Err(HostError::NotReady),
        }
    }

    /// The agent signalled ACP `auth_required` in `generation`.
    fn note_auth_required(&self, generation: u64) {
        let mut state = self.state();
        if state.generation == generation && !state.auth_required {
            state.auth_required = true;
            state.host_event();
        }
    }

    fn call_error(&self, generation: u64, error: CallError) -> HostError {
        match error {
            CallError::Rpc(error) if error.code == RpcError::AUTH_REQUIRED => {
                self.note_auth_required(generation);
                HostError::AuthRequired
            },
            CallError::Rpc(error) => HostError::Agent(clip(&error.message, 300)),
            CallError::Closed => HostError::NotReady,
            CallError::Timeout => HostError::Timeout,
            CallError::Busy => HostError::Limit,
            CallError::TooLarge => HostError::InvalidInput("the message is too large".into()),
        }
    }

    /// Public description: incarnation, phase, profile, negotiated agent
    /// and features.
    pub fn info(&self) -> Value {
        info_of(&self.inner.host_id, &self.inner.spec, &self.state())
    }

    /// Atomic snapshot plus sequence watermark.
    pub fn snapshot(&self) -> Value {
        let state = self.state();
        state.snapshot(info_of(&self.inner.host_id, &self.inner.spec, &state))
    }

    /// Events after `after` in `generation`, and a receiver for later ones;
    /// `None` when the caller must resynchronize from a new snapshot.
    pub fn subscribe(&self, generation: u64, after: u64) -> Option<EventSubscription> {
        let state = self.state();
        if state.generation != generation {
            return None;
        }
        let backlog = state.log.since(after)?;
        Some((backlog, state.log.subscribe()))
    }

    fn close_session(&self, session: Option<String>, peer: &Peer, caps: &AgentCapabilities) {
        let (Some(session), true) = (session, caps.close_session) else { return };
        let peer = peer.clone();
        tokio::spawn(async move {
            let _ = peer
                .request("session/close", json!({"sessionId": session}), Some(CLOSE_TIMEOUT))
                .await;
        });
    }

    /// Create a session in host-validated absolute `cwd`.
    pub async fn new_session(
        &self,
        cwd: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, HostError> {
        let cwd = validate_cwd(cwd).await?;
        let (generation, peer, caps) = self.ready_for(expected)?;
        let result = peer
            .request("session/new", json!({"cwd": cwd, "mcpServers": []}), Some(SESSION_TIMEOUT))
            .await
            .map_err(|error| self.call_error(generation, error))?;
        let id = result["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= MAX_ID)
            .ok_or_else(|| HostError::Agent("the agent returned no session id".into()))?
            .to_string();
        let settings = SessionSettings::from_response(&result);
        let evicted = {
            let mut state = self.state();
            if state.generation != generation {
                return Err(HostError::NotReady);
            }
            if state.auth_required {
                state.auth_required = false;
                state.host_event();
            }
            if !state.transcript_room(&id) {
                drop(state);
                // Created at the agent but not admitted here: release it.
                self.close_session(Some(id), &peer, &caps);
                return Err(HostError::Capacity);
            }
            let evicted = state.admit(&id, &cwd, settings, false, false);
            state.reset_transcript(&id);
            state.session_event(&id);
            self.inner
                .persist
                .submit(|pending| pending.record(&id, &cwd, None));
            evicted
        };
        self.close_session(evicted, &peer, &caps);
        Ok(self.opened(&id, "new"))
    }

    fn opened(&self, id: &str, mode: &str) -> Value {
        let mut body = self.state().session_body(id);
        body["mode"] = json!(mode);
        body
    }

    /// The live answer for an already admitted session, if any.
    fn live_session(&self, id: &str) -> Option<Result<Value, HostError>> {
        let pending = {
            let mut state = self.state();
            let pending = state.sessions.get(id)?.pending;
            if !pending {
                state.touch(id);
            }
            pending
        };
        Some(if pending { Err(HostError::Loading) } else { Ok(self.opened(id, "live")) })
    }

    /// Reopen `id`: already admitted → live; otherwise `session/load`
    /// (history replay) when advertised, else `session/resume` (no replay).
    ///
    /// The working directory is this host's persisted one when it has one —
    /// that always wins — else the agent's own listing, canonicalized; never
    /// the caller's. The session stays pending (no prompts, no new file
    /// authority) until the agent confirms, and is revoked when it fails.
    pub async fn open_session(
        &self,
        id: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, HostError> {
        valid_id(id)?;
        let (generation, peer, caps) = self.ready_for(expected)?;
        if let Some(live) = self.live_session(id) {
            return live;
        }
        let cwd = match self.inner.store.lookup(id) {
            Some(entry) => entry.cwd,
            None => {
                let listed = self.state().listed.get(id).cloned();
                let listed = listed.ok_or(HostError::UnknownSession)?;
                validate_cwd(&listed).await?
            },
        };
        if !Path::new(&cwd).is_dir() {
            return Err(HostError::InvalidInput(
                "the session's working directory no longer exists".into(),
            ));
        }
        let (method, mode) = if caps.load_session {
            ("session/load", "load")
        } else if caps.resume_session {
            ("session/resume", "resume")
        } else {
            return Err(HostError::NotReopenable);
        };
        let replaying = mode == "load";
        let evicted = {
            let mut state = self.state();
            if state.generation != generation {
                return Err(HostError::NotReady);
            }
            if let Some(session) = state.sessions.get(id) {
                // Admitted concurrently by another open.
                let pending = session.pending;
                drop(state);
                return if pending { Err(HostError::Loading) } else { Ok(self.opened(id, "live")) };
            }
            // The reopen holds a transcript slot from now on: refuse before
            // the agent does any work rather than exceed the pool.
            if !state.transcript_room(id) {
                return Err(HostError::Capacity);
            }
            let evicted = state.admit(id, &cwd, SessionSettings::default(), true, replaying);
            if replaying {
                state.reset_transcript(id);
            } else {
                // Resume restores context without replay: keep whatever
                // this host retained, and say earlier history is missing.
                state.reserve_transcript(id, true);
            }
            state.session_event(id);
            evicted
        };
        self.close_session(evicted, &peer, &caps);
        let result = peer
            .request(
                method,
                json!({"sessionId": id, "cwd": cwd, "mcpServers": []}),
                Some(LOAD_TIMEOUT),
            )
            .await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                {
                    let mut state = self.state();
                    if state.generation == generation {
                        state.revoke(id);
                        state.session_event(id);
                    }
                }
                return Err(self.call_error(generation, error));
            },
        };
        {
            let mut state = self.state();
            if state.generation != generation
                || !state.confirm(id, SessionSettings::from_response(&result))
            {
                return Err(HostError::NotReady);
            }
            state.session_event(id);
            // Only a title signal seen while pending changes the stored
            // title (a clear included); without one the stored title stays.
            let title = state
                .sessions
                .get(id)
                .filter(|s| s.title_reported)
                .map(|s| s.title.clone());
            self.inner
                .persist
                .submit(|pending| pending.record(id, &cwd, title));
        }
        Ok(self.opened(id, mode))
    }

    /// Sessions: the agent's `session/list` when advertised (paged by its
    /// opaque cursor), otherwise this host's record. Currently admitted
    /// sessions are always included on the first page.
    pub async fn list_sessions(&self, cursor: Option<String>) -> Result<Value, HostError> {
        let ready = self.ready().ok();
        let reopen = match &ready {
            Some((_, _, caps)) if caps.load_session => "load",
            Some((_, _, caps)) if caps.resume_session => "resume",
            Some(_) => "none",
            None => "unknown",
        };
        let mut listed = None;
        let mut list_error = None;
        if let Some((generation, peer, caps)) = &ready {
            if caps.list_sessions {
                let mut params = json!({});
                if let Some(cursor) = &cursor {
                    params["cursor"] = json!(cursor);
                }
                match peer
                    .request("session/list", params, Some(LIST_TIMEOUT))
                    .await
                {
                    Ok(result) => listed = Some(self.accept_listing(*generation, &result)),
                    Err(error) => {
                        list_error = Some(self.call_error(*generation, error).to_string());
                    },
                }
            }
        }
        let first_page = cursor.is_none();
        let (mut sessions, next_cursor, source) = match listed {
            Some((sessions, next)) => (sessions, next, "agent"),
            None if !first_page => (Vec::new(), None, "host"),
            None => {
                let sessions = self
                    .inner
                    .store
                    .entries()
                    .into_iter()
                    .map(|entry| {
                        json!({
                            "sessionId": entry.session_id,
                            "cwd": entry.cwd,
                            "title": entry.title,
                            "updatedAt": entry.updated_at,
                        })
                    })
                    .collect();
                (sessions, None, "host")
            },
        };
        if first_page {
            let state = self.state();
            let mut admitted: Vec<(&String, &super::state::Session)> =
                state.sessions.iter().collect();
            admitted.sort_by(|a, b| a.0.cmp(b.0));
            for (id, session) in admitted {
                if !sessions.iter().any(|s| s["sessionId"] == **id) {
                    sessions.push(json!({
                        "sessionId": id, "cwd": session.cwd, "title": session.title,
                        "updatedAt": Value::Null,
                    }));
                }
            }
            for entry in &mut sessions {
                let id = entry["sessionId"].as_str().unwrap_or("").to_string();
                let session = state.sessions.get(&id);
                entry["live"] = json!(session.is_some_and(|s| !s.pending));
                entry["running"] = json!(session.is_some_and(|s| s.active.is_some()));
            }
        }
        Ok(json!({
            "sessions": sessions,
            "nextCursor": next_cursor,
            "source": source,
            "reopen": reopen,
            "listError": list_error,
        }))
    }

    fn accept_listing(&self, generation: u64, result: &Value) -> (Vec<Value>, Option<String>) {
        let mut out = Vec::new();
        let mut state = self.state();
        for session in result["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
        {
            let (Some(id), Some(cwd)) = (session["sessionId"].as_str(), session["cwd"].as_str())
            else {
                continue;
            };
            if id.is_empty() || id.len() > MAX_ID || !Path::new(cwd).is_absolute() {
                continue;
            }
            if state.generation == generation {
                state.remember_listed(id, cwd);
            }
            out.push(json!({
                "sessionId": id,
                "cwd": cwd,
                "title": session["title"].as_str().map(|t| clip(t, 512)),
                "updatedAt": session["updatedAt"].as_str().map(|t| clip(t, 64)),
            }));
        }
        let next = result["nextCursor"]
            .as_str()
            .filter(|c| c.len() <= 4096)
            .map(str::to_string);
        (out, next)
    }

    /// Retained history of `session`: the turn index plus a window of turns
    /// (`turn` selects one; otherwise the newest `limit` before `before`).
    /// The answer carries the event watermark it is consistent with, so a
    /// controller can apply exactly the events that follow it.
    pub fn history(
        &self,
        session: &str,
        before: Option<&str>,
        turn: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Value, HostError> {
        valid_id(session)?;
        let state = self.state();
        let Some(transcript) = state.transcript_ref(session) else {
            return Ok(json!({
                "available": false, "sessionId": session,
                "seq": state.log.last(), "generation": state.generation,
                "hostId": self.inner.host_id,
            }));
        };
        let index: Vec<Value> = transcript
            .turns
            .iter()
            .map(|t| {
                let user = t
                    .items
                    .iter()
                    .find(|i| i.kind == super::fold::ItemKind::UserMessage)
                    .map(|i| clip(&i.text, 200))
                    .unwrap_or_default();
                json!({
                    "turnId": t.id, "userText": user, "replayed": t.replayed,
                    "outcome": t.outcome, "omitted": t.omitted,
                })
            })
            .collect();
        let turns: Vec<&super::fold::Turn> = transcript.turns.iter().collect();
        let window: Vec<&super::fold::Turn> = match turn {
            Some(id) => turns.iter().copied().filter(|t| t.id == id).collect(),
            None => {
                let end = before
                    .and_then(|before| turns.iter().position(|t| t.id == before))
                    .unwrap_or(turns.len());
                let take = limit
                    .unwrap_or(DEFAULT_WINDOW_TURNS)
                    .clamp(1, MAX_WINDOW_TURNS);
                turns[end.saturating_sub(take)..end].to_vec()
            },
        };
        let has_older = window
            .first()
            .and_then(|first| turns.iter().position(|t| t.id == first.id))
            .is_some_and(|start| start > 0);
        let session_state = state.session_body(session);
        Ok(json!({
            "available": true,
            "sessionId": session,
            "truncated": transcript.truncated,
            "turns": index,
            "window": window,
            "hasOlder": has_older,
            "session": session_state,
            "seq": state.log.last(),
            "generation": state.generation,
            "hostId": self.inner.host_id,
        }))
    }

    /// Admit a prompt turn and return its id at once; a separate task sends
    /// it to the agent (unless it is cancelled first) and awaits the answer.
    pub async fn prompt(
        &self,
        session: &str,
        text: &str,
        images: &[String],
        expected: Option<&Identity>,
    ) -> Result<String, HostError> {
        valid_id(session)?;
        if text.len() > MAX_PROMPT_TEXT || images.len() > MAX_PROMPT_IMAGES {
            return Err(HostError::InvalidInput("the message is too large".into()));
        }
        if text.trim().is_empty() && images.is_empty() {
            return Err(HostError::InvalidInput("the message is empty".into()));
        }
        let (generation, peer, caps) = self.ready_for(expected)?;
        if !images.is_empty() && !caps.image {
            return Err(HostError::Unsupported("this agent does not accept images".into()));
        }
        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(json!({"type": "text", "text": text}));
        }
        for image in images {
            blocks.push(image_block(image)?);
        }
        let turn = uuid::Uuid::new_v4().to_string();
        {
            let mut state = self.state();
            if state.generation != generation {
                return Err(HostError::NotReady);
            }
            let Some(entry) = state.sessions.get(session) else {
                return Err(HostError::UnknownSession);
            };
            if entry.pending {
                return Err(HostError::Loading);
            }
            if entry.active.is_some() {
                return Err(HostError::Busy);
            }
            // A running turn holds its transcript; get one first or refuse.
            if !state.reserve_transcript(session, false) {
                return Err(HostError::Capacity);
            }
            if let Some(entry) = state.sessions.get_mut(session) {
                entry.active = Some(ActiveTurn::new(turn.clone()));
            }
            state.touch(session);
            if let Some(transcript) = state.transcript_mut(session) {
                transcript.begin_turn(&turn, text, images);
            }
            let body = json!({
                "type": "turn_started", "sessionId": session, "turnId": turn,
                "userText": clip(text, MAX_PROMPT_TEXT), "imageCount": images.len(),
            });
            state.event(body);
        }
        // The dispatch budget starts now, before the lane (see the module
        // docs).
        let deadline = Instant::now() + self.budgets().dispatch;
        let host = self.clone();
        let (session_id, turn_id) = (session.to_string(), turn.clone());
        tokio::spawn(async move {
            let call = {
                let Ok(_lane) = tokio::time::timeout_at(deadline, host.inner.dispatch.lock()).await
                else {
                    // Nothing was queued: the agent never sees this turn.
                    host.finish_turn(generation, &session_id, &turn_id, TurnOutcome::Failed {
                        message: "the agent is not accepting input; the message was not sent"
                            .to_string(),
                    });
                    return;
                };
                if !host.mark_dispatched(generation, &session_id, &turn_id) {
                    // Cancelled before it was sent (or the generation is
                    // gone): the agent never sees this turn.
                    host.finish_turn(generation, &session_id, &turn_id, TurnOutcome::Cancelled);
                    return;
                }
                peer.send_request(
                    "session/prompt",
                    json!({"sessionId": session_id, "prompt": blocks}),
                    Some(deadline),
                )
                .await
            };
            // Only a prompt the agent received in full waits for its answer
            // without limit.
            let answer = match call {
                Ok(call) => call.wait(None).await,
                Err(error) => Err(error),
            };
            let outcome = match answer {
                Ok(result) => TurnOutcome::from_prompt_result(&result),
                Err(CallError::Closed) => TurnOutcome::AgentExited,
                Err(CallError::Rpc(error)) if error.code == RpcError::AUTH_REQUIRED => {
                    host.note_auth_required(generation);
                    TurnOutcome::Failed {
                        message: HostError::AuthRequired.to_string(),
                    }
                },
                Err(CallError::Rpc(error)) => TurnOutcome::Failed {
                    message: clip(&error.message, 300),
                },
                // Possibly half-written: the peer closed the connection.
                Err(CallError::Timeout) if peer.is_closed() => TurnOutcome::Failed {
                    message: "the agent stopped reading its input; the connection was closed"
                        .to_string(),
                },
                Err(CallError::Timeout) => TurnOutcome::Failed {
                    message: "the agent is not reading its input; the message was not sent"
                        .to_string(),
                },
                Err(error) => TurnOutcome::Failed {
                    message: error.to_string(),
                },
            };
            host.finish_turn(generation, &session_id, &turn_id, outcome);
        });
        Ok(turn)
    }

    /// Mark `turn` as dispatched unless it was cancelled or ended. Called
    /// with the dispatch lane held.
    fn mark_dispatched(&self, generation: u64, session: &str, turn: &str) -> bool {
        let mut state = self.state();
        if state.generation != generation {
            return false;
        }
        let Some(active) = state
            .sessions
            .get_mut(session)
            .and_then(|entry| entry.active.as_mut())
            .filter(|active| active.id == turn)
        else {
            return false;
        };
        if active.cancel_requested {
            return false;
        }
        active.dispatched = true;
        state.session_event(session);
        true
    }

    fn finish_turn(&self, generation: u64, session: &str, turn: &str, outcome: TurnOutcome) {
        let (leftovers, peer) = {
            let mut state = self.state();
            if state.generation != generation {
                return;
            }
            let leftovers = state.end_turn(session, turn, outcome).unwrap_or_default();
            (leftovers, state.peer.clone())
        };
        if let Some(peer) = peer {
            for permission in leftovers {
                peer.respond_now_until(
                    permission.acp_id,
                    Ok(cancelled_outcome()),
                    Instant::now() + self.budgets().control,
                );
            }
        }
    }

    /// Whether `turn` of `session` is still running and was dispatched in
    /// `generation`.
    fn dispatched(&self, generation: u64, session: &str, turn: &str) -> bool {
        let state = self.state();
        state.generation == generation
            && state
                .sessions
                .get(session)
                .and_then(|entry| entry.active.as_ref())
                .is_some_and(|active| active.id == turn && active.dispatched)
    }

    /// Cancel the running turn of `session`. With `expected_turn` (and
    /// `expected`) only that turn is cancelled; a different or finished turn
    /// — or another host or generation — is left alone. Pending permissions
    /// of that turn are answered `cancelled`; `session/cancel` is written
    /// under the dispatch lane only while that turn still runs, so it can
    /// never follow a newer turn's prompt (see the module docs). The turn
    /// ends when the agent answers. `true` when a turn was (or already is)
    /// being cancelled.
    ///
    /// One control budget covers the lane and every write. If it runs out
    /// the generation is closed (see the module docs) and this fails; the
    /// turn then ends with the generation, so a retry reports `false`.
    pub async fn cancel(
        &self,
        session: &str,
        expected_turn: Option<&str>,
        expected: Option<&Identity>,
    ) -> Result<bool, HostError> {
        valid_id(session)?;
        let deadline = Instant::now() + self.budgets().control;
        let (generation, peer, _) = self.ready()?;
        if !self.is_current(expected, generation) {
            return Ok(false);
        }
        let turn = {
            let mut state = self.state();
            if state.generation != generation {
                return Ok(false);
            }
            let Some(entry) = state.sessions.get_mut(session) else {
                return Err(HostError::UnknownSession);
            };
            let Some(active) = entry.active.as_mut() else { return Ok(false) };
            if expected_turn.is_some_and(|expected| expected != active.id) {
                return Ok(false);
            }
            if active.cancel_requested {
                return Ok(true);
            }
            // Set before the lane: a dispatch that has not checked yet then
            // never sends the prompt.
            active.cancel_requested = true;
            let turn = active.id.clone();
            state.session_event(session);
            turn
        };
        let mut incomplete = peer.abandon_on_drop();
        let Ok(_lane) = tokio::time::timeout_at(deadline, self.inner.dispatch.lock()).await else {
            // The lane is held by a write the agent is not reading: the
            // cancel cannot be delivered, and the turn may not be left
            // looking cancelled on a live connection.
            peer.abandon();
            return Err(HostError::Timeout);
        };
        // This turn's pending permission requests are answered `cancelled`
        // (the spec requires it), within the same budget.
        let withdrawn: Vec<Permission> = {
            let mut state = self.state();
            if state.generation != generation {
                incomplete.disarm();
                return Ok(true);
            }
            let handles: Vec<String> = state
                .permissions
                .iter()
                .filter(|(_, p)| p.session == session && p.turn == turn)
                .map(|(handle, _)| handle.clone())
                .collect();
            handles
                .iter()
                .filter_map(|handle| state.resolve(handle, "cancelled"))
                .collect()
        };
        for permission in withdrawn {
            let delivered = peer
                .respond(permission.acp_id, Ok(cancelled_outcome()), deadline)
                .await;
            self.control_outcome(generation, &peer, delivered)?;
        }
        if !self.dispatched(generation, session, &turn) {
            // Never sent (it ends as cancelled when its dispatch runs), or
            // already over: nothing for the agent to cancel.
            incomplete.disarm();
            return Ok(true);
        }
        #[cfg(test)]
        self.pause_before_cancel().await;
        let delivered = peer
            .notify("session/cancel", json!({"sessionId": session}), deadline)
            .await;
        self.control_outcome(generation, &peer, delivered)?;
        incomplete.disarm();
        Ok(true)
    }

    #[cfg(test)]
    async fn pause_before_cancel(&self) {
        let pause = lock(&self.inner.cancel_pause).clone();
        if let Some((reached, resume)) = pause {
            reached.wait().await;
            resume.wait().await;
        }
    }

    /// Answer permission `handle` with `option_id`. An option the request did
    /// not offer is rejected and the request stays pending; the first valid
    /// answer wins and later ones see [`HostError::AlreadyResolved`]. An
    /// answer that cannot be delivered within the control budget closes the
    /// generation (see the module docs): a consumed request is never left
    /// unanswered on a live connection.
    pub async fn answer_permission(&self, handle: &str, option_id: &str) -> Result<(), HostError> {
        valid_id(handle)?;
        let deadline = Instant::now() + self.budgets().control;
        let (permission, peer, generation) = {
            let mut state = self.state();
            let Some(pending) = state.permissions.get(handle) else {
                return Err(if state.was_resolved(handle) {
                    HostError::AlreadyResolved
                } else {
                    HostError::UnknownPermission
                });
            };
            if !pending
                .options
                .iter()
                .any(|option| option.option_id == option_id)
            {
                return Err(HostError::InvalidOption);
            }
            let peer = state.peer.clone().ok_or(HostError::NotReady)?;
            let generation = state.generation;
            let permission = state
                .resolve(handle, "selected")
                .ok_or(HostError::AlreadyResolved)?;
            (permission, peer, generation)
        };
        let mut incomplete = peer.abandon_on_drop();
        let delivered = peer
            .respond(
                permission.acp_id,
                Ok(json!({"outcome": {"outcome": "selected", "optionId": option_id}})),
                deadline,
            )
            .await;
        let result = self.control_outcome(generation, &peer, delivered);
        incomplete.disarm();
        result
    }

    /// The settings of a usable (admitted, confirmed) session.
    fn usable_settings(&self, session: &str) -> Result<SessionSettings, HostError> {
        let state = self.state();
        let entry = state
            .sessions
            .get(session)
            .ok_or(HostError::UnknownSession)?;
        if entry.pending {
            return Err(HostError::Loading);
        }
        Ok(entry.settings.clone())
    }

    /// Set a select configuration option to one of its advertised values.
    pub async fn set_config_option(
        &self,
        session: &str,
        config: &str,
        value: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, HostError> {
        valid_id(session)?;
        let (generation, peer, _) = self.ready_for(expected)?;
        if !self
            .usable_settings(session)?
            .allows_config_value(config, value)
        {
            return Err(HostError::InvalidInput("that value is not offered".into()));
        }
        let result = peer
            .request(
                "session/set_config_option",
                json!({"sessionId": session, "configId": config, "value": value}),
                Some(CONFIG_TIMEOUT),
            )
            .await
            .map_err(|error| self.call_error(generation, error))?;
        let mut state = self.state();
        if state.generation != generation {
            return Err(HostError::NotReady);
        }
        if let Some(entry) = state.sessions.get_mut(session) {
            entry.settings.config_options = schema::select_options(&result["configOptions"]);
        }
        state.session_event(session);
        Ok(state.session_body(session))
    }

    /// Switch the legacy session mode to an advertised one.
    pub async fn set_mode(
        &self,
        session: &str,
        mode: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, HostError> {
        valid_id(session)?;
        let (generation, peer, _) = self.ready_for(expected)?;
        if !self.usable_settings(session)?.allows_mode(mode) {
            return Err(HostError::InvalidInput("that mode is not offered".into()));
        }
        peer.request(
            "session/set_mode",
            json!({"sessionId": session, "modeId": mode}),
            Some(CONFIG_TIMEOUT),
        )
        .await
        .map_err(|error| self.call_error(generation, error))?;
        let mut state = self.state();
        if state.generation != generation {
            return Err(HostError::NotReady);
        }
        if let Some(entry) = state.sessions.get_mut(session) {
            entry.settings.set_current_mode(mode);
        }
        state.session_event(session);
        Ok(state.session_body(session))
    }

    /// The host-authorized working directory of `session`: one this source
    /// admitted before (persisted, never rebound — this earlier authority
    /// stays), or a confirmed admission of the current generation. A reopen
    /// still pending grants no *new* authority.
    pub fn session_dir(&self, session: &str) -> Option<String> {
        if let Some(entry) = self.inner.store.lookup(session) {
            return Some(entry.cwd);
        }
        self.state().authorized_cwd(session).map(str::to_string)
    }

    fn on_update(&self, generation: u64, params: Option<Value>) {
        let Some(params) = params else { return };
        let Some(session) = params["sessionId"].as_str().map(str::to_string) else { return };
        let update = &params["update"];
        let mut state = self.state();
        if state.generation != generation {
            return;
        }
        let Some(entry) = state.sessions.get_mut(&session) else { return };
        let (pending, replaying) = (entry.pending, entry.replaying);
        let turn = if pending { None } else { entry.active.as_ref().map(|t| t.id.clone()) };
        if let Some(signal) = session_signal(update) {
            match signal {
                SessionSignal::ConfigOptions(options) => {
                    entry.settings.config_options = options;
                },
                SessionSignal::Mode(mode) => entry.settings.set_current_mode(&mode),
                SessionSignal::Title(Some(title)) => {
                    entry.title.clone_from(&title);
                    entry.title_reported = true;
                    // Queued under the state lock, after the admission's
                    // own record; a reopen still pending records its
                    // title — or its explicit clear — when it is confirmed.
                    if !pending {
                        self.inner
                            .persist
                            .submit(|queue| queue.title(&session, title));
                    }
                },
                SessionSignal::Title(None) => return,
                SessionSignal::Usage {
                    used,
                    size,
                } => entry.usage = Some((used, size)),
            }
            if !pending {
                state.session_event(&session);
            }
            return;
        }
        // A resume restores context without history: content that arrives
        // before it is confirmed is not part of this view.
        if pending && !replaying {
            return;
        }
        // Content outside a turn with no transcript room is dropped, and the
        // transcript then says its history is incomplete.
        if !state.reserve_transcript(&session, false) {
            return;
        }
        let changes = match state.transcript_mut(&session) {
            Some(transcript) => transcript.apply(turn.as_deref(), update),
            None => return,
        };
        if changes.is_empty() || pending {
            return;
        }
        let body = json!({
            "type": "update", "sessionId": session, "turnId": turn, "update": update,
        });
        state.event(body);
    }

    fn on_permission(&self, generation: u64, peer: &Peer, id: RequestId, params: Option<Value>) {
        let Some((session, tool_call, options)) =
            params.as_ref().and_then(schema::permission_request)
        else {
            peer.respond_now(
                id,
                Err(RpcError::new(RpcError::INVALID_PARAMS, "malformed permission request")),
            );
            return;
        };
        let mut state = self.state();
        if state.generation != generation {
            drop(state);
            peer.respond_now_until(
                id,
                Ok(cancelled_outcome()),
                Instant::now() + self.budgets().control,
            );
            return;
        }
        let known = state
            .sessions
            .get(&session)
            .map(|entry| entry.active.clone());
        let Some(turn) = known else {
            drop(state);
            peer.respond_now(id, Err(RpcError::new(RpcError::INVALID_PARAMS, "unknown session")));
            return;
        };
        let Some(turn) = turn.filter(|turn| !turn.cancel_requested) else {
            drop(state);
            // Outside a live turn, or after cancellation: the spec requires
            // `cancelled` for a cancelled turn.
            peer.respond_now_until(
                id,
                Ok(cancelled_outcome()),
                Instant::now() + self.budgets().control,
            );
            return;
        };
        if state.permissions.len() >= MAX_PERMISSIONS {
            drop(state);
            peer.respond_now_until(
                id,
                Ok(cancelled_outcome()),
                Instant::now() + self.budgets().control,
            );
            return;
        }
        let handle = uuid::Uuid::new_v4().to_string();
        let permission = Permission {
            session,
            turn: turn.id,
            acp_id: id,
            options,
            tool_call,
        };
        let body = State::permission_body(&handle, &permission);
        state.permissions.insert(handle, permission);
        state.event(body);
    }
}

fn cancelled_outcome() -> Value {
    json!({"outcome": {"outcome": "cancelled"}})
}

fn info_of(host_id: &str, spec: &AgentSpec, state: &State) -> Value {
    let negotiated = state.negotiated.as_ref();
    json!({
        "api": 1,
        "hostId": host_id,
        "hostingSupported": process::hosting_supported(),
        "generation": state.generation,
        "phase": state.phase,
        "authRequired": state.auth_required,
        "profile": {"id": spec.profile_id, "displayName": spec.display_name},
        "protocolVersion": negotiated.map(|n| n.protocol_version),
        "agent": negotiated.map(|n| &n.agent),
        "capabilities": negotiated.map(|n| &n.capabilities),
        "authMethods": negotiated.map(|n| &n.auth_methods),
    })
}

/// Routes the agent's requests and notifications into the host. Holds the
/// generation it was created for; anything arriving for another one is
/// ignored (or answered `cancelled`).
struct AgentInbound {
    host: Weak<Inner>,
    generation: u64,
}

impl AgentInbound {
    fn host(&self) -> Option<AgentHost> {
        self.host.upgrade().map(|inner| AgentHost {
            inner,
        })
    }
}

impl Inbound for AgentInbound {
    fn notification(&self, _peer: &Peer, method: &str, params: Option<Value>) {
        if method == "session/update" {
            if let Some(host) = self.host() {
                host.on_update(self.generation, params);
            }
        }
    }

    fn request(&self, peer: &Peer, id: RequestId, method: &str, params: Option<Value>) {
        match (method, self.host()) {
            ("session/request_permission", Some(host)) => {
                host.on_permission(self.generation, peer, id, params);
            },
            // File system, terminal and elicitation methods are not
            // advertised and not implemented; nothing is read or run.
            _ => peer.respond_now(
                id,
                Err(RpcError::new(
                    RpcError::METHOD_NOT_FOUND,
                    "method not supported by this client",
                )),
            ),
        }
    }
}

#[async_trait::async_trait]
impl crate::file_links::SessionDirResolver for AgentHost {
    async fn session_dir(&self, session: &str) -> anyhow::Result<Option<String>> {
        Ok(AgentHost::session_dir(self, session))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Condvar;

    use super::*;
    use crate::acp::store::SourceId;

    const WAIT: Duration = Duration::from_secs(15);

    /// The example agent Cargo builds next to the test binaries.
    fn mock_binary() -> PathBuf {
        let exe = std::env::current_exe().expect("test executable");
        let dir = exe
            .parent()
            .and_then(|deps| deps.parent())
            .expect("target dir");
        let mock = dir.join("examples").join("mock_acp_agent");
        assert!(
            mock.is_file(),
            "{} is missing: run the package tests (`cargo test -p pocket-codex-host-svc`), which \
             build examples",
            mock.display()
        );
        mock
    }

    async fn mock_host() -> AgentHost {
        let program = mock_binary();
        let spec = AgentSpec {
            profile_id: "custom-mock".into(),
            display_name: "Mock".into(),
            program: program.to_string_lossy().into_owned(),
            args: vec!["--profile".into(), "alpha".into()],
        };
        start_mock(program, spec).await
    }

    async fn start_mock(program: PathBuf, spec: AgentSpec) -> AgentHost {
        let store = SessionStore::memory(SourceId::with_key(&"k".repeat(32), &program, &spec.args));
        let (host, launched) = AgentHost::start(HostOptions {
            spec,
            program,
            path: None,
            store,
        })
        .await;
        launched.expect("launch");
        host
    }

    async fn eventually(mut check: impl FnMut() -> bool, what: &str) {
        let deadline = Instant::now() + WAIT;
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn outcome(host: &AgentHost, session: &str, turn: &str) -> Option<TurnOutcome> {
        host.state()
            .outcomes_of(session, turn)
            .first()
            .map(|outcome| (*outcome).clone())
    }

    /// The race the dispatch lane closes: cancel A passes its last check
    /// (A running and dispatched), A then ends on its own and B is admitted
    /// and dispatched, and only then is the cancel written. Without one lane
    /// for dispatch and cancel, the session-wide `session/cancel` lands after
    /// B's prompt and cancels B.
    #[tokio::test]
    async fn a_cancel_for_an_ended_turn_never_reaches_the_next_one() {
        let host = mock_host().await;
        let cwd = tempfile::tempdir().expect("tempdir");
        let opened = host
            .new_session(&cwd.path().to_string_lossy(), None)
            .await
            .expect("session");
        let session = opened["sessionId"].as_str().expect("id").to_string();
        let a = host
            .prompt(&session, "linger", &[], None)
            .await
            .expect("turn a");
        eventually(
            || {
                let generation = host.state().generation;
                host.dispatched(generation, &session, &a)
            },
            "a dispatched",
        )
        .await;
        let reached = Arc::new(tokio::sync::Barrier::new(2));
        let resume = Arc::new(tokio::sync::Barrier::new(2));
        *lock(&host.inner.cancel_pause) = Some((reached.clone(), resume.clone()));
        let canceller = tokio::spawn({
            let (host, session, a) = (host.clone(), session.clone(), a.clone());
            async move { host.cancel(&session, Some(&a), None).await }
        });
        reached.wait().await;
        // A ends on its own while the cancel is between its check and its
        // write; B is admitted and its dispatch starts.
        eventually(|| outcome(&host, &session, &a).is_some(), "a ended").await;
        let b = host
            .prompt(&session, "probe", &[], None)
            .await
            .expect("turn b");
        tokio::time::sleep(Duration::from_millis(150)).await;
        resume.wait().await;
        assert_eq!(canceller.await.expect("join"), Ok(true));
        eventually(|| outcome(&host, &session, &b).is_some(), "b ended").await;
        assert_eq!(
            outcome(&host, &session, &b),
            Some(TurnOutcome::Stopped {
                stop_reason: "end_turn".into()
            }),
            "the cancel meant for A must not cancel B"
        );
        host.stop().await;
    }

    // ---- Delivery budgets against an agent that stops reading ----------
    //
    // The `deaf` script asks for a permission and then never reads its input
    // again, while the process stays alive. A prompt far larger than a pipe
    // buffer then blocks the writer in the middle of its frame while the
    // queue still has room. Budgets are shortened per test; the production
    // ones are unchanged.

    /// A host with a live permission request from an agent that stopped
    /// reading, and that request's handle.
    async fn deaf_host() -> (AgentHost, String, String, Peer, tempfile::TempDir) {
        let host = mock_host().await;
        let cwd = tempfile::tempdir().expect("tempdir");
        let new = |host: AgentHost, cwd: String| async move {
            host.new_session(&cwd, None).await.expect("session")["sessionId"]
                .as_str()
                .expect("id")
                .to_string()
        };
        let path = cwd.path().to_string_lossy().into_owned();
        let deaf = new(host.clone(), path.clone()).await;
        let other = new(host.clone(), path).await;
        host.prompt(&deaf, "deaf", &[], None).await.expect("turn");
        eventually(|| !host.state().permissions.is_empty(), "the agent's permission request").await;
        let handle = host
            .state()
            .permissions
            .keys()
            .next()
            .cloned()
            .expect("handle");
        let peer = host.state().peer.clone().expect("peer");
        (host, other, handle, peer, cwd)
    }

    fn budgets(host: &AgentHost, dispatch: Duration, control: Duration) {
        *lock(&host.inner.budgets) = Budgets {
            dispatch,
            control,
        };
    }

    /// A prompt the agent does not read: its delivery (not just its queue
    /// admission) is bounded, it fails truthfully, and the possibly
    /// half-written connection is closed — that generation ends.
    #[tokio::test]
    async fn a_prompt_the_agent_never_reads_fails_boundedly_and_ends_the_generation() {
        let (host, other, _, peer, _cwd) = deaf_host().await;
        budgets(&host, Duration::from_millis(300), Duration::from_millis(300));
        let started = Instant::now();
        let big = "x".repeat(1024 * 1024);
        let turn = host
            .prompt(&other, &big, &[], None)
            .await
            .expect("admitted");
        eventually(|| outcome(&host, &other, &turn).is_some() || peer.is_closed(), "bounded").await;
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(peer.is_closed(), "a partly written prompt closes the connection");
        eventually(
            || {
                let state = host.state();
                state.phase != Phase::Ready || state.peer.is_none()
            },
            "the generation ends",
        )
        .await;
        host.stop().await;
    }

    /// The writer is stalled while the queue has room; a permission answer
    /// cannot be delivered in time. The request was consumed, so the
    /// generation is closed rather than left waiting on a live connection,
    /// and a retry cannot pretend to answer it.
    #[tokio::test]
    async fn an_undeliverable_permission_answer_ends_the_generation() {
        let (host, other, handle, peer, _cwd) = deaf_host().await;
        // A long dispatch budget: the stalled prompt keeps the writer busy.
        budgets(&host, WAIT, Duration::from_millis(300));
        host.prompt(&other, &"x".repeat(1024 * 1024), &[], None)
            .await
            .expect("admitted");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = Instant::now();
        assert_eq!(host.answer_permission(&handle, "allow").await, Err(HostError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(peer.is_closed(), "no consumed request stays unanswered on a live connection");
        assert!(host.answer_permission(&handle, "allow").await.is_err(), "first answer wins");
        host.stop().await;
    }

    /// The dispatch lane is held by a prompt the agent does not read. A
    /// cancel's budget includes waiting for the lane: it fails in time,
    /// ends the generation, and a retry reports that nothing runs any more
    /// instead of a cancel that never happened.
    #[tokio::test]
    async fn a_cancel_behind_a_stalled_lane_fails_in_time_and_a_retry_is_truthful() {
        let (host, other, _, peer, _cwd) = deaf_host().await;
        budgets(&host, WAIT, Duration::from_millis(300));
        let stalled = host
            .prompt(&other, &"x".repeat(1024 * 1024), &[], None)
            .await
            .expect("admitted");
        eventually(
            || {
                let generation = host.state().generation;
                host.dispatched(generation, &other, &stalled)
            },
            "the big prompt holds the lane",
        )
        .await;
        let started = Instant::now();
        assert_eq!(host.cancel(&other, Some(&stalled), None).await, Err(HostError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(peer.is_closed());
        let retried = host.cancel(&other, Some(&stalled), None).await;
        assert!(
            matches!(retried, Ok(false) | Err(HostError::NotReady)),
            "a retry is not a silent success: {retried:?}"
        );
        host.stop().await;
    }

    #[tokio::test]
    async fn dropping_a_consumed_permission_answer_ends_the_generation() {
        let (host, _other, handle, peer, _cwd) = deaf_host().await;
        budgets(&host, WAIT, WAIT);
        let mut stalled = Box::pin(peer.notify(
            "fixture/block",
            json!("x".repeat(1024 * 1024)),
            Instant::now() + WAIT,
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut stalled)
                .await
                .is_err(),
            "writer must really be blocked before the answer"
        );
        let mut answer = Box::pin(host.answer_permission(&handle, "allow"));
        assert!(tokio::time::timeout(Duration::from_millis(50), &mut answer)
            .await
            .is_err());
        assert!(host.state().was_resolved(&handle), "actual permission was consumed");
        drop(answer);
        assert!(peer.is_closed());
        drop(stalled);
        host.stop().await;
    }

    #[tokio::test]
    async fn dropping_cancel_while_waiting_for_the_lane_ends_the_generation() {
        let (host, other, _, peer, _cwd) = deaf_host().await;
        budgets(&host, WAIT, WAIT);
        let turn = host
            .prompt(&other, &"x".repeat(1024 * 1024), &[], None)
            .await
            .expect("prompt");
        eventually(
            || {
                let generation = host.state().generation;
                host.dispatched(generation, &other, &turn)
            },
            "dispatch owns lane",
        )
        .await;
        let mut cancel = Box::pin(host.cancel(&other, Some(&turn), None));
        assert!(tokio::time::timeout(Duration::from_millis(50), &mut cancel)
            .await
            .is_err());
        assert!(
            host.state().sessions[&other]
                .active
                .as_ref()
                .expect("active")
                .cancel_requested
        );
        drop(cancel);
        assert!(peer.is_closed());
        host.stop().await;
    }

    #[tokio::test]
    async fn automatic_permission_cancellation_has_an_owned_write_deadline() {
        let (host, other, _, peer, _cwd) = deaf_host().await;
        budgets(&host, WAIT, Duration::from_millis(50));
        // No active turn in this admitted session: production on_permission
        // automatically replies cancelled, using the agent's legal string id.
        let generation = host.state().generation;
        host.on_permission(
            generation,
            &peer,
            RequestId::Str("x".repeat(128 * 1024)),
            Some(json!({
                "sessionId": other,
                "toolCall": {"toolCallId":"late", "title":"Late request"},
                "options": [{"optionId":"allow", "name":"Allow", "kind":"allow_once"}]
            })),
        );
        eventually(|| peer.is_closed(), "automatic response deadline").await;
        host.stop().await;
    }

    /// Stall the writer of session records while titles flood in: pending
    /// work stays one change, a flush gives up in time, and once the store
    /// answers the newest title is what was written.
    #[tokio::test]
    async fn a_stalled_store_keeps_titles_coalesced_and_flushes_boundedly() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let applied: Arc<Mutex<Vec<Change>>> = Arc::default();
        let apply: Apply = {
            let (gate, applied) = (gate.clone(), applied.clone());
            Arc::new(move |changes: Vec<Change>| {
                let (open, wake) = &*gate;
                let mut open = lock(open);
                while !*open {
                    open = wake.wait(open).unwrap_or_else(|poison| poison.into_inner());
                }
                lock(&applied).extend(changes);
            })
        };
        let persist = Persist::spawn(apply);
        persist.submit(|pending| pending.record("s", "/w", None));
        // Let the worker take the admission and block on the store.
        tokio::time::sleep(Duration::from_millis(50)).await;
        for n in 0..10_000 {
            persist.submit(|pending| pending.title("s", Some(format!("v{n}"))));
        }
        assert_eq!(persist.pending(), 1, "titles coalesce while the store is stalled");
        let started = Instant::now();
        assert!(!persist.flush(Duration::from_millis(100)).await, "a stalled flush gives up");
        assert!(started.elapsed() < Duration::from_secs(2));
        {
            let (open, wake) = &*gate;
            *lock(open) = true;
            wake.notify_all();
        }
        assert!(persist.flush(WAIT).await);
        let applied = lock(&applied);
        assert_eq!(applied.len(), 2, "the admission, then one coalesced title");
        assert_eq!(applied[0].cwd.as_deref(), Some("/w"));
        assert_eq!(applied[1].title, Some(Some("v9999".to_string())));
    }
}

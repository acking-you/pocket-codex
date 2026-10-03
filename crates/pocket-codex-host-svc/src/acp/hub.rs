//! [`AcpHub`]: the only ACP client of one agent process, multiplexing its
//! sessions to many controllers (TRD §4.2.5).
//!
//! All hub state lives behind one synchronous mutex that is never held
//! across an `.await`. Mutations and the notifications they cause are
//! enqueued under the same lock, so every connection sees one session's
//! notifications in fold order and an attach snapshot is ordered against
//! them by `seq` (T14).
//!
//! ```text
//!   agent stdout ─► AgentPeer read loop ─► on_agent (inbound.rs) ─┐
//!   controller   ─► HubConnection::handle ─► ops.rs ──────────────┤ lock(HubState)
//!   supervisor   ─► launch_once / on_exit (this file) ────────────┘    │
//!                                                     conn tx (try_send, cap 4096)
//! ```

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use pocket_codex_core::acp::{
    methods,
    pcx::{
        notifications, AgentIdentity, AuthState, HubMeta, PcxCaps, ProcessState, HUB_META_VERSION,
    },
    rpc::{RequestId, RpcMessage},
    ConfigOption, Implementation, InitializeResponse, SessionInfo, PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::{
    auth::{self, GatewayAuth, TerminalLauncher},
    defaults::Defaults,
    error::AcpError,
    launch::{AgentConnector, ChildHandle, LaunchSpec},
    peer::{AgentPeer, PeerExit},
    pending::Pending,
    process::{backoff, FailureLog},
    session::HubSession,
};

/// What to launch and how to log in through a gateway; re-read on every
/// start and restart, so settings changes (D20 gateway, D21 flags) apply
/// after `restart()`.
pub type LaunchProvider =
    Arc<dyn Fn() -> Result<(LaunchSpec, Option<GatewayAuth>), AcpError> + Send + Sync>;

/// Outbound queue of one controller connection.
pub const CONNECTION_QUEUE: usize = 4096;
/// Deadline of `initialize` to the agent.
pub(super) const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long `start` waits for the first auth detection.
const AUTH_DETECT_WAIT: Duration = Duration::from_secs(15);
/// Grace period before the process tree is signalled.
const TERMINATE_GRACE: Duration = Duration::from_secs(3);
/// Idle sessions are closed after this long.
pub(super) const IDLE_CLOSE_AFTER: Duration = Duration::from_secs(600);
/// Idle check interval.
const IDLE_CHECK_EVERY: Duration = Duration::from_secs(60);

/// Hub construction options.
pub struct HubOptions {
    /// Instance name.
    pub instance: String,
    /// Production: `install::resolve_launch` plus the agent's gateway
    /// settings; tests return fixed values.
    pub launch: LaunchProvider,
    /// `<state_dir>/acp`; holds `sessions/<instance>.json`,
    /// `defaults/<instance>.json`, `probe/` and `run/`.
    pub state_dir: PathBuf,
    /// Agent stderr log.
    pub log_file: Option<PathBuf>,
    /// Starts the agent.
    pub connector: Arc<dyn AgentConnector>,
    /// Opens terminals for terminal login (desktop only).
    pub terminal: Option<Arc<dyn TerminalLauncher>>,
}

/// Snapshot of a hub for hosting status.
#[derive(Clone, Debug, PartialEq)]
pub struct HubInfo {
    /// Agent id.
    pub agent_id: String,
    /// Agent display name.
    pub agent_name: String,
    /// Installed version.
    pub agent_version: Option<String>,
    /// `agentInfo` from `initialize`.
    pub agent_info: Option<Implementation>,
    /// The catalog pins this version.
    pub pinned: bool,
    /// Negotiated capabilities.
    pub caps: PcxCaps,
    /// Authentication state.
    pub auth: AuthState,
    /// Process state.
    pub process: ProcessState,
    /// Agent process id.
    pub pid: Option<u32>,
    /// Agent program.
    pub program: PathBuf,
}

/// One running agent process.
pub(super) struct AgentRun {
    pub(super) id: u64,
    pub(super) peer: Arc<AgentPeer>,
    pub(super) child: Option<ChildHandle>,
    pub(super) spec: LaunchSpec,
    pub(super) gateway: Option<GatewayAuth>,
    pub(super) init: Option<InitializeResponse>,
}

/// One controller connection.
pub(super) struct Conn {
    pub(super) tx: mpsc::Sender<RpcMessage>,
    pub(super) closed: CancellationToken,
    pub(super) initialized: bool,
    pub(super) form: bool,
    pub(super) url: bool,
    pub(super) next_request: i64,
    pub(super) outgoing: HashMap<RequestId, String>,
}

/// Everything behind the hub lock.
pub(super) struct HubState {
    pub(super) process: ProcessState,
    pub(super) run: Option<AgentRun>,
    pub(super) next_run: u64,
    pub(super) launch_gen: u64,
    pub(super) failures: FailureLog,
    pub(super) last_spec: LaunchSpec,
    pub(super) agent_info: Option<Implementation>,
    pub(super) caps: PcxCaps,
    pub(super) auth: AuthState,
    /// Persistent note shown with the auth state (gateway without method).
    pub(super) auth_note: Option<String>,
    pub(super) default_config_options: Vec<ConfigOption>,
    pub(super) defaults: Defaults,
    pub(super) sessions: HashMap<String, HubSession>,
    pub(super) created: Vec<SessionInfo>,
    pub(super) list_cache: Option<(tokio::time::Instant, Vec<SessionInfo>)>,
    pub(super) list_refreshing: bool,
    pub(super) list_notify_changed: bool,
    pub(super) list_waiters: Vec<oneshot::Sender<()>>,
    pub(super) conns: HashMap<u64, Conn>,
    pub(super) next_conn: u64,
    pub(super) pending: BTreeMap<String, Pending>,
    pub(super) next_pending: u64,
    pub(super) next_submission: u64,
    pub(super) turn_waiters: HashMap<String, Vec<oneshot::Sender<Result<String, AcpError>>>>,
    pub(super) shut_down: bool,
}

impl HubState {
    /// Enqueue `message` for `conn`; a full queue disconnects it (1013).
    pub(super) fn send(&mut self, conn: u64, message: RpcMessage) {
        let overflow = match self.conns.get(&conn) {
            Some(c) => c.tx.try_send(message).is_err(),
            None => return,
        };
        if overflow {
            warn!(conn, "controller connection is not keeping up; disconnecting");
            if let Some(c) = self.conns.get(&conn) {
                c.closed.cancel();
            }
            self.drop_conn(conn);
        }
    }

    /// Forget a connection everywhere; its pending requests stay with the
    /// hub.
    pub(super) fn drop_conn(&mut self, conn: u64) {
        self.conns.remove(&conn);
        for session in self.sessions.values_mut() {
            session.subscribers.remove(&conn);
        }
        for pending in self.pending.values_mut() {
            pending.sent_to.remove(&conn);
        }
    }

    /// Broadcast a session notification to the session's subscribers with the
    /// next `seq`.
    pub(super) fn notify_session(
        &mut self,
        session: &str,
        method: &str,
        params: impl FnOnce(u64) -> Value,
    ) {
        let Some(s) = self.sessions.get_mut(session) else { return };
        let seq = s.next_seq();
        let subscribers: Vec<u64> = s.subscribers.iter().copied().collect();
        let params = params(seq);
        for conn in subscribers {
            self.send(conn, RpcMessage::Notification {
                method: method.to_string(),
                params: params.clone(),
            });
        }
    }

    /// Send a hub-level notification to every initialized connection.
    pub(super) fn notify_all(&mut self, method: &str, params: Value) {
        let conns: Vec<u64> = self
            .conns
            .iter()
            .filter(|(_, c)| c.initialized)
            .map(|(id, _)| *id)
            .collect();
        for conn in conns {
            self.send(conn, RpcMessage::Notification {
                method: method.to_string(),
                params: params.clone(),
            });
        }
    }

    /// `_meta.pcx` / `_pcx/hub/state`.
    pub(super) fn hub_meta(&self) -> HubMeta {
        HubMeta {
            version: HUB_META_VERSION,
            agent: AgentIdentity {
                id: self.last_spec.agent_id.clone(),
                name: self.last_spec.display_name.clone(),
                version: self.last_spec.version.clone(),
                pinned: self.last_spec.pinned,
                info: self.agent_info.clone(),
            },
            caps: self.caps.clone(),
            auth: self.auth.clone(),
            process: self.process.clone(),
            default_config_options: self.default_config_options.clone(),
        }
    }

    /// Broadcast `_pcx/hub/state`.
    pub(super) fn broadcast_hub_state(&mut self) {
        let meta = serde_json::to_value(self.hub_meta()).unwrap_or(Value::Null);
        self.notify_all(notifications::HUB_STATE, meta);
    }

    /// The ready agent run.
    pub(super) fn ready_run(&self) -> Result<&AgentRun, AcpError> {
        match (&self.process, &self.run) {
            (ProcessState::Ready, Some(run)) if run.init.is_some() => Ok(run),
            (state, _) => {
                Err(AcpError::AgentUnavailable(format!("the agent is {}", state_name(state))))
            },
        }
    }

    /// Mark the agent as requiring authentication.
    pub(super) fn auth_required(&mut self, message: Option<String>) {
        if self.auth.status != pocket_codex_core::acp::pcx::auth_status::REQUIRED {
            self.auth.status = pocket_codex_core::acp::pcx::auth_status::REQUIRED.into();
            if message.is_some() {
                self.auth.message = message;
            }
            self.broadcast_hub_state();
        }
    }

    /// Pending requests of `session`.
    pub(super) fn pending_count(&self, session: &str) -> u32 {
        let n = self
            .pending
            .values()
            .filter(|p| p.session_id.as_deref() == Some(session))
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

fn state_name(state: &ProcessState) -> &'static str {
    match state {
        ProcessState::Stopped => "stopped",
        ProcessState::Starting => "starting",
        ProcessState::Ready => "ready",
        ProcessState::Restarting {
            ..
        } => "restarting",
        ProcessState::Failed {
            ..
        } => "failed",
    }
}

/// The ACP hub of one hosted instance.
pub struct AcpHub {
    pub(super) options: HubOptions,
    pub(super) epoch: String,
    pub(super) state: Mutex<HubState>,
    pub(super) shutdown: CancellationToken,
    /// Serializes `_pcx/hub/defaults` probes.
    pub(super) probe: tokio::sync::Mutex<()>,
}

/// Lowest-level lock helper; recovers from poisoning.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wall-clock milliseconds since the epoch.
pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `initialize` params the hub sends to the agent (TRD §3.2).
pub(super) fn client_initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false,
            "auth": {"terminal": true, "_meta": {"gateway": true}},
            "elicitation": {"form": {}, "url": {}},
            "session": {"configOptions": {"boolean": {}}},
            "_meta": {"terminal-auth": true}
        },
        "clientInfo": {
            "name": "pocket-codex",
            "title": "Pocket-Codex",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

impl AcpHub {
    /// Call `options.launch`, spawn, wait for the first `Ready` or `Failed`
    /// (≤ 60 s), then wait for the first auth detection (≤ 15 s) so the
    /// returned `info().auth` is meaningful.
    pub async fn start(options: HubOptions) -> Result<Arc<Self>, AcpError> {
        let created = super::ops::read_index(&options.state_dir, &options.instance);
        let hub = Arc::new(Self {
            options,
            epoch: format!("{:016x}", uuid::Uuid::new_v4().as_u128() as u64),
            state: Mutex::new(HubState {
                process: ProcessState::Starting,
                run: None,
                next_run: 0,
                launch_gen: 0,
                failures: FailureLog::default(),
                last_spec: LaunchSpec::default(),
                agent_info: None,
                caps: PcxCaps::default(),
                auth: AuthState {
                    status: pocket_codex_core::acp::pcx::auth_status::UNKNOWN.into(),
                    ..AuthState::default()
                },
                auth_note: None,
                default_config_options: Vec::new(),
                defaults: Defaults::default(),
                sessions: HashMap::new(),
                created,
                list_cache: None,
                list_refreshing: false,
                list_notify_changed: false,
                list_waiters: Vec::new(),
                conns: HashMap::new(),
                next_conn: 0,
                pending: BTreeMap::new(),
                next_pending: 0,
                next_submission: 0,
                turn_waiters: HashMap::new(),
                shut_down: false,
            }),
            shutdown: CancellationToken::new(),
            probe: tokio::sync::Mutex::new(()),
        });
        let first = match tokio::time::timeout(INITIALIZE_TIMEOUT, hub.launch_once()).await {
            Ok(result) => result,
            Err(_) => Err(AcpError::InitializeTimeout {
                stderr_tail: String::new(),
            }),
        };
        let launched = match first {
            Ok(launched) => launched,
            Err(error) => {
                let run = {
                    let mut st = lock(&hub.state);
                    st.process = ProcessState::Failed {
                        message: error.detail(),
                        stderr_tail: String::new(),
                    };
                    st.shut_down = true;
                    st.run.take()
                };
                if let Some(run) = run {
                    hub.stop_run(run).await;
                }
                return Err(error);
            },
        };
        if !launched.gateway_ok {
            let _ = tokio::time::timeout(AUTH_DETECT_WAIT, hub.detect_auth(launched.run_id)).await;
        }
        tokio::spawn(super::ops::idle_loop(Arc::downgrade(&hub), IDLE_CHECK_EVERY));
        Ok(hub)
    }

    /// Hosting snapshot.
    pub fn info(&self) -> HubInfo {
        let st = lock(&self.state);
        HubInfo {
            agent_id: st.last_spec.agent_id.clone(),
            agent_name: st.last_spec.display_name.clone(),
            agent_version: st.last_spec.version.clone(),
            agent_info: st.agent_info.clone(),
            pinned: st.last_spec.pinned,
            caps: st.caps.clone(),
            auth: st.auth.clone(),
            process: st.process.clone(),
            pid: st
                .run
                .as_ref()
                .and_then(|r| r.child.as_ref())
                .and_then(ChildHandle::pid),
            program: st.last_spec.program.clone(),
        }
    }

    /// Stop the current process (not counted as a failure) and start again,
    /// re-reading the launch provider.
    pub async fn restart(self: &Arc<Self>) -> Result<(), AcpError> {
        let old = {
            let mut st = lock(&self.state);
            if st.shut_down {
                return Err(AcpError::AgentUnavailable("hosting has stopped".into()));
            }
            st.launch_gen += 1;
            st.failures.clear();
            super::inbound::cleanup_after_exit(&mut st, "the agent was restarted");
            st.process = ProcessState::Starting;
            st.broadcast_hub_state();
            st.run.take()
        };
        if let Some(run) = old {
            self.stop_run(run).await;
        }
        let launched = match self.launch_once().await {
            Ok(launched) => launched,
            Err(error) => {
                let mut st = lock(&self.state);
                st.process = ProcessState::Failed {
                    message: error.detail(),
                    stderr_tail: String::new(),
                };
                st.broadcast_hub_state();
                return Err(error);
            },
        };
        if !launched.gateway_ok {
            let _ = tokio::time::timeout(AUTH_DETECT_WAIT, self.detect_auth(launched.run_id)).await;
        }
        Ok(())
    }

    /// Cancel running turns, wait up to `grace` for them, then terminate the
    /// process.
    pub async fn shutdown(&self, grace: Duration) {
        let running = {
            let mut st = lock(&self.state);
            st.shut_down = true;
            st.launch_gen += 1;
            self.shutdown.cancel();
            let running: Vec<String> = st
                .sessions
                .values()
                .filter(|s| s.running())
                .map(|s| s.id.clone())
                .collect();
            if let Some(run) = &st.run {
                for id in &running {
                    let _ = run
                        .peer
                        .notify_now(methods::SESSION_CANCEL, json!({"sessionId": id}));
                }
            }
            running
        };
        let deadline = tokio::time::Instant::now() + grace;
        while !running.is_empty() && tokio::time::Instant::now() < deadline {
            if !lock(&self.state).sessions.values().any(HubSession::running) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let run = {
            let mut st = lock(&self.state);
            super::inbound::cleanup_after_exit(&mut st, "hosting stopped");
            st.process = ProcessState::Stopped;
            st.broadcast_hub_state();
            st.run.take()
        };
        if let Some(run) = run {
            self.stop_run(run).await;
        }
    }

    /// Sessions with a running turn.
    pub fn running_sessions(&self) -> Vec<String> {
        let st = lock(&self.state);
        st.sessions
            .values()
            .filter(|s| s.running())
            .map(|s| s.id.clone())
            .collect()
    }

    /// Working directory of a known session.
    pub fn session_cwd(&self, session: &str) -> Option<String> {
        lock(&self.state)
            .sessions
            .get(session)
            .map(|s| s.cwd.clone())
            .filter(|c| !c.is_empty())
    }

    /// Start an agent-type `authenticate`; returns `inProgress` immediately.
    pub async fn authenticate(self: &Arc<Self>, method_id: &str) -> Result<AuthState, AcpError> {
        super::ops::start_authenticate(self, method_id)
    }

    /// Open the host terminal for a terminal-type method.
    pub fn terminal_login(self: &Arc<Self>, method_id: &str) -> Result<(), AcpError> {
        super::ops::terminal_login(self, method_id)
    }

    /// Restart and re-detect authentication.
    pub async fn recheck_auth(self: &Arc<Self>) -> Result<AuthState, AcpError> {
        self.restart().await?;
        Ok(lock(&self.state).auth.clone())
    }

    async fn stop_run(&self, mut run: AgentRun) {
        run.peer.close();
        if let Some(child) = run.child.take() {
            child.terminate(TERMINATE_GRACE).await;
        }
    }

    /// One start attempt: spawn, `initialize`, gateway login, `Ready`.
    ///
    /// Boxed because the exit watcher it spawns may call it again.
    pub(super) fn launch_once(
        self: &Arc<Self>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Launched, AcpError>> + Send + '_>>
    {
        Box::pin(self.launch_attempt())
    }

    async fn launch_attempt(self: &Arc<Self>) -> Result<Launched, AcpError> {
        let (spec, gateway) = (self.options.launch)()?;
        let mut io = self.options.connector.connect(&spec).await?;
        let child = io.child.take();
        let run_id = {
            let mut st = lock(&self.state);
            st.next_run += 1;
            st.last_spec = spec.clone();
            super::defaults::restore(&mut st, &self.options.state_dir, &self.options.instance);
            st.next_run
        };
        let weak = Arc::downgrade(self);
        let handler: super::peer::InboundHandler = Arc::new(move |message| {
            if let Some(hub) = weak.upgrade() {
                hub.on_agent(run_id, message);
            }
        });
        let (peer, join) = AgentPeer::start(io, self.options.log_file.clone(), handler);
        {
            let mut st = lock(&self.state);
            st.run = Some(AgentRun {
                id: run_id,
                peer: peer.clone(),
                child,
                spec: spec.clone(),
                gateway: gateway.clone(),
                init: None,
            });
        }
        let init = match self.initialize(&peer).await {
            Ok(init) => init,
            Err(error) => {
                self.abandon_run(run_id).await;
                return Err(error);
            },
        };
        let gateway_result = match &gateway {
            Some(g) => Some(self.gateway_login(&peer, &init, g).await),
            None => None,
        };
        let abandoned = {
            let mut st = lock(&self.state);
            let current = st.run.as_ref().map(|r| r.id) == Some(run_id);
            if !current || st.shut_down {
                true
            } else {
                super::inbound::adopt_initialize(&mut st, &init, gateway_result.as_ref());
                if let Some(run) = st.run.as_mut() {
                    run.init = Some(init);
                }
                st.process = ProcessState::Ready;
                st.broadcast_hub_state();
                false
            }
        };
        if abandoned {
            self.abandon_run(run_id).await;
            return Err(AcpError::AgentUnavailable("hosting has stopped".into()));
        }
        info!(instance = %self.options.instance, run_id, "ACP agent ready");
        let hub = self.clone();
        tokio::spawn(async move {
            let exit = join
                .await
                .unwrap_or(PeerExit::Io("read task panicked".into()));
            hub.on_exit(run_id, exit).await;
        });
        let skip_detection =
            matches!(gateway_result, Some(GatewayOutcome::Ok | GatewayOutcome::Failed(_)));
        Ok(Launched {
            run_id,
            gateway_ok: skip_detection,
        })
    }

    async fn initialize(&self, peer: &AgentPeer) -> Result<InitializeResponse, AcpError> {
        let value = match peer
            .request(methods::INITIALIZE, client_initialize(), Some(INITIALIZE_TIMEOUT))
            .await
        {
            Ok(value) => value,
            Err(AcpError::Timeout(_)) => {
                return Err(AcpError::InitializeTimeout {
                    stderr_tail: peer.stderr_tail_bytes(2048),
                });
            },
            Err(error) => {
                return Err(AcpError::AgentStartFailed {
                    message: error.detail(),
                    stderr_tail: peer.stderr_tail_bytes(2048),
                });
            },
        };
        let init: InitializeResponse =
            serde_json::from_value(value).map_err(|e| AcpError::AgentStartFailed {
                message: format!("invalid initialize response: {e}"),
                stderr_tail: peer.stderr_tail_bytes(2048),
            })?;
        if init.protocol_version != PROTOCOL_VERSION {
            return Err(AcpError::ProtocolUnsupported(init.protocol_version));
        }
        Ok(init)
    }

    async fn gateway_login(
        &self,
        peer: &AgentPeer,
        init: &InitializeResponse,
        gateway: &GatewayAuth,
    ) -> GatewayOutcome {
        let Some(method) = auth::selected_gateway(&init.auth_methods, gateway) else {
            return GatewayOutcome::NoMethod(
                "该 agent 没有提供网关登录方式，网关配置未生效".into(),
            );
        };
        let params = auth::gateway_params(&method.id, gateway);
        match peer
            .request(methods::AUTHENTICATE, params, Some(INITIALIZE_TIMEOUT))
            .await
        {
            Ok(_) => GatewayOutcome::Ok,
            Err(error) => {
                GatewayOutcome::Failed(format!("网关登录失败：{}", gateway.redact(&error.detail())))
            },
        }
    }

    async fn abandon_run(&self, run_id: u64) {
        let run = {
            let mut st = lock(&self.state);
            if st.run.as_ref().map(|r| r.id) == Some(run_id) {
                st.run.take()
            } else {
                None
            }
        };
        if let Some(run) = run {
            self.stop_run(run).await;
        }
    }

    /// The agent's stdout ended.
    async fn on_exit(self: Arc<Self>, run_id: u64, exit: PeerExit) {
        let (run, delay, gen) = {
            let mut st = lock(&self.state);
            if st.run.as_ref().map(|r| r.id) != Some(run_id) || st.shut_down {
                return;
            }
            let run = st.run.take();
            let stderr = run
                .as_ref()
                .map(|r| r.peer.stderr_tail_bytes(2048))
                .unwrap_or_default();
            warn!(instance = %self.options.instance, ?exit, "ACP agent exited");
            super::inbound::cleanup_after_exit(&mut st, "the agent process exited");
            if st.failures.record() {
                st.process = ProcessState::Failed {
                    message: "the agent kept exiting".into(),
                    stderr_tail: stderr,
                };
                st.broadcast_hub_state();
                (run, None, st.launch_gen)
            } else {
                let attempt = st.failures.recent();
                let delay = backoff(attempt);
                st.process = ProcessState::Restarting {
                    attempt,
                    retry_in_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                };
                st.broadcast_hub_state();
                (run, Some(delay), st.launch_gen)
            }
        };
        if let Some(run) = run {
            self.stop_run(run).await;
        }
        if let Some(delay) = delay {
            self.restart_loop(delay, gen).await;
        }
    }

    async fn restart_loop(self: Arc<Self>, mut delay: Duration, gen: u64) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                _ = self.shutdown.cancelled() => return,
            }
            {
                let mut st = lock(&self.state);
                if st.launch_gen != gen || st.shut_down {
                    return;
                }
                st.process = ProcessState::Starting;
                st.broadcast_hub_state();
            }
            match self.launch_once().await {
                Ok(launched) => {
                    if !launched.gateway_ok {
                        self.detect_auth(launched.run_id).await;
                    }
                    return;
                },
                Err(error) => {
                    let mut st = lock(&self.state);
                    if st.launch_gen != gen || st.shut_down {
                        return;
                    }
                    if st.failures.record() {
                        st.process = ProcessState::Failed {
                            message: error.detail(),
                            stderr_tail: String::new(),
                        };
                        st.broadcast_hub_state();
                        return;
                    }
                    let attempt = st.failures.recent();
                    delay = backoff(attempt);
                    st.process = ProcessState::Restarting {
                        attempt,
                        retry_in_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    };
                    st.broadcast_hub_state();
                },
            }
        }
    }

    /// Detect whether the agent needs authentication (`session/list {}`).
    pub(super) async fn detect_auth(&self, run_id: u64) {
        use pocket_codex_core::acp::pcx::auth_status;
        let peer = {
            let st = lock(&self.state);
            match &st.run {
                Some(run) if run.id == run_id && st.caps.list => Some(run.peer.clone()),
                _ => None,
            }
        };
        let status = match peer {
            None => auth_status::UNKNOWN,
            Some(peer) => {
                match peer
                    .request(methods::SESSION_LIST, json!({}), Some(Duration::from_secs(30)))
                    .await
                {
                    Ok(_) => auth_status::OK,
                    Err(e) if e.is_auth_required() => auth_status::REQUIRED,
                    Err(_) => auth_status::UNKNOWN,
                }
            },
        };
        let mut st = lock(&self.state);
        if st.run.as_ref().map(|r| r.id) != Some(run_id) {
            return;
        }
        if st.auth.status != auth_status::IN_PROGRESS {
            st.auth.status = status.into();
            if status == auth_status::OK {
                st.auth.message = st.auth_note.clone();
            }
            st.broadcast_hub_state();
        }
    }
}

/// Result of a successful [`AcpHub::launch_once`].
pub(super) struct Launched {
    pub(super) run_id: u64,
    /// A gateway login decided the auth state; skip detection.
    pub(super) gateway_ok: bool,
}

/// Result of the D20 gateway login.
pub(super) enum GatewayOutcome {
    /// `authenticate` succeeded.
    Ok,
    /// The agent offers no gateway method (message for the UI).
    NoMethod(String),
    /// `authenticate` failed (message without the secret).
    Failed(String),
}

/// A transport-independent controller connection (T18).
#[derive(Clone)]
pub struct HubConnection {
    pub(super) id: u64,
    pub(super) hub: Arc<AcpHub>,
    pub(super) closed: CancellationToken,
}

/// Unique id of a connection within its hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);

impl AcpHub {
    /// A transport-independent controller connection (T18): a cloneable
    /// handle plus the receiver of everything the hub sends to this
    /// controller.
    pub fn open_connection(self: &Arc<Self>) -> (HubConnection, mpsc::Receiver<RpcMessage>) {
        let (tx, rx) = mpsc::channel(CONNECTION_QUEUE);
        let closed = CancellationToken::new();
        let mut st = lock(&self.state);
        st.next_conn += 1;
        let id = st.next_conn;
        st.conns.insert(id, Conn {
            tx,
            closed: closed.clone(),
            initialized: false,
            form: false,
            url: false,
            next_request: 1,
            outgoing: HashMap::new(),
        });
        (
            HubConnection {
                id,
                hub: self.clone(),
                closed,
            },
            rx,
        )
    }
}

impl HubConnection {
    /// Connection id.
    pub fn id(&self) -> ConnId {
        ConnId(self.id)
    }

    /// Cancelled when the hub drops this connection (outbound queue full);
    /// the transport then closes with code 1013.
    pub fn closed(&self) -> CancellationToken {
        self.closed.clone()
    }

    /// Accept one message from the controller and return quickly. Requests
    /// are answered on the receiver returned by `open_connection`; anything
    /// that may wait (agent calls, loads, turns) runs in a spawned task.
    pub fn handle(&self, message: RpcMessage) {
        super::ops::handle(&self.hub, self.id, message);
    }

    /// Detach from every session; pending requests stay with the hub.
    pub fn close(&self) {
        lock(&self.hub.state).drop_conn(self.id);
    }
}

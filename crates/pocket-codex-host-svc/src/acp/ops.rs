//! Controller-facing protocol of [`HubConnection`](super::hub::HubConnection)
//! (TRD §4.2.6): standard ACP methods answered by the hub as a "virtual
//! agent", the `_pcx/*` extension methods, and session lifecycle (§4.2.4).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use pocket_codex_core::acp::{
    methods,
    pcx::{
        self, auth_kind, auth_status, notifications, AttachParams, AttachResult, AuthState,
        RunningResult, RunningSession, SessionGenerationParams, SessionListMeta,
        SessionLoadFailedParams, SessionLoadedParams, SessionParams, SessionStateParams,
        SubmitParams, SubmitResult, TurnStartedParams, WindowParams, WindowResult,
    },
    rpc::{code, RequestId, RpcError, RpcMessage},
    ContentBlock, HubItem, InitializeRequest, ListSessionsRequest, ListSessionsResponse,
    NewSessionRequest, PromptResponse, SessionInfo, SessionSetup, SetConfigOptionResponse,
    TextContent, Transcript,
};
use serde_json::{json, Value};
use tokio::sync::{oneshot, watch};
use tracing::warn;

use super::{
    auth,
    error::AcpError,
    hub::{lock, now_ms, AcpHub, HubState, IDLE_CLOSE_AFTER},
    inbound::{self, accepts, deliver, with_pcx},
    session::{HubSession, LoadOutcome, Phase, Queued},
};

const LIST_DEADLINE: Duration = Duration::from_secs(5);
const LIST_FRESH: Duration = Duration::from_secs(5);
const LIST_PAGE_TIMEOUT: Duration = Duration::from_secs(30);
const LIST_MAX_PAGES: usize = 20;
const LIST_MAX_SESSIONS: usize = 500;
const HUB_PAGE: usize = 100;
const ATTACH_DEADLINE: Duration = Duration::from_secs(20);
const CONFIG_DEADLINE: Duration = Duration::from_secs(30);
const NEW_DEADLINE: Duration = Duration::from_secs(45);
const NEW_TIMEOUT: Duration = Duration::from_secs(60);
const RESUME_TIMEOUT: Duration = Duration::from_secs(60);
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TURNS: usize = 2000;
const MAX_TRANSCRIPTS: usize = 16;
const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024 * 1024;
const INDEX_MAX: usize = 500;
const TERMINAL_POLL: Duration = Duration::from_secs(2);
const TERMINAL_MAX: Duration = Duration::from_secs(15 * 60);

/// Accept one controller message (see `HubConnection::handle`).
pub(super) fn handle(hub: &Arc<AcpHub>, conn: u64, message: RpcMessage) {
    match message {
        RpcMessage::Request {
            id,
            method,
            params,
        } => {
            if method == methods::INITIALIZE {
                initialize(hub, conn, id, &params);
                return;
            }
            {
                let mut st = lock(&hub.state);
                match st.conns.get(&conn) {
                    None => return,
                    Some(c) if !c.initialized => {
                        let error = RpcError::new(code::INVALID_REQUEST, "call initialize first");
                        st.send(conn, RpcMessage::Response {
                            id,
                            result: Err(error),
                        });
                        return;
                    },
                    Some(_) => {},
                }
            }
            let hub = hub.clone();
            tokio::spawn(async move {
                if let Some(result) = dispatch(&hub, conn, id.clone(), &method, params).await {
                    lock(&hub.state).send(conn, RpcMessage::Response {
                        id,
                        result,
                    });
                }
            });
        },
        RpcMessage::Notification {
            method,
            params,
        } => {
            if method == methods::SESSION_CANCEL {
                if let Some(session) = params.get("sessionId").and_then(Value::as_str) {
                    cancel(hub, session);
                }
            }
        },
        RpcMessage::Response {
            id,
            result,
        } => answer_pending(hub, conn, id, result),
    }
}

fn rpc(result: Result<Value, AcpError>) -> Option<Result<Value, RpcError>> {
    Some(result.map_err(|e| e.to_rpc()))
}

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, AcpError> {
    serde_json::from_value(params).map_err(|e| AcpError::InvalidParams(e.to_string()))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, AcpError> {
    serde_json::to_value(value).map_err(|e| AcpError::Internal(e.to_string()))
}

async fn dispatch(
    hub: &Arc<AcpHub>,
    conn: u64,
    id: RequestId,
    method: &str,
    params: Value,
) -> Option<Result<Value, RpcError>> {
    if method != pcx::methods::SESSION_DETACH && method != pcx::methods::SESSIONS_RUNNING {
        if let Err(e) = lock(&hub.state).ready_run() {
            return rpc(Err(e));
        }
    }
    match method {
        methods::SESSION_LIST => rpc(async { list(hub, parse(params)?).await }.await),
        methods::SESSION_NEW => rpc(async { new_session(hub, conn, parse(params)?).await }.await),
        methods::SESSION_LOAD => {
            standard_load(hub, conn, id, params).await;
            None
        },
        methods::SESSION_PROMPT => rpc(standard_prompt(hub, params).await),
        methods::SESSION_SET_CONFIG_OPTION | methods::SESSION_SET_MODE => {
            rpc(set_config(hub, method, params).await)
        },
        pcx::methods::SESSION_ATTACH => {
            match parse::<AttachParams>(params) {
                Ok(p) => attach(hub, conn, id, p.session_id, p.cwd, p.tail, false).await,
                Err(e) => return rpc(Err(e)),
            }
            None
        },
        pcx::methods::SESSION_RELOAD => {
            match parse::<SessionParams>(params) {
                Ok(p) => attach(hub, conn, id, p.session_id, None, None, true).await,
                Err(e) => return rpc(Err(e)),
            }
            None
        },
        pcx::methods::SESSION_DETACH => rpc(parse::<SessionParams>(params).map(|p| {
            if let Some(s) = lock(&hub.state).sessions.get_mut(&p.session_id) {
                s.subscribers.remove(&conn);
            }
            json!({})
        })),
        pcx::methods::SESSION_WINDOW => rpc(parse(params).and_then(|p| window(hub, p))),
        pcx::methods::SESSION_SUBMIT => rpc(async { submit(hub, parse(params)?) }.await),
        pcx::methods::SESSIONS_RUNNING => rpc(running(hub)),
        pcx::methods::HUB_DEFAULTS => {
            rpc(async { to_value(&super::defaults::defaults(hub).await?) }.await)
        },
        pcx::methods::AUTH_AUTHENTICATE => rpc(async {
            let p: pcx::AuthenticateParams = parse(params)?;
            let kind = lock(&hub.state)
                .auth
                .methods
                .iter()
                .find(|m| m.id == p.method_id)
                .map(|m| m.kind.clone());
            match kind.as_deref() {
                None => Err(AcpError::InvalidParams(format!("unknown method {}", p.method_id))),
                Some(auth_kind::AGENT) => to_value(&start_authenticate(hub, &p.method_id)?),
                Some(_) => Err(AcpError::HostOnly(
                    "this login method can only be completed on the host".into(),
                )),
            }
        }
        .await),
        _ => Some(Err(RpcError::new(code::METHOD_NOT_FOUND, format!("{method} is not supported")))),
    }
}

fn initialize(hub: &Arc<AcpHub>, conn: u64, id: RequestId, params: &Value) {
    let request: Option<InitializeRequest> = serde_json::from_value(params.clone()).ok();
    let elicitation = request
        .as_ref()
        .and_then(|r| r.client_capabilities.elicitation.clone());
    let mut st = lock(&hub.state);
    let Some(c) = st.conns.get_mut(&conn) else { return };
    c.initialized = true;
    c.form = elicitation.as_ref().is_some_and(|e| e.form.is_some());
    c.url = elicitation.as_ref().is_some_and(|e| e.url.is_some());
    let prompt = st
        .run
        .as_ref()
        .and_then(|r| r.init.as_ref())
        .map(|i| i.agent_capabilities.prompt_capabilities.clone())
        .unwrap_or_default();
    let result = json!({
        "protocolVersion": pocket_codex_core::acp::PROTOCOL_VERSION,
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": prompt,
            "sessionCapabilities": {"list": {}},
            "mcpCapabilities": {}
        },
        "authMethods": [],
        "agentInfo": {"name": "pocket-codex-acp-hub", "version": env!("CARGO_PKG_VERSION")},
        "_meta": {"pcx": st.hub_meta()}
    });
    st.send(conn, RpcMessage::Response {
        id,
        result: Ok(result),
    });
    let hub_level: Vec<(String, Option<&'static str>)> = st
        .pending
        .values()
        .filter(|p| p.session_id.is_none())
        .map(|p| (p.id.clone(), p.mode()))
        .collect();
    for (pending, mode) in hub_level {
        if accepts(&st, conn, mode) {
            deliver(&mut st, &pending, conn, None);
        }
    }
}

fn answer_pending(hub: &Arc<AcpHub>, conn: u64, id: RequestId, result: Result<Value, RpcError>) {
    let mut st = lock(&hub.state);
    let Some(pending_id) = st.conns.get_mut(&conn).and_then(|c| c.outgoing.remove(&id)) else {
        return;
    };
    let Some(pending) = st.pending.get_mut(&pending_id) else { return };
    let answer = match result {
        Ok(answer) => answer,
        Err(_) => {
            pending.sent_to.remove(&conn);
            return;
        },
    };
    match pending.validate(&answer) {
        Ok(response) => inbound::resolve(&mut st, &pending_id, response, Some(conn)),
        Err(reason) => deliver(&mut st, &pending_id, conn, Some(&reason)),
    }
}

// ---------------------------------------------------------------- listing

fn sort_key(info: &SessionInfo) -> Option<i64> {
    let at = info.updated_at.as_deref()?;
    chrono::DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.timestamp_millis())
}

fn merged(st: &HubState) -> Vec<SessionInfo> {
    let mut all: Vec<SessionInfo> = st
        .list_cache
        .as_ref()
        .map(|(_, l)| l.clone())
        .unwrap_or_default();
    for created in &st.created {
        if !all.iter().any(|s| s.session_id == created.session_id) {
            all.push(created.clone());
        }
    }
    for info in &mut all {
        if let Some(s) = st.sessions.get(&info.session_id) {
            if s.title.is_some() {
                info.title = s.title.clone();
            }
        }
    }
    all.sort_by(|a, b| match (sort_key(a), sort_key(b)) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    all
}

async fn list(hub: &Arc<AcpHub>, request: ListSessionsRequest) -> Result<Value, AcpError> {
    let (fresh, can_list) = {
        let st = lock(&hub.state);
        let fresh = st
            .list_cache
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < LIST_FRESH);
        (fresh, st.caps.list)
    };
    let mut loading = false;
    if can_list && !fresh {
        let rx = start_refresh(hub);
        if tokio::time::timeout(LIST_DEADLINE, rx).await.is_err() {
            let mut st = lock(&hub.state);
            st.list_notify_changed = true;
            loading = st.list_cache.is_none();
        }
    }
    let st = lock(&hub.state);
    let mut all = merged(&st);
    if let Some(cwd) = &request.cwd {
        all.retain(|s| &s.cwd == cwd);
    }
    let offset = request
        .cursor
        .as_deref()
        .and_then(|c| c.strip_prefix("o:"))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    let page: Vec<SessionInfo> = all
        .iter()
        .skip(offset)
        .take(HUB_PAGE)
        .map(|info| {
            let meta = st.sessions.get(&info.session_id).map(|s| SessionListMeta {
                running: s.running(),
                pending: st.pending_count(&s.id),
                queue: u32::try_from(s.queue.len()).unwrap_or(u32::MAX),
            });
            let mut info = info.clone();
            let meta = serde_json::to_value(meta.unwrap_or_default()).unwrap_or(Value::Null);
            let mut base = match info.meta.take() {
                Some(Value::Object(map)) => map,
                _ => serde_json::Map::new(),
            };
            base.insert("pcx".into(), meta);
            info.meta = Some(Value::Object(base));
            info
        })
        .collect();
    let next = offset + page.len();
    let response = ListSessionsResponse {
        sessions: page,
        next_cursor: (next < all.len()).then(|| format!("o:{next}")),
        meta: loading.then(|| json!({"pcx": {"loading": true}})),
    };
    to_value(&response)
}

fn start_refresh(hub: &Arc<AcpHub>) -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();
    let mut st = lock(&hub.state);
    st.list_waiters.push(tx);
    if !st.list_refreshing {
        st.list_refreshing = true;
        tokio::spawn(refresh(hub.clone()));
    }
    rx
}

async fn refresh(hub: Arc<AcpHub>) {
    let result = fetch_all(&hub).await;
    let probe_dir = super::defaults::probe_dir(&hub.options.state_dir);
    let mut st = lock(&hub.state);
    st.list_refreshing = false;
    match result {
        Ok(mut listed) => {
            listed.retain(|info| !super::defaults::is_hidden(&st, info, &probe_dir));
            apply_listing(&mut st, listed);
        },
        Err(e) => {
            if e.is_auth_required() {
                st.auth_required(Some(e.detail()));
            }
            warn!("session/list failed: {e}");
        },
    }
    for waiter in std::mem::take(&mut st.list_waiters) {
        let _ = waiter.send(());
    }
    if std::mem::take(&mut st.list_notify_changed) {
        st.notify_all(notifications::SESSIONS_CHANGED, json!({}));
    }
}

async fn fetch_all(hub: &Arc<AcpHub>) -> Result<Vec<SessionInfo>, AcpError> {
    let peer = lock(&hub.state).ready_run()?.peer.clone();
    let mut out: Vec<SessionInfo> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..LIST_MAX_PAGES {
        let request = ListSessionsRequest {
            cwd: None,
            cursor: cursor.clone(),
            meta: None,
        };
        let value = peer
            .request(methods::SESSION_LIST, to_value(&request)?, Some(LIST_PAGE_TIMEOUT))
            .await?;
        let page: ListSessionsResponse = parse(value)?;
        out.extend(page.sessions);
        cursor = page.next_cursor;
        if cursor.is_none() || out.len() >= LIST_MAX_SESSIONS {
            break;
        }
    }
    out.truncate(LIST_MAX_SESSIONS);
    Ok(out)
}

fn apply_listing(st: &mut HubState, listed: Vec<SessionInfo>) {
    let mut changed: Vec<String> = Vec::new();
    for info in &listed {
        let session = st
            .sessions
            .entry(info.session_id.clone())
            .or_insert_with(|| HubSession::listed(info.session_id.clone(), info.cwd.clone()));
        if session.cwd.is_empty() {
            session.cwd = info.cwd.clone();
        }
        if info.title.is_some() {
            session.title = info.title.clone();
        }
        if info.updated_at == session.updated_at {
            session.baseline_pending = false;
            continue;
        }
        let first_sight = session.updated_at.is_none() && session.transcript.is_none();
        session.updated_at = info.updated_at.clone();
        if std::mem::take(&mut session.baseline_pending) {
            session.transcript_updated_at = info.updated_at.clone();
        } else if !first_sight && !session.subscribers.is_empty() && !session.running() {
            changed.push(session.id.clone());
        }
    }
    st.list_cache = Some((tokio::time::Instant::now(), listed));
    for id in changed {
        broadcast_state(st, &id);
    }
}

fn broadcast_state(st: &mut HubState, id: &str) {
    let pending = st.pending_count(id);
    let Some(s) = st.sessions.get(id) else { return };
    let params = SessionStateParams {
        session_id: id.to_string(),
        running: s.running(),
        queue: u32::try_from(s.queue.len()).unwrap_or(u32::MAX),
        pending,
        title: s.title.clone(),
        updated_at: s.updated_at.clone(),
        seq: 0,
    };
    st.notify_session(id, notifications::SESSION_STATE, |seq| {
        serde_json::to_value(SessionStateParams {
            seq,
            ..params
        })
        .unwrap_or(Value::Null)
    });
}

/// Hub-created sessions of an agent without `session/list`.
pub(super) fn read_index(state_dir: &Path, instance: &str) -> Vec<SessionInfo> {
    std::fs::read(index_path(state_dir, instance))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn index_path(state_dir: &Path, instance: &str) -> PathBuf {
    state_dir.join("sessions").join(format!("{instance}.json"))
}

fn write_index(state_dir: &Path, instance: &str, sessions: &[SessionInfo]) {
    let path = index_path(state_dir, instance);
    let keep: Vec<&SessionInfo> = sessions.iter().rev().take(INDEX_MAX).rev().collect();
    let Ok(bytes) = serde_json::to_vec(&keep) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(&path) {
        use std::io::Write as _;
        let _ = file.write_all(&bytes);
    }
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ------------------------------------------------------------ new session

async fn new_session(
    hub: &Arc<AcpHub>,
    conn: u64,
    request: NewSessionRequest,
) -> Result<Value, AcpError> {
    if request.cwd.trim().is_empty() {
        return Err(AcpError::CwdRequired);
    }
    let (peer, run_id) = {
        let st = lock(&hub.state);
        let run = st.ready_run()?;
        (run.peer.clone(), run.id)
    };
    let cwd = request.cwd.clone();
    let hub2 = hub.clone();
    let task = tokio::spawn(async move {
        let params = json!({ "cwd": cwd, "mcpServers": [] });
        let result = peer
            .request(methods::SESSION_NEW, params, Some(NEW_TIMEOUT))
            .await;
        let mut st = lock(&hub2.state);
        let value = match result {
            Ok(value) => value,
            Err(e) => {
                if e.is_auth_required() {
                    st.auth_required(Some(e.detail()));
                }
                return Err(e);
            },
        };
        let setup: SessionSetup = parse(value.clone())?;
        let Some(id) = setup.session_id.clone() else {
            return Err(AcpError::AgentError {
                code: code::INTERNAL_ERROR,
                message: "session/new returned no sessionId".into(),
            });
        };
        if st.run.as_ref().map(|r| r.id) != Some(run_id) {
            return Err(AcpError::AgentUnavailable("the agent restarted".into()));
        }
        let epoch = hub2.epoch.clone();
        let session = st
            .sessions
            .entry(id.clone())
            .or_insert_with(|| HubSession::listed(id.clone(), cwd.clone()));
        session.cwd = cwd.clone();
        session.agent_loaded = true;
        session.phase = Phase::Idle;
        session.materialized = true;
        session.updated_at = Some(rfc3339_now());
        session.transcript_updated_at = session.updated_at.clone();
        session.transcript = Some(Transcript::new(format!("{epoch}.{}", session.generation_seq)));
        session.subscribers.insert(conn);
        session.touch();
        let updated = session.updated_at.clone();
        adopt_setup(&hub2, &mut st, &id, &setup);
        st.created.push(SessionInfo {
            session_id: id.clone(),
            cwd: cwd.clone(),
            title: None,
            updated_at: updated,
            meta: None,
        });
        if !st.caps.list {
            write_index(&hub2.options.state_dir, &hub2.options.instance, &st.created);
        }
        st.notify_all(notifications::SESSIONS_CHANGED, json!({}));
        enforce_lru(&hub2, &mut st);
        Ok(value)
    });
    match tokio::time::timeout(NEW_DEADLINE, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(AcpError::Internal(e.to_string())),
        Err(_) => Err(AcpError::Timeout("session/new is taking too long".into())),
    }
}

/// Record config options and modes from a session response.
fn adopt_setup(hub: &AcpHub, st: &mut HubState, id: &str, setup: &SessionSetup) {
    let mut caps_changed = false;
    if let Some(options) = &setup.config_options {
        caps_changed |= !st.caps.config_options;
        st.caps.config_options = true;
        st.default_config_options = options.clone();
        super::defaults::save(st, &hub.options.state_dir, &hub.options.instance);
    }
    if setup.modes.is_some() {
        caps_changed |= !st.caps.modes;
        st.caps.modes = true;
    }
    if let Some(s) = st.sessions.get_mut(id) {
        if let Some(options) = &setup.config_options {
            s.config_options = options.clone();
        }
        if let Some(modes) = &setup.modes {
            s.modes = Some(modes.clone());
        }
    }
    if caps_changed {
        st.broadcast_hub_state();
    }
}

// ---------------------------------------------------------------- loading

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// `session/resume`, keeping the current transcript.
    Resume,
    /// `session/resume` with a fresh, history-less transcript.
    ResumeFresh,
    /// `session/load` replaying into a new transcript.
    Load,
}

/// Make sure `id` is loaded; `None` when it already is, otherwise the
/// receiver of the (possibly shared) load.
fn begin_load(
    hub: &Arc<AcpHub>,
    st: &mut HubState,
    id: &str,
    force: bool,
) -> Result<Option<watch::Receiver<LoadOutcome>>, AcpError> {
    let (caps_load, caps_resume) = (st.caps.load, st.caps.resume);
    let run_id = st.ready_run()?.id;
    let Some(s) = st.sessions.get_mut(id) else {
        return Err(AcpError::SessionNotLoadable(format!("unknown session {id}")));
    };
    if matches!(s.phase, Phase::Loading) {
        return Ok(s.load_rx.clone());
    }
    if s.running() || matches!(s.phase, Phase::Closing) {
        return Ok(None);
    }
    if !force && s.agent_loaded && s.transcript.is_some() {
        return Ok(None);
    }
    let current = s.transcript.is_some() && s.updated_at == s.transcript_updated_at;
    let plan = if caps_resume && current && !force {
        Plan::Resume
    } else if caps_load {
        Plan::Load
    } else if caps_resume {
        Plan::ResumeFresh
    } else {
        return Err(AcpError::SessionNotLoadable("this agent cannot open past sessions".into()));
    };
    let (tx, rx) = watch::channel(None);
    s.phase = Phase::Loading;
    s.load_rx = Some(rx.clone());
    if plan == Plan::Load {
        s.replay = Some(Transcript::new(String::new()));
    }
    tokio::spawn(load_task(hub.clone(), id.to_string(), plan, run_id, tx));
    Ok(Some(rx))
}

async fn load_task(
    hub: Arc<AcpHub>,
    id: String,
    plan: Plan,
    run_id: u64,
    tx: watch::Sender<LoadOutcome>,
) {
    let call = {
        let st = lock(&hub.state);
        let peer = st
            .run
            .as_ref()
            .filter(|r| r.id == run_id)
            .map(|r| r.peer.clone());
        let cwd = st
            .sessions
            .get(&id)
            .map(|s| s.cwd.clone())
            .unwrap_or_default();
        peer.map(|p| (p, cwd))
    };
    let result = match call {
        None => Err(AcpError::AgentUnavailable("the agent restarted".into())),
        Some((peer, cwd)) => {
            let params = json!({ "sessionId": id, "cwd": cwd, "mcpServers": [] });
            match plan {
                Plan::Load => {
                    peer.request(methods::SESSION_LOAD, params, Some(LOAD_TIMEOUT))
                        .await
                },
                _ => {
                    peer.request(methods::SESSION_RESUME, params, Some(RESUME_TIMEOUT))
                        .await
                },
            }
        },
    };
    let outcome = {
        let mut st = lock(&hub.state);
        let stale = st.run.as_ref().map(|r| r.id) != Some(run_id);
        let result = if stale {
            Err(AcpError::AgentUnavailable("the agent restarted".into()))
        } else {
            result.and_then(parse::<SessionSetup>)
        };
        match result {
            Ok(setup) => {
                finish_load(&hub, &mut st, &id, plan, &setup);
                Ok(())
            },
            Err(e) => {
                fail_load(&mut st, &id, &e, stale);
                Err(e)
            },
        }
    };
    let _ = tx.send(Some(outcome));
}

fn finish_load(hub: &Arc<AcpHub>, st: &mut HubState, id: &str, plan: Plan, setup: &SessionSetup) {
    let epoch = hub.epoch.clone();
    let mut generation_changed = false;
    let Some(s) = st.sessions.get_mut(id) else { return };
    match plan {
        Plan::Resume => {},
        Plan::ResumeFresh => {
            if s.materialized {
                s.generation_seq += 1;
            }
            let mut t = Transcript::new(format!("{epoch}.{}", s.generation_seq));
            t.mark_history_unavailable();
            generation_changed = s.transcript.is_some();
            s.transcript = Some(t);
        },
        Plan::Load => {
            let mut fresh = s.replay.take().unwrap_or_default();
            match s.transcript.take() {
                Some(old) if fresh.adopt_ids_from(&old) => fresh.set_generation(old.generation()),
                Some(_) => {
                    s.generation_seq += 1;
                    fresh.set_generation(format!("{epoch}.{}", s.generation_seq));
                    generation_changed = true;
                },
                None => {
                    if s.materialized {
                        s.generation_seq += 1;
                    }
                    fresh.set_generation(format!("{epoch}.{}", s.generation_seq));
                },
            }
            s.transcript = Some(fresh);
        },
    }
    s.materialized = true;
    s.replay = None;
    s.transcript_updated_at = s.updated_at.clone();
    s.agent_loaded = true;
    s.phase = Phase::Idle;
    s.load_rx = None;
    s.touch();
    let generation = s
        .transcript
        .as_ref()
        .map(|t| t.generation().to_string())
        .unwrap_or_default();
    adopt_setup(hub, st, id, setup);
    let session_id = id.to_string();
    st.notify_session(id, notifications::SESSION_LOADED, |seq| {
        serde_json::to_value(SessionLoadedParams {
            session_id: session_id.clone(),
            seq,
        })
        .unwrap_or(Value::Null)
    });
    if generation_changed {
        st.notify_session(id, notifications::SESSION_GENERATION, |seq| {
            serde_json::to_value(SessionGenerationParams {
                session_id,
                generation,
                seq,
            })
            .unwrap_or(Value::Null)
        });
    }
    drain_queue(hub, st, id);
    enforce_lru(hub, st);
}

fn fail_load(st: &mut HubState, id: &str, error: &AcpError, stale: bool) {
    if error.is_auth_required() {
        st.auth_required(Some(error.detail()));
    }
    let Some(s) = st.sessions.get_mut(id) else { return };
    s.replay = None;
    s.load_rx = None;
    if !stale {
        s.phase = Phase::Listed;
        s.agent_loaded = false;
    }
    let session_id = id.to_string();
    let message = error.to_rpc().message;
    st.notify_session(id, notifications::SESSION_LOAD_FAILED, |seq| {
        serde_json::to_value(SessionLoadFailedParams {
            session_id,
            error: message,
            seq,
        })
        .unwrap_or(Value::Null)
    });
    inbound::fail_queue(st, id, "error");
}

/// Wait for a load (bounded); `Ok(true)` = loaded, `Ok(false)` = still
/// loading.
async fn wait_load(
    rx: Option<watch::Receiver<LoadOutcome>>,
    limit: Duration,
) -> Result<bool, AcpError> {
    let Some(mut rx) = rx else { return Ok(true) };
    let waited = tokio::time::timeout(limit, async {
        loop {
            if let Some(outcome) = rx.borrow().clone() {
                return outcome;
            }
            if rx.changed().await.is_err() {
                return Err(AcpError::AgentUnavailable("the load was abandoned".into()));
            }
        }
    })
    .await;
    match waited {
        Ok(Ok(())) => Ok(true),
        Ok(Err(e)) => Err(e),
        Err(_) => Ok(false),
    }
}

/// Make sure the hub knows `id`, using `cwd` or a fresh listing.
async fn ensure_known(hub: &Arc<AcpHub>, id: &str, cwd: Option<String>) -> Result<(), AcpError> {
    {
        let mut st = lock(&hub.state);
        if let Some(s) = st.sessions.get_mut(id) {
            if s.cwd.is_empty() {
                if let Some(cwd) = cwd {
                    s.cwd = cwd;
                }
            }
            return Ok(());
        }
        if let Some(cwd) = cwd.clone().filter(|c| !c.is_empty()) {
            st.sessions
                .insert(id.to_string(), HubSession::listed(id.to_string(), cwd));
            return Ok(());
        }
        if !st.caps.list {
            if let Some(info) = st.created.iter().find(|s| s.session_id == id).cloned() {
                st.sessions
                    .insert(id.to_string(), HubSession::listed(id.to_string(), info.cwd));
                return Ok(());
            }
        }
    }
    let rx = start_refresh(hub);
    let _ = tokio::time::timeout(LIST_DEADLINE, rx).await;
    if lock(&hub.state).sessions.contains_key(id) {
        Ok(())
    } else {
        Err(AcpError::SessionNotLoadable(format!("unknown session {id}")))
    }
}

/// Load `id` without subscribing (history reads, D9).
pub(super) async fn ensure_loaded_quietly(hub: &Arc<AcpHub>, id: &str) -> Result<(), AcpError> {
    ensure_known(hub, id, None).await?;
    let rx = {
        let mut st = lock(&hub.state);
        begin_load(hub, &mut st, id, false)?
    };
    if wait_load(rx, LOAD_TIMEOUT).await? {
        Ok(())
    } else {
        Err(AcpError::SessionLoading)
    }
}

// ----------------------------------------------------------------- attach

async fn attach(
    hub: &Arc<AcpHub>,
    conn: u64,
    request: RequestId,
    id: String,
    cwd: Option<String>,
    tail: Option<u32>,
    reload: bool,
) {
    let tail = tail.unwrap_or(20).clamp(1, 100) as usize;
    let result = async {
        ensure_known(hub, &id, cwd).await?;
        let rx = {
            let mut st = lock(&hub.state);
            begin_load(hub, &mut st, &id, reload)?
        };
        wait_load(rx, ATTACH_DEADLINE).await
    }
    .await;
    let mut st = lock(&hub.state);
    let loaded = match result {
        Ok(loaded) => loaded,
        Err(e) => {
            st.send(conn, RpcMessage::Response {
                id: request,
                result: Err(e.to_rpc()),
            });
            return;
        },
    };
    let loading = !loaded
        || st
            .sessions
            .get(&id)
            .is_some_and(|s| matches!(s.phase, Phase::Loading) || s.transcript.is_none());
    let Some(s) = st.sessions.get_mut(&id) else { return };
    s.subscribers.insert(conn);
    s.touch();
    let snapshot = snapshot(&st, &id, tail, loading);
    let value =
        serde_json::to_value(&snapshot).map_err(|e| AcpError::Internal(e.to_string()).to_rpc());
    st.send(conn, RpcMessage::Response {
        id: request,
        result: value,
    });
    if !loading {
        resend_pending(&mut st, &id, conn);
    }
}

fn resend_pending(st: &mut HubState, session: &str, conn: u64) {
    let ids: Vec<(String, Option<&'static str>)> = st
        .pending
        .values()
        .filter(|p| p.session_id.as_deref() == Some(session) && !p.sent_to.contains_key(&conn))
        .map(|p| (p.id.clone(), p.mode()))
        .collect();
    for (pending, mode) in ids {
        if accepts(st, conn, mode) {
            deliver(st, &pending, conn, None);
        }
    }
}

/// Replace oversized images and drop the oldest items until the response
/// fits.
fn bound_items(items: &mut Vec<HubItem>, has_older: &mut bool) {
    for item in items.iter_mut() {
        for block in item.content.iter_mut() {
            if let ContentBlock::Image(image) = block {
                if image.data.len() > MAX_IMAGE_BYTES {
                    *block = ContentBlock::Text(TextContent {
                        text: "[图片过大，未传输]".into(),
                        ..TextContent::default()
                    });
                }
            }
        }
    }
    let mut size: usize = items
        .iter()
        .map(|i| serde_json::to_vec(i).map_or(0, |v| v.len()))
        .sum();
    while size > MAX_RESPONSE_BYTES && !items.is_empty() {
        let first = items.remove(0);
        size -= serde_json::to_vec(&first).map_or(0, |v| v.len());
        *has_older = true;
    }
}

fn snapshot(st: &HubState, id: &str, tail: usize, loading: bool) -> AttachResult {
    let Some(s) = st.sessions.get(id) else { return AttachResult::default() };
    let mut result = AttachResult {
        session_id: s.id.clone(),
        cwd: s.cwd.clone(),
        title: s.title.clone(),
        updated_at: s.updated_at.clone(),
        loading,
        seq: s.seq,
        running: s.running(),
        active_turn: s.active_turn(),
        queue: s.queue.iter().map(Queued::info).collect(),
        config_options: s.config_options.clone(),
        modes: s.modes.clone(),
        commands: s.commands.clone(),
        usage: s.usage.clone(),
        ..AttachResult::default()
    };
    if let (false, Some(t)) = (loading, s.transcript.as_ref()) {
        let (mut items, mut has_older) = t.window_before(None, tail);
        bound_items(&mut items, &mut has_older);
        result.generation = t.generation().to_string();
        result.older_unavailable = t.dropped_turns() > 0 && !has_older;
        result.items = items;
        result.has_older = has_older;
        let turns = t.turns();
        result.turns = turns[turns.len().saturating_sub(MAX_TURNS)..].to_vec();
        result.dropped_turns = t.dropped_turns();
    } else if let Some(t) = s.transcript.as_ref() {
        result.generation = t.generation().to_string();
    }
    result
}

// ----------------------------------------------------------------- window

fn window(hub: &Arc<AcpHub>, params: WindowParams) -> Result<Value, AcpError> {
    let mut st = lock(&hub.state);
    let Some(s) = st.sessions.get(&params.session_id) else {
        return Err(AcpError::NotFound(format!("unknown session {}", params.session_id)));
    };
    if matches!(s.phase, Phase::Loading) {
        return Err(AcpError::SessionLoading);
    }
    let Some(t) = s.transcript.as_ref() else {
        let _ = begin_load(hub, &mut st, &params.session_id, false)?;
        return Err(AcpError::SessionLoading);
    };
    if t.generation() != params.generation {
        return Err(AcpError::GenerationChanged);
    }
    let limit = params.limit.unwrap_or(60).clamp(1, 100) as usize;
    let mut result = WindowResult {
        generation: t.generation().to_string(),
        ..WindowResult::default()
    };
    if let Some(turn) = params.turn {
        if params.after.as_deref().is_some_and(|a| t.item(a).is_none()) {
            return Err(AcpError::GenerationChanged);
        }
        let (items, has_more) = t.turn_items(turn, params.after.as_deref(), limit);
        let first_index = items
            .first()
            .and_then(|first| t.items().iter().position(|i| i.id == first.id))
            .unwrap_or(0);
        result.items = items;
        result.has_more = has_more;
        result.has_older = first_index > 0;
    } else {
        if params
            .before
            .as_deref()
            .is_some_and(|b| t.item(b).is_none())
        {
            return Err(AcpError::GenerationChanged);
        }
        let (items, has_older) = t.window_before(params.before.as_deref(), limit);
        result.items = items;
        result.has_older = has_older;
        result.older_unavailable = t.dropped_turns() > 0 && !has_older;
    }
    bound_items(&mut result.items, &mut result.has_older);
    if let Some(s) = st.sessions.get_mut(&params.session_id) {
        s.last_access = tokio::time::Instant::now();
    }
    to_value(&result)
}

// ----------------------------------------------------------------- submit

fn submit(hub: &Arc<AcpHub>, params: SubmitParams) -> Result<Value, AcpError> {
    let result = submit_inner(hub, params)?;
    to_value(&result)
}

fn submit_inner(hub: &Arc<AcpHub>, params: SubmitParams) -> Result<SubmitResult, AcpError> {
    let mut st = lock(&hub.state);
    st.ready_run()?;
    let has_image = params
        .prompt
        .iter()
        .any(|b| matches!(b, ContentBlock::Image(_)));
    if has_image && !st.caps.image {
        return Err(AcpError::ImagesUnsupported);
    }
    if !st.sessions.contains_key(&params.session_id) {
        if let Some(info) = merged(&st)
            .into_iter()
            .find(|s| s.session_id == params.session_id)
        {
            st.sessions
                .insert(info.session_id.clone(), HubSession::listed(info.session_id, info.cwd));
        } else {
            return Err(AcpError::NotFound(format!("unknown session {}", params.session_id)));
        }
    }
    if let Some(previous) = st
        .sessions
        .get(&params.session_id)
        .and_then(|s| s.submissions.get(&params.client_submission_id))
    {
        return Ok(previous);
    }
    st.next_submission += 1;
    let submission = format!("q{}", st.next_submission);
    let id = params.session_id.clone();
    let startable = st.sessions.get(&id).is_some_and(|s| {
        matches!(s.phase, Phase::Idle)
            && s.agent_loaded
            && s.transcript.is_some()
            && s.queue.is_empty()
    });
    let result = if startable {
        let turn = start_turn(hub, &mut st, &id, submission.clone(), params.prompt);
        SubmitResult {
            submission_id: submission,
            queued: false,
            position: 0,
            turn: Some(turn),
        }
    } else {
        let needs_load = st
            .sessions
            .get(&id)
            .is_some_and(|s| matches!(s.phase, Phase::Listed) || s.transcript.is_none());
        let Some(s) = st.sessions.get_mut(&id) else {
            return Err(AcpError::NotFound(format!("unknown session {id}")));
        };
        s.queue.push_back(Queued {
            submission: submission.clone(),
            client_submission: params.client_submission_id.clone(),
            prompt: params.prompt,
            queued_at: tokio::time::Instant::now(),
        });
        s.touch();
        let position = u32::try_from(s.queue.len()).unwrap_or(u32::MAX);
        if needs_load && !matches!(s.phase, Phase::Loading | Phase::Running { .. } | Phase::Closing)
        {
            if let Err(e) = begin_load(hub, &mut st, &id, false) {
                if let Some(s) = st.sessions.get_mut(&id) {
                    s.queue.retain(|q| q.submission != submission);
                }
                return Err(e);
            }
        }
        SubmitResult {
            submission_id: submission,
            queued: true,
            position,
            turn: None,
        }
    };
    if let Some(s) = st.sessions.get_mut(&id) {
        s.submissions
            .insert(params.client_submission_id, result.clone());
    }
    Ok(result)
}

/// Start a live turn of `id` (session must be idle and loaded).
fn start_turn(
    hub: &Arc<AcpHub>,
    st: &mut HubState,
    id: &str,
    submission: String,
    prompt: Vec<ContentBlock>,
) -> u32 {
    let now = now_ms();
    let (peer, run_id) = match st.run.as_ref() {
        Some(run) => (run.peer.clone(), run.id),
        None => return 0,
    };
    let Some(s) = st.sessions.get_mut(id) else { return 0 };
    let Some(t) = s.transcript.as_mut() else { return 0 };
    let (turn, user_item_id) = t.begin_live_turn(&prompt, now);
    let user_item = t.item(&user_item_id).cloned();
    let generation = t.generation().to_string();
    s.phase = Phase::Running {
        turn,
        submission: submission.clone(),
    };
    s.touch();
    let session_id = id.to_string();
    let first = prompt
        .first()
        .cloned()
        .unwrap_or_else(|| ContentBlock::text(""));
    let item_id = user_item_id.clone();
    st.notify_session(id, methods::SESSION_UPDATE, |seq| {
        let meta = pcx::UpdateMeta {
            seq,
            item_id: Some(item_id),
            created: Some(true),
            turn,
            generation,
            item: user_item,
        };
        with_pcx(
            &json!({"sessionId": session_id, "update": {"sessionUpdate": "user_message_chunk", "content": first}}),
            serde_json::to_value(meta).unwrap_or(Value::Null),
        )
    });
    let started = TurnStartedParams {
        session_id: id.to_string(),
        turn,
        submission_id: submission.clone(),
        user_item_id,
        started_at_ms: now,
        seq: 0,
    };
    st.notify_session(id, notifications::TURN_STARTED, |seq| {
        serde_json::to_value(TurnStartedParams {
            seq,
            ..started
        })
        .unwrap_or(Value::Null)
    });
    let hub = hub.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        let params = json!({ "sessionId": id, "prompt": prompt });
        let result = peer.request(methods::SESSION_PROMPT, params, None).await;
        let mut st = lock(&hub.state);
        if st.run.as_ref().map(|r| r.id) != Some(run_id) {
            return;
        }
        let current = st.sessions.get(&id).is_some_and(
            |s| matches!(&s.phase, Phase::Running { submission: q, .. } if *q == submission),
        );
        if !current {
            return;
        }
        match result.and_then(parse::<PromptResponse>) {
            // The process went away; its exit cleanup ends the turn.
            Err(AcpError::AgentUnavailable(_)) => return,
            Ok(response) => inbound::finish_turn(&mut st, &id, &response.stop_reason, None),
            Err(e) => {
                if e.is_auth_required() {
                    st.auth_required(Some(e.detail()));
                }
                inbound::finish_turn(
                    &mut st,
                    &id,
                    pcx::stop_reason::ERROR,
                    Some(e.to_rpc().message),
                );
            },
        }
        drain_queue(&hub, &mut st, &id);
    });
    turn
}

/// Start the next queued prompt of an idle, loaded session.
fn drain_queue(hub: &Arc<AcpHub>, st: &mut HubState, id: &str) {
    let next = match st.sessions.get_mut(id) {
        Some(s) if matches!(s.phase, Phase::Idle) && s.agent_loaded && s.transcript.is_some() => {
            s.queue.pop_front()
        },
        _ => None,
    };
    if let Some(next) = next {
        start_turn(hub, st, id, next.submission, next.prompt);
    }
}

fn cancel(hub: &Arc<AcpHub>, id: &str) {
    let mut st = lock(&hub.state);
    let running = st.sessions.get(id).is_some_and(HubSession::running);
    if !running {
        return;
    }
    if let Some(run) = st.run.as_ref() {
        let _ = run
            .peer
            .notify_now(methods::SESSION_CANCEL, json!({ "sessionId": id }));
    }
    inbound::cancel_pending(&mut st, id);
    inbound::fail_queue(&mut st, id, "cancelled");
}

fn running(hub: &Arc<AcpHub>) -> Result<Value, AcpError> {
    let st = lock(&hub.state);
    let sessions = st
        .sessions
        .values()
        .map(|s| RunningSession {
            session_id: s.id.clone(),
            running: s.running(),
            pending: st.pending_count(&s.id),
            queue: u32::try_from(s.queue.len()).unwrap_or(u32::MAX),
        })
        .filter(|r| r.running || r.pending > 0 || r.queue > 0)
        .collect();
    to_value(&RunningResult {
        sessions,
    })
}

// ------------------------------------------------------- standard methods

async fn standard_load(hub: &Arc<AcpHub>, conn: u64, request: RequestId, params: Value) {
    let result = async {
        let p: SessionParams = parse(params.clone())?;
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string);
        ensure_known(hub, &p.session_id, cwd).await?;
        let rx = {
            let mut st = lock(&hub.state);
            begin_load(hub, &mut st, &p.session_id, false)?
        };
        if !wait_load(rx, LOAD_TIMEOUT).await? {
            return Err(AcpError::Timeout("session/load is taking too long".into()));
        }
        Ok(p.session_id)
    }
    .await;
    let mut st = lock(&hub.state);
    let id = match result {
        Ok(id) => id,
        Err(e) => {
            st.send(conn, RpcMessage::Response {
                id: request,
                result: Err(e.to_rpc()),
            });
            return;
        },
    };
    let Some(s) = st.sessions.get_mut(&id) else { return };
    s.subscribers.insert(conn);
    let updates: Vec<Value> = s
        .transcript
        .as_ref()
        .map(|t| t.items().iter().flat_map(replay_updates).collect())
        .unwrap_or_default();
    let response = json!({ "configOptions": s.config_options, "modes": s.modes });
    for update in updates {
        st.send(conn, RpcMessage::Notification {
            method: methods::SESSION_UPDATE.into(),
            params: json!({ "sessionId": id, "update": update }),
        });
    }
    st.send(conn, RpcMessage::Response {
        id: request,
        result: Ok(response),
    });
    resend_pending(&mut st, &id, conn);
}

/// `session/update` payloads that rebuild `item` for a standard client.
fn replay_updates(item: &HubItem) -> Vec<Value> {
    use pocket_codex_core::acp::transcript::kind;
    let chunk = |tag: &str| -> Vec<Value> {
        item.content
            .iter()
            .map(|block| json!({"sessionUpdate": tag, "content": block, "messageId": item.message_id}))
            .collect()
    };
    match item.kind.as_str() {
        kind::USER => chunk("user_message_chunk"),
        kind::AGENT => chunk("agent_message_chunk"),
        kind::THOUGHT => chunk("agent_thought_chunk"),
        kind::TOOL => item
            .tool
            .as_ref()
            .and_then(|t| serde_json::to_value(t).ok())
            .map(|mut v| {
                v["sessionUpdate"] = json!("tool_call");
                vec![v]
            })
            .unwrap_or_default(),
        kind::PLAN => vec![json!({"sessionUpdate": "plan", "entries": item.plan})],
        _ => Vec::new(),
    }
}

async fn standard_prompt(hub: &Arc<AcpHub>, params: Value) -> Result<Value, AcpError> {
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| AcpError::InvalidParams("sessionId is required".into()))?
        .to_string();
    let prompt: Vec<ContentBlock> =
        parse(params.get("prompt").cloned().unwrap_or_else(|| json!([])))?;
    let request = SubmitParams {
        session_id,
        prompt,
        client_submission_id: uuid::Uuid::new_v4().to_string(),
    };
    let (tx, rx) = oneshot::channel();
    let submitted = submit_inner(hub, request)?;
    {
        let mut st = lock(&hub.state);
        st.turn_waiters
            .entry(submitted.submission_id.clone())
            .or_default()
            .push(tx);
    }
    let stop = rx
        .await
        .map_err(|_| AcpError::AgentUnavailable("the turn was abandoned".into()))??;
    to_value(&PromptResponse {
        stop_reason: stop,
        meta: None,
    })
}

async fn set_config(hub: &Arc<AcpHub>, method: &str, params: Value) -> Result<Value, AcpError> {
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| AcpError::InvalidParams("sessionId is required".into()))?
        .to_string();
    let peer = lock(&hub.state).ready_run()?.peer.clone();
    let value = match tokio::time::timeout(
        CONFIG_DEADLINE,
        peer.request(method, params.clone(), Some(CONFIG_DEADLINE)),
    )
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(e)) => {
            if e.is_auth_required() {
                lock(&hub.state).auth_required(Some(e.detail()));
            }
            return Err(e);
        },
        Err(_) => return Err(AcpError::Timeout(format!("{method} timed out"))),
    };
    let mut st = lock(&hub.state);
    let update = if method == methods::SESSION_SET_CONFIG_OPTION {
        let response: SetConfigOptionResponse = parse(value.clone()).unwrap_or_default();
        if let Some(s) = st.sessions.get_mut(&session_id) {
            s.config_options = response.config_options.clone();
        }
        if !response.config_options.is_empty() {
            st.default_config_options = response.config_options.clone();
            super::defaults::save(&mut st, &hub.options.state_dir, &hub.options.instance);
        }
        json!({"sessionUpdate": "config_option_update", "configOptions": response.config_options})
    } else {
        let mode = params
            .get("modeId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(modes) = st
            .sessions
            .get_mut(&session_id)
            .and_then(|s| s.modes.as_mut())
        {
            modes.current_mode_id = mode.clone();
        }
        json!({"sessionUpdate": "current_mode_update", "currentModeId": mode})
    };
    let (turn, generation) = st
        .sessions
        .get(&session_id)
        .and_then(|s| s.transcript.as_ref())
        .map(|t| (t.current_turn(), t.generation().to_string()))
        .unwrap_or_default();
    let sid = session_id.clone();
    st.notify_session(&session_id, methods::SESSION_UPDATE, |seq| {
        let meta = pcx::UpdateMeta {
            seq,
            item_id: None,
            created: None,
            turn,
            generation,
            item: None,
        };
        json!({"sessionId": sid, "update": update, "_meta": {"pcx": meta}})
    });
    Ok(value)
}

// ------------------------------------------------------------ auth flows

/// Start an agent-type `authenticate` in the background.
pub(super) fn start_authenticate(
    hub: &Arc<AcpHub>,
    method_id: &str,
) -> Result<AuthState, AcpError> {
    let (peer, run_id, description) = {
        let mut st = lock(&hub.state);
        let run = st.ready_run()?;
        let (peer, run_id) = (run.peer.clone(), run.id);
        let description = st
            .auth
            .methods
            .iter()
            .find(|m| m.id == method_id)
            .map(|m| m.description.clone())
            .ok_or_else(|| AcpError::InvalidParams(format!("unknown method {method_id}")))?;
        st.auth.status = auth_status::IN_PROGRESS.into();
        st.auth.message = None;
        st.broadcast_hub_state();
        (peer, run_id, description)
    };
    let state = lock(&hub.state).auth.clone();
    let hub = hub.clone();
    let method_id = method_id.to_string();
    tokio::spawn(async move {
        let result = peer
            .request(methods::AUTHENTICATE, json!({ "methodId": method_id }), Some(AUTH_TIMEOUT))
            .await;
        {
            let mut st = lock(&hub.state);
            st.auth.status = auth_status::UNKNOWN.into();
            if let Err(e) = &result {
                st.auth.message = Some(e.detail());
            }
        }
        hub.detect_auth(run_id).await;
        let mut st = lock(&hub.state);
        if result.is_ok() && st.auth.status == auth_status::REQUIRED && !description.is_empty() {
            st.auth.message = Some(description);
            st.broadcast_hub_state();
        }
    });
    Ok(state)
}

/// Open the host terminal for a terminal-type method and watch its exit code.
pub(super) fn terminal_login(hub: &Arc<AcpHub>, method_id: &str) -> Result<(), AcpError> {
    let launch = {
        let st = lock(&hub.state);
        let run = st.ready_run()?;
        let init = run
            .init
            .as_ref()
            .ok_or_else(|| AcpError::AgentUnavailable("not ready".into()))?;
        let method = init
            .auth_methods
            .iter()
            .find(|m| m.id == method_id)
            .ok_or_else(|| AcpError::InvalidParams(format!("unknown method {method_id}")))?;
        auth::terminal_launch(method, &run.spec)?
    };
    let Some(terminal) = hub.options.terminal.clone() else {
        return Err(AcpError::NoTerminal {
            command: launch.command_line(),
        });
    };
    let run_dir = hub.options.state_dir.join("run");
    std::fs::create_dir_all(&run_dir)?;
    let status = run_dir.join(format!("{}.status", uuid::Uuid::new_v4().simple()));
    terminal.open(&launch, &status)?;
    let weak = Arc::downgrade(hub);
    tokio::spawn(watch_terminal(weak, status));
    Ok(())
}

async fn watch_terminal(hub: Weak<AcpHub>, status: PathBuf) {
    let deadline = tokio::time::Instant::now() + TERMINAL_MAX;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(TERMINAL_POLL).await;
        let Some(text) = std::fs::read_to_string(&status)
            .ok()
            .filter(|t| !t.trim().is_empty())
        else {
            continue;
        };
        let _ = std::fs::remove_file(&status);
        let Some(hub) = hub.upgrade() else { return };
        let code = text.trim().parse::<i32>().unwrap_or(-1);
        if code == 0 {
            let _ = hub.restart().await;
        } else {
            let mut st = lock(&hub.state);
            st.auth.message = Some(format!("登录未完成（退出码 {code}）"));
            st.broadcast_hub_state();
        }
        return;
    }
}

// ------------------------------------------------- idle reclaim and LRU

/// Close idle sessions every `every` (TRD §4.2.4).
pub(super) async fn idle_loop(hub: Weak<AcpHub>, every: Duration) {
    let mut interval = tokio::time::interval(every);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;
    loop {
        interval.tick().await;
        let Some(hub) = hub.upgrade() else { return };
        if hub.shutdown.is_cancelled() {
            return;
        }
        close_idle(&hub);
    }
}

fn close_idle(hub: &Arc<AcpHub>) {
    let mut st = lock(&hub.state);
    if !st.caps.close {
        return;
    }
    let Ok(run) = st.ready_run() else { return };
    let (peer, run_id) = (run.peer.clone(), run.id);
    let ids: Vec<String> = st
        .sessions
        .values()
        .filter(|s| {
            matches!(s.phase, Phase::Idle)
                && s.agent_loaded
                && s.subscribers.is_empty()
                && s.queue.is_empty()
                && s.last_activity.elapsed() >= IDLE_CLOSE_AFTER
        })
        .map(|s| s.id.clone())
        .filter(|id| st.pending_count(id) == 0)
        .collect();
    for id in ids {
        if let Some(s) = st.sessions.get_mut(&id) {
            s.phase = Phase::Closing;
        }
        tokio::spawn(close_session(hub.clone(), peer.clone(), run_id, id, false));
    }
}

async fn close_session(
    hub: Arc<AcpHub>,
    peer: Arc<super::peer::AgentPeer>,
    run_id: u64,
    id: String,
    drop_transcript: bool,
) {
    let result = peer
        .request(methods::SESSION_CLOSE, json!({ "sessionId": id }), Some(CLOSE_TIMEOUT))
        .await;
    let mut st = lock(&hub.state);
    let stale = st.run.as_ref().map(|r| r.id) != Some(run_id);
    let Some(s) = st.sessions.get_mut(&id) else { return };
    if !matches!(s.phase, Phase::Closing) {
        return;
    }
    if result.is_ok() || stale {
        s.agent_loaded = false;
        s.phase = Phase::Listed;
        if drop_transcript {
            s.transcript = None;
        }
    } else {
        s.phase = Phase::Idle;
    }
}

/// Evict least recently used transcripts beyond 16 / 256 MiB.
fn enforce_lru(hub: &Arc<AcpHub>, st: &mut HubState) {
    loop {
        let with: Vec<&HubSession> = st
            .sessions
            .values()
            .filter(|s| s.transcript.is_some())
            .collect();
        let bytes: usize = with
            .iter()
            .filter_map(|s| s.transcript.as_ref())
            .map(Transcript::approx_bytes)
            .sum();
        if with.len() <= MAX_TRANSCRIPTS && bytes <= MAX_TRANSCRIPT_BYTES {
            return;
        }
        let candidate = with
            .iter()
            .filter(|s| {
                !s.running()
                    && s.subscribers.is_empty()
                    && matches!(s.phase, Phase::Idle | Phase::Listed)
                    && (!s.agent_loaded || st.caps.close)
            })
            .filter(|s| {
                st.pending
                    .values()
                    .all(|p| p.session_id.as_deref() != Some(s.id.as_str()))
            })
            .min_by_key(|s| s.last_access)
            .map(|s| (s.id.clone(), s.agent_loaded));
        let Some((id, loaded)) = candidate else {
            warn!("ACP transcript cache is over budget and nothing can be evicted");
            return;
        };
        if loaded {
            let Ok(run) = st.ready_run() else { return };
            let (peer, run_id) = (run.peer.clone(), run.id);
            if let Some(s) = st.sessions.get_mut(&id) {
                s.phase = Phase::Closing;
            }
            tokio::spawn(close_session(hub.clone(), peer, run_id, id.clone(), true));
            // Count it as evicted for this pass.
            if let Some(s) = st.sessions.get_mut(&id) {
                s.transcript = None;
            }
        } else if let Some(s) = st.sessions.get_mut(&id) {
            s.transcript = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_sort_newest_first_with_missing_last() {
        let info = |id: &str, at: Option<&str>| SessionInfo {
            session_id: id.into(),
            updated_at: at.map(str::to_string),
            ..SessionInfo::default()
        };
        let mut all = [
            info("a", Some("2026-01-01T00:00:00Z")),
            info("b", None),
            info("c", Some("2026-01-02T00:00:00+08:00")),
            info("d", Some("2026-01-02T00:00:00Z")),
        ];
        all.sort_by(|a, b| match (sort_key(a), sort_key(b)) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        let ids: Vec<_> = all.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["d", "c", "a", "b"]);
    }
}

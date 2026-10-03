//! Test doubles (TRD §8.1): a scripted in-process ACP agent over
//! `tokio::io::duplex`.
//!
//! [`FakeAgent::spawn`] returns a [`DuplexConnector`] (every `connect` starts
//! a fresh agent instance, as a restarted process would) and a [`FakeHandle`]
//! that records everything the hub sent and drives the agent from a test.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use pocket_codex_core::acp::{
    rpc::{self, code, RequestId, RpcError, RpcMessage},
    SessionInfo,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, Notify},
};
use tokio_util::sync::CancellationToken;

use super::{
    error::AcpError,
    launch::{AgentConnector, AgentIo, LaunchSpec},
};

/// One step of a scripted prompt turn.
#[derive(Clone, Debug)]
pub enum Step {
    /// Send `session/update` with this `update` payload.
    Update(Value),
    /// Send `session/request_permission` and wait for the answer.
    RequestPermission {
        /// Permission options.
        options: Vec<Value>,
    },
    /// Send `elicitation/create` with these params and wait for the answer.
    Elicit(Value),
    /// Send an arbitrary request to the client and wait for the answer.
    Call {
        /// Method.
        method: String,
        /// Params.
        params: Value,
    },
    /// Wait.
    Sleep(u64),
    /// Answer `session/prompt` with this stop reason.
    Finish(String),
    /// Close the connection, like a crashing process.
    Crash,
    /// Write raw bytes (a line without validation).
    Raw(Vec<u8>),
}

/// Behaviour of the fake agent.
#[derive(Clone, Debug)]
pub struct FakeScript {
    /// `initialize` result.
    pub initialize: Value,
    /// Sessions returned by `session/list`.
    pub sessions: Vec<SessionInfo>,
    /// `session/list` page size.
    pub page_size: usize,
    /// `update` payloads replayed by `session/load`, per session.
    pub replays: HashMap<String, Vec<Value>>,
    /// Delay before `session/load` answers.
    pub load_delay_ms: u64,
    /// `session/list` and `session/new` fail with `-32000` until
    /// `authenticate` succeeds.
    pub requires_auth: bool,
    /// `authenticate` fails with this message.
    pub authenticate_error: Option<String>,
    /// Steps of successive prompts; an empty queue finishes with `end_turn`.
    pub prompts: VecDeque<Vec<Step>>,
    /// Extra fields of the `session/new`, `session/load` and
    /// `session/resume` responses (`configOptions`, `modes`).
    pub session_setup: Value,
    /// Instances crash right after `initialize` while this is positive
    /// (decremented per instance).
    pub crash_after_initialize: u32,
}

impl Default for FakeScript {
    fn default() -> Self {
        Self {
            initialize: default_initialize(),
            sessions: Vec::new(),
            page_size: 50,
            replays: HashMap::new(),
            load_delay_ms: 0,
            requires_auth: false,
            authenticate_error: None,
            prompts: VecDeque::new(),
            session_setup: json!({}),
            crash_after_initialize: 0,
        }
    }
}

/// An `initialize` result with list, load, resume and close support.
pub fn default_initialize() -> Value {
    json!({
        "protocolVersion": 1,
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": {"image": true, "audio": false, "embeddedContext": true},
            "sessionCapabilities": {"list": {}, "resume": {}, "close": {}}
        },
        "authMethods": [{"id": "login", "name": "Log in", "description": "Run `fake login`"}],
        "agentInfo": {"name": "fake-agent", "version": "1.0.0"}
    })
}

#[derive(Default)]
struct FakeState {
    script: FakeScript,
    received: Vec<(String, Value)>,
    answers: Vec<(String, Result<Value, RpcError>)>,
    connects: u32,
    instance: Option<Instance>,
    next_session: u32,
    authenticated: bool,
}

#[derive(Clone)]
struct Instance {
    out: mpsc::UnboundedSender<Vec<u8>>,
    crash: CancellationToken,
    pending: Waiters,
    next_id: Arc<Mutex<i64>>,
    prompts: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl Instance {
    fn write(&self, message: &RpcMessage) {
        let _ = self.out.send(rpc::encode_line(message));
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = {
            let mut next = lock(&self.next_id);
            *next += 1;
            *next
        };
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, (method.to_string(), tx));
        self.write(&RpcMessage::Request {
            id: RequestId::Number(id),
            method: method.to_string(),
            params,
        });
        rx.await
            .unwrap_or_else(|_| Err(RpcError::new(code::INTERNAL_ERROR, "closed")))
    }
}

type Waiters = Arc<Mutex<HashMap<i64, (String, oneshot::Sender<Result<Value, RpcError>>)>>>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Inspects and drives a fake agent.
#[derive(Clone)]
pub struct FakeHandle {
    state: Arc<Mutex<FakeState>>,
    changed: Arc<Notify>,
}

/// Starts a fresh fake agent on every `connect`.
#[derive(Clone)]
pub struct DuplexConnector {
    handle: FakeHandle,
    fail: Arc<Mutex<Option<AcpError>>>,
}

/// Entry point of the fake agent.
pub struct FakeAgent;

impl FakeAgent {
    /// A connector and the handle observing it.
    pub fn spawn(script: FakeScript) -> (DuplexConnector, FakeHandle) {
        let handle = FakeHandle {
            state: Arc::new(Mutex::new(FakeState {
                script,
                ..FakeState::default()
            })),
            changed: Arc::new(Notify::new()),
        };
        (
            DuplexConnector {
                handle: handle.clone(),
                fail: Arc::new(Mutex::new(None)),
            },
            handle,
        )
    }
}

impl DuplexConnector {
    /// Make the next `connect` calls fail with `error` (`None` to stop).
    pub fn fail_with(&self, error: Option<AcpError>) {
        *lock(&self.fail) = error;
    }
}

#[async_trait]
impl AgentConnector for DuplexConnector {
    async fn connect(&self, _spec: &LaunchSpec) -> Result<AgentIo, AcpError> {
        if let Some(error) = lock(&self.fail).clone() {
            return Err(error);
        }
        let (hub_side, agent_side) = tokio::io::duplex(64 * 1024 * 1024);
        let (hub_read, hub_write) = tokio::io::split(hub_side);
        let (agent_read, agent_write) = tokio::io::split(agent_side);
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let instance = Instance {
            out: out_tx,
            crash: CancellationToken::new(),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1000)),
            prompts: Arc::new(Mutex::new(HashMap::new())),
        };
        {
            let mut st = lock(&self.handle.state);
            st.connects += 1;
            st.instance = Some(instance.clone());
        }
        self.handle.changed.notify_waiters();
        let crash = instance.crash.clone();
        tokio::spawn(async move {
            let mut writer = agent_write;
            loop {
                tokio::select! {
                    line = out_rx.recv() => match line {
                        Some(line) => {
                            if writer.write_all(&line).await.is_err() { break; }
                            let _ = writer.flush().await;
                        },
                        None => break,
                    },
                    _ = crash.cancelled() => break,
                }
            }
            let _ = writer.shutdown().await;
        });
        let handle = self.handle.clone();
        tokio::spawn(async move { handle.serve(instance, agent_read).await });
        Ok(AgentIo {
            reader: Box::new(hub_read),
            writer: Box::new(hub_write),
            stderr: None,
            child: None,
        })
    }
}

impl FakeHandle {
    /// Everything the hub sent, as (method, params).
    pub fn received(&self) -> Vec<(String, Value)> {
        lock(&self.state).received.clone()
    }

    /// Methods the hub sent, in order.
    pub fn methods(&self) -> Vec<String> {
        lock(&self.state)
            .received
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }

    /// How many times the hub sent `method`.
    pub fn count(&self, method: &str) -> usize {
        lock(&self.state)
            .received
            .iter()
            .filter(|(m, _)| m == method)
            .count()
    }

    /// Params of every `method` call.
    pub fn params_of(&self, method: &str) -> Vec<Value> {
        lock(&self.state)
            .received
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Answers the client gave to agent requests, as (method, result).
    pub fn answers(&self) -> Vec<(String, Result<Value, RpcError>)> {
        lock(&self.state).answers.clone()
    }

    /// An agent instance is connected (its stdin is still open).
    pub fn running(&self) -> bool {
        lock(&self.state).instance.is_some()
    }

    /// Number of agent instances started.
    pub fn connects(&self) -> u32 {
        lock(&self.state).connects
    }

    /// Change the script for later requests.
    pub fn update_script(&self, edit: impl FnOnce(&mut FakeScript)) {
        edit(&mut lock(&self.state).script);
    }

    /// Kill the current instance (EOF on the hub side).
    pub fn crash(&self) {
        if let Some(instance) = lock(&self.state).instance.take() {
            instance.crash.cancel();
        }
    }

    /// Send a notification from the current instance.
    pub fn notify(&self, method: &str, params: Value) {
        if let Some(instance) = lock(&self.state).instance.clone() {
            instance.write(&RpcMessage::Notification {
                method: method.into(),
                params,
            });
        }
    }

    /// Send a request from the current instance and wait for the answer.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let instance = lock(&self.state).instance.clone();
        match instance {
            Some(instance) => instance.call(method, params).await,
            None => Err(RpcError::new(code::INTERNAL_ERROR, "no instance")),
        }
    }

    /// Wait (up to 30 s) until `method` was received `n` times.
    pub async fn wait_for(&self, method: &str, n: usize) {
        self.wait_until(|h| h.count(method) >= n).await;
    }

    /// Wait (up to 30 s) until `connects() >= n`.
    pub async fn wait_connects(&self, n: u32) {
        self.wait_until(|h| h.connects() >= n).await;
    }

    /// Wait (up to 30 s) until `n` answers arrived.
    pub async fn wait_answers(&self, n: usize) {
        self.wait_until(|h| h.answers().len() >= n).await;
    }

    async fn wait_until(&self, done: impl Fn(&Self) -> bool) {
        let _ = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let notified = self.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if done(self) {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }

    fn record(&self, method: &str, params: &Value) {
        lock(&self.state)
            .received
            .push((method.to_string(), params.clone()));
        self.changed.notify_waiters();
    }

    fn record_answer(&self, method: &str, result: Result<Value, RpcError>) {
        lock(&self.state).answers.push((method.to_string(), result));
        self.changed.notify_waiters();
    }

    async fn serve(self, instance: Instance, reader: tokio::io::ReadHalf<tokio::io::DuplexStream>) {
        let mut lines = BufReader::new(reader).lines();
        loop {
            let line = tokio::select! {
                line = lines.next_line() => line,
                _ = instance.crash.cancelled() => break,
            };
            let Ok(Some(line)) = line else { break };
            let Ok(message) = rpc::decode(line.as_bytes()) else { continue };
            match message {
                RpcMessage::Response {
                    id: RequestId::Number(n),
                    result,
                } => {
                    if let Some((method, tx)) = lock(&instance.pending).remove(&n) {
                        self.record_answer(&method, result.clone());
                        let _ = tx.send(result);
                    }
                },
                RpcMessage::Response {
                    ..
                } => {},
                RpcMessage::Notification {
                    method,
                    params,
                } => {
                    self.record(&method, &params);
                    if method == "session/cancel" {
                        let session = params["sessionId"].as_str().unwrap_or_default().to_string();
                        if let Some(token) = lock(&instance.prompts).get(&session) {
                            token.cancel();
                        }
                    }
                },
                RpcMessage::Request {
                    id,
                    method,
                    params,
                } => {
                    self.record(&method, &params);
                    let handle = self.clone();
                    let instance = instance.clone();
                    tokio::spawn(async move {
                        let result = handle.answer(&instance, &method, params).await;
                        if let Some(result) = result {
                            instance.write(&RpcMessage::Response {
                                id,
                                result,
                            });
                        }
                    });
                },
            }
        }
        instance.crash.cancel();
        let mut st = lock(&self.state);
        if st.instance.as_ref().is_some_and(|i| i.crash.is_cancelled()) {
            st.instance = None;
        }
    }

    async fn answer(
        &self,
        instance: &Instance,
        method: &str,
        params: Value,
    ) -> Option<Result<Value, RpcError>> {
        let auth_error = || Err(RpcError::new(code::AUTH_REQUIRED, "Authentication required"));
        let needs_auth = {
            let st = lock(&self.state);
            st.script.requires_auth && !st.authenticated
        };
        match method {
            "initialize" => {
                let (init, crash) = {
                    let mut st = lock(&self.state);
                    let crash = st.script.crash_after_initialize > 0;
                    if crash {
                        st.script.crash_after_initialize -= 1;
                    }
                    (st.script.initialize.clone(), crash)
                };
                if crash {
                    let instance = instance.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        instance.crash.cancel();
                    });
                }
                Some(Ok(init))
            },
            "authenticate" => {
                let error = lock(&self.state).script.authenticate_error.clone();
                match error {
                    Some(message) => Some(Err(RpcError::new(code::INTERNAL_ERROR, message))),
                    None => {
                        lock(&self.state).authenticated = true;
                        Some(Ok(json!({})))
                    },
                }
            },
            "session/list" if needs_auth => Some(auth_error()),
            "session/new" if needs_auth => Some(auth_error()),
            "session/list" => {
                let st = lock(&self.state);
                let offset: usize = params["cursor"]
                    .as_str()
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
                let size = st.script.page_size.max(1);
                let page: Vec<&SessionInfo> =
                    st.script.sessions.iter().skip(offset).take(size).collect();
                let next = offset + page.len();
                let mut result = json!({ "sessions": page });
                if next < st.script.sessions.len() {
                    result["nextCursor"] = json!(next.to_string());
                }
                Some(Ok(result))
            },
            "session/new" => {
                let mut st = lock(&self.state);
                st.next_session += 1;
                let id = format!("fake-{}", st.next_session);
                let mut result = st.script.session_setup.clone();
                result["sessionId"] = json!(id);
                Some(Ok(result))
            },
            "session/load" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_string();
                let (replay, delay, setup) = {
                    let st = lock(&self.state);
                    (
                        st.script.replays.get(&session).cloned().unwrap_or_default(),
                        st.script.load_delay_ms,
                        st.script.session_setup.clone(),
                    )
                };
                for update in replay {
                    instance.write(&RpcMessage::Notification {
                        method: "session/update".into(),
                        params: json!({ "sessionId": session, "update": update }),
                    });
                }
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Some(Ok(setup))
            },
            "session/resume" => Some(Ok(lock(&self.state).script.session_setup.clone())),
            "session/close" | "session/set_mode" => Some(Ok(json!({}))),
            "session/set_config_option" => {
                let mut st = lock(&self.state);
                let scripted = st
                    .script
                    .session_setup
                    .get_mut("configOptions")
                    .and_then(Value::as_array_mut);
                match scripted {
                    // Scripted options: update the current value and return all of them.
                    Some(options) => {
                        for option in options.iter_mut() {
                            if option["id"] == params["configId"] {
                                option["currentValue"] = params["value"].clone();
                            }
                        }
                        Some(Ok(json!({ "configOptions": options.clone() })))
                    },
                    None => Some(Ok(json!({
                        "configOptions": [{"id": params["configId"], "name": "Option", "type": "select",
                            "currentValue": params["value"], "options": [{"value": params["value"], "name": "v"}]}]
                    }))),
                }
            },
            "session/prompt" => Some(self.prompt(instance, params).await),
            _ => {
                Some(Err(RpcError::new(code::METHOD_NOT_FOUND, format!("{method} not supported"))))
            },
        }
    }

    async fn prompt(&self, instance: &Instance, params: Value) -> Result<Value, RpcError> {
        let session = params["sessionId"].as_str().unwrap_or_default().to_string();
        let cancel = CancellationToken::new();
        lock(&instance.prompts).insert(session.clone(), cancel.clone());
        let steps = lock(&self.state)
            .script
            .prompts
            .pop_front()
            .unwrap_or_default();
        let finish = |reason: &str| Ok(json!({ "stopReason": reason }));
        for step in steps {
            if cancel.is_cancelled() {
                return finish("cancelled");
            }
            match step {
                Step::Update(update) => instance.write(&RpcMessage::Notification {
                    method: "session/update".into(),
                    params: json!({ "sessionId": session, "update": update }),
                }),
                Step::RequestPermission {
                    options,
                } => {
                    let params = json!({
                        "sessionId": session,
                        "toolCall": {"toolCallId": "perm-tool", "title": "Run"},
                        "options": options,
                    });
                    tokio::select! {
                        _ = instance.call("session/request_permission", params) => {},
                        _ = cancel.cancelled() => return finish("cancelled"),
                    }
                },
                Step::Elicit(params) => {
                    tokio::select! {
                        _ = instance.call("elicitation/create", params) => {},
                        _ = cancel.cancelled() => return finish("cancelled"),
                    }
                },
                Step::Call {
                    method,
                    params,
                } => {
                    let _ = instance.call(&method, params).await;
                },
                Step::Sleep(ms) => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(ms)) => {},
                        _ = cancel.cancelled() => return finish("cancelled"),
                    }
                },
                Step::Finish(reason) => return finish(&reason),
                Step::Crash => {
                    instance.crash.cancel();
                    return Err(RpcError::new(code::INTERNAL_ERROR, "crashed"));
                },
                Step::Raw(bytes) => {
                    let _ = instance.out.send(bytes);
                },
            }
        }
        if cancel.is_cancelled() {
            return finish("cancelled");
        }
        finish("end_turn")
    }
}

//! ACP hub protocol tests (TRD §8.2 "Hub"), driven through
//! `AcpHub::open_connection` without a WebSocket (T18).

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use pocket_codex_core::acp::{
    pcx::ProcessState,
    rpc::{code, RequestId, RpcError, RpcMessage},
    SessionInfo,
};
use pocket_codex_host_svc::acp::{
    testing::{FakeAgent, FakeHandle, FakeScript, Step},
    AcpError, AcpHub, GatewayAuth, HubConnection, HubOptions, LaunchSpec, TerminalLaunch,
    TerminalLauncher,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(30);

fn spec() -> LaunchSpec {
    LaunchSpec {
        agent_id: "fake".into(),
        display_name: "Fake Agent".into(),
        program: PathBuf::from("/opt/fake/fake-agent"),
        args: vec!["dist/index.js".into()],
        version: Some("1.0.0".into()),
        pinned: true,
        launch_only_args: vec!["--hide-claude-auth".into()],
        ..LaunchSpec::default()
    }
}

type Launch = Arc<Mutex<(LaunchSpec, Option<GatewayAuth>)>>;

struct Setup {
    hub: Arc<AcpHub>,
    fake: FakeHandle,
    launch: Launch,
    _dir: TempDir,
}

fn options(
    script: FakeScript,
    launch: Launch,
    terminal: Option<Arc<dyn TerminalLauncher>>,
) -> (HubOptions, FakeHandle, TempDir) {
    let (connector, fake) = FakeAgent::spawn(script);
    let dir = tempfile::tempdir().expect("tempdir");
    let provider = launch.clone();
    let options = HubOptions {
        instance: "test".into(),
        launch: Arc::new(move || Ok(provider.lock().expect("launch").clone())),
        state_dir: dir.path().join("acp"),
        log_file: None,
        connector: Arc::new(connector),
        terminal,
    };
    (options, fake, dir)
}

async fn start_with(
    script: FakeScript,
    gateway: Option<GatewayAuth>,
    terminal: Option<Arc<dyn TerminalLauncher>>,
) -> Setup {
    let launch: Launch = Arc::new(Mutex::new((spec(), gateway)));
    let (options, fake, dir) = options(script, launch.clone(), terminal);
    let hub = AcpHub::start(options).await.expect("hub starts");
    Setup {
        hub,
        fake,
        launch,
        _dir: dir,
    }
}

async fn start(script: FakeScript) -> Setup {
    start_with(script, None, None).await
}

fn listed(id: &str, cwd: &str, updated: Option<&str>) -> SessionInfo {
    SessionInfo {
        session_id: id.into(),
        cwd: cwd.into(),
        title: Some(format!("title {id}")),
        updated_at: updated.map(str::to_string),
        meta: None,
    }
}

fn agent_chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
}

fn user_chunk(text: &str) -> Value {
    json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}})
}

fn permission_options() -> Vec<Value> {
    vec![
        json!({"optionId": "allow", "name": "Allow", "kind": "allow_once"}),
        json!({"optionId": "deny", "name": "Deny", "kind": "reject_once"}),
    ]
}

/// A controller driving one hub connection.
struct Ctl {
    conn: HubConnection,
    rx: mpsc::Receiver<RpcMessage>,
    inbox: VecDeque<RpcMessage>,
    next: i64,
}

impl Ctl {
    fn open(hub: &Arc<AcpHub>) -> Self {
        let (conn, rx) = hub.open_connection();
        Self {
            conn,
            rx,
            inbox: VecDeque::new(),
            next: 0,
        }
    }

    async fn connected(hub: &Arc<AcpHub>) -> Self {
        let mut ctl = Self::open(hub);
        ctl.init(true, true).await;
        ctl
    }

    async fn init(&mut self, form: bool, url: bool) -> Value {
        let mut elicitation = json!({});
        if form {
            elicitation["form"] = json!({});
        }
        if url {
            elicitation["url"] = json!({});
        }
        self.call(
            "initialize",
            json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": elicitation}}),
        )
        .await
        .expect("initialize")
    }

    fn send(&mut self, method: &str, params: Value) -> RequestId {
        self.next += 1;
        let id = RequestId::Number(self.next);
        self.conn.handle(RpcMessage::Request {
            id: id.clone(),
            method: method.into(),
            params,
        });
        id
    }

    async fn recv(&mut self) -> RpcMessage {
        tokio::time::timeout(WAIT, self.rx.recv())
            .await
            .expect("message in time")
            .expect("connection open")
    }

    async fn response(&mut self, id: &RequestId) -> Result<Value, RpcError> {
        if let Some(pos) = self
            .inbox
            .iter()
            .position(|m| matches!(m, RpcMessage::Response { id: got, .. } if got == id))
        {
            if let Some(RpcMessage::Response {
                result, ..
            }) = self.inbox.remove(pos)
            {
                return result;
            }
        }
        loop {
            match self.recv().await {
                RpcMessage::Response {
                    id: got,
                    result,
                } if &got == id => return result,
                other => self.inbox.push_back(other),
            }
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.send(method, params);
        self.response(&id).await
    }

    /// Next message matching `pred`, keeping the others.
    async fn take(&mut self, pred: impl Fn(&RpcMessage) -> bool) -> RpcMessage {
        if let Some(pos) = self.inbox.iter().position(&pred) {
            if let Some(message) = self.inbox.remove(pos) {
                return message;
            }
        }
        loop {
            let message = self.recv().await;
            if pred(&message) {
                return message;
            }
            self.inbox.push_back(message);
        }
    }

    async fn notification(&mut self, method: &str) -> Value {
        match self
            .take(|m| matches!(m, RpcMessage::Notification { method: got, .. } if got == method))
            .await
        {
            RpcMessage::Notification {
                params, ..
            } => params,
            _ => unreachable!(),
        }
    }

    async fn request(&mut self, method: &str) -> (RequestId, Value) {
        match self
            .take(|m| matches!(m, RpcMessage::Request { method: got, .. } if got == method))
            .await
        {
            RpcMessage::Request {
                id,
                params,
                ..
            } => (id, params),
            _ => unreachable!(),
        }
    }

    fn answer(&self, id: RequestId, result: Value) {
        self.conn.handle(RpcMessage::Response {
            id,
            result: Ok(result),
        });
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn.handle(RpcMessage::Notification {
            method: method.into(),
            params,
        });
    }

    /// Drain what has already arrived.
    fn drain(&mut self) -> Vec<RpcMessage> {
        let mut out: Vec<RpcMessage> = self.inbox.drain(..).collect();
        while let Ok(message) = self.rx.try_recv() {
            out.push(message);
        }
        out
    }

    async fn new_session(&mut self, cwd: &str) -> String {
        let result = self
            .call("session/new", json!({"cwd": cwd}))
            .await
            .expect("session/new");
        result["sessionId"]
            .as_str()
            .expect("session id")
            .to_string()
    }

    async fn submit(&mut self, session: &str, text: &str, key: &str) -> Value {
        self.call(
            "_pcx/session/submit",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": text}], "clientSubmissionId": key}),
        )
        .await
        .expect("submit")
    }
}

async fn wait_process(hub: &AcpHub, pred: impl Fn(&ProcessState) -> bool) {
    tokio::time::timeout(Duration::from_secs(600), async {
        while !pred(&hub.info().process) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process state in time");
}

fn error_message(result: Result<Value, RpcError>) -> RpcError {
    result.expect_err("an error")
}

#[tokio::test]
async fn initialize_negotiates_v1_and_reports_caps() {
    let s = start(FakeScript::default()).await;
    let info = s.hub.info();
    assert!(info.caps.list && info.caps.load && info.caps.resume && info.caps.close);
    assert!(info.caps.image && info.caps.queue && info.caps.url_elicitation && !info.caps.steer);
    assert_eq!(info.agent_id, "fake");
    assert_eq!(info.process, ProcessState::Ready);
    let init = &s.fake.params_of("initialize")[0];
    assert_eq!(init["protocolVersion"], 1);
    assert_eq!(init["clientCapabilities"]["auth"]["_meta"]["gateway"], true);
    assert_eq!(init["clientCapabilities"]["_meta"]["terminal-auth"], true);
    assert_eq!(init["clientCapabilities"]["fs"]["readTextFile"], false);
    let mut ctl = Ctl::open(&s.hub);
    let result = ctl.init(true, true).await;
    assert_eq!(result["protocolVersion"], 1);
    assert_eq!(result["agentInfo"]["name"], "pocket-codex-acp-hub");
    let pcx = &result["_meta"]["pcx"];
    assert_eq!(pcx["version"], 1);
    assert_eq!(pcx["agent"]["id"], "fake");
    assert_eq!(pcx["agent"]["pinned"], true);
    assert_eq!(pcx["caps"]["list"], true);
    assert_eq!(pcx["caps"]["steer"], false);
    assert_eq!(pcx["process"]["state"], "ready");
    assert_eq!(result["agentCapabilities"]["promptCapabilities"]["image"], true);
}

#[tokio::test]
async fn rejects_protocol_other_than_v1() {
    let mut script = FakeScript::default();
    script.initialize["protocolVersion"] = json!(2);
    let launch: Launch = Arc::new(Mutex::new((spec(), None)));
    let (options, _fake, _dir) = options(script, launch, None);
    let error = AcpHub::start(options).await.err().expect("start fails");
    assert_eq!(error, AcpError::ProtocolUnsupported(2));
    assert_eq!(error.code(), "acp.protocol_unsupported");
}

#[tokio::test]
async fn list_merges_pages_and_sorts_by_updated_at() {
    let script = FakeScript {
        page_size: 2,
        sessions: vec![
            listed("a", "/w", Some("2026-01-01T00:00:00Z")),
            listed("b", "/w", None),
            listed("c", "/w", Some("2026-03-01T00:00:00Z")),
            listed("d", "/x", Some("2026-02-01T00:00:00Z")),
            listed("e", "/w", Some("2025-12-01T00:00:00Z")),
        ],
        ..FakeScript::default()
    };
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let created = ctl.new_session("/w").await;
    let list = ctl.call("session/list", json!({})).await.expect("list");
    let ids: Vec<&str> = list["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter_map(|s| s["sessionId"].as_str())
        .collect();
    assert_eq!(ids, vec![created.as_str(), "c", "d", "a", "e", "b"]);
    assert!(s.fake.count("session/list") >= 4, "three agent pages plus auth detection");
    assert_eq!(
        list["sessions"][0]["_meta"]["pcx"],
        json!({"running": false, "pending": 0, "queue": 0})
    );
    let only_x = ctl
        .call("session/list", json!({"cwd": "/x"}))
        .await
        .expect("list");
    assert_eq!(only_x["sessions"].as_array().map(Vec::len), Some(1));
}

fn replay_script(session: &str) -> FakeScript {
    FakeScript {
        sessions: vec![listed(session, "/w", Some("2026-01-01T00:00:00Z"))],
        replays: [(session.to_string(), vec![user_chunk("hi"), agent_chunk("hello")])].into(),
        ..FakeScript::default()
    }
}

#[tokio::test]
async fn attach_loads_once_for_concurrent_callers() {
    let mut script = replay_script("s1");
    script.load_delay_ms = 200;
    let s = start(script).await;
    let mut a = Ctl::connected(&s.hub).await;
    let mut b = Ctl::connected(&s.hub).await;
    let ia = a.send("_pcx/session/attach", json!({"sessionId": "s1", "cwd": "/w"}));
    let ib = b.send("_pcx/session/attach", json!({"sessionId": "s1", "cwd": "/w"}));
    let ra = a.response(&ia).await.expect("attach a");
    let rb = b.response(&ib).await.expect("attach b");
    assert_eq!(s.fake.count("session/load"), 1);
    for r in [&ra, &rb] {
        assert_eq!(r["loading"], false);
        assert_eq!(r["items"].as_array().map(Vec::len), Some(2));
        assert_eq!(r["items"][1]["content"][0]["text"], "hello");
    }
    assert_eq!(ra["generation"], rb["generation"]);
}

#[tokio::test(start_paused = true)]
async fn attach_prefers_resume_when_transcript_is_current() {
    let s = start(replay_script("s1")).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    // Learn updatedAt from a listing, then materialize via load.
    ctl.call("session/list", json!({})).await.expect("list");
    let first = ctl
        .call("_pcx/session/attach", json!({"sessionId": "s1"}))
        .await
        .expect("attach");
    assert_eq!(s.fake.count("session/load"), 1);
    s.fake.crash();
    s.fake.wait_connects(2).await;
    wait_process(&s.hub, |p| *p == ProcessState::Ready).await;
    let again = ctl
        .call("_pcx/session/attach", json!({"sessionId": "s1"}))
        .await
        .expect("attach");
    assert_eq!(s.fake.count("session/load"), 1, "no second load");
    assert_eq!(s.fake.count("session/resume"), 1);
    assert_eq!(again["generation"], first["generation"]);
    assert_eq!(again["items"], first["items"]);
}

#[tokio::test(start_paused = true)]
async fn attach_returns_loading_after_20s_then_notifies_loaded() {
    let mut script = replay_script("s1");
    script.load_delay_ms = 30_000;
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let result = ctl
        .call("_pcx/session/attach", json!({"sessionId": "s1", "cwd": "/w"}))
        .await
        .expect("attach");
    assert_eq!(result["loading"], true);
    assert_eq!(result["items"], json!([]));
    let window = ctl
        .call("_pcx/session/window", json!({"sessionId": "s1", "generation": ""}))
        .await;
    assert_eq!(error_message(window).code, code::SESSION_LOADING);
    let loaded = ctl.notification("_pcx/session/loaded").await;
    assert_eq!(loaded["sessionId"], "s1");
    assert!(loaded["seq"]
        .as_u64()
        .is_some_and(|seq| seq > result["seq"].as_u64().unwrap_or(0)));
    let again = ctl
        .call("_pcx/session/attach", json!({"sessionId": "s1"}))
        .await
        .expect("attach");
    assert_eq!(again["loading"], false);
    assert_eq!(again["items"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn submit_to_unloaded_session_loads_then_prompts() {
    let s = start(replay_script("s1")).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    ctl.call("session/list", json!({})).await.expect("list");
    let submitted = ctl.submit("s1", "go", "k1").await;
    assert_eq!(submitted["queued"], true);
    assert_eq!(submitted["position"], 1);
    s.fake.wait_for("session/prompt", 1).await;
    let methods = s.fake.methods();
    let load = methods
        .iter()
        .position(|m| m == "session/load")
        .expect("load");
    let prompt = methods
        .iter()
        .position(|m| m == "session/prompt")
        .expect("prompt");
    assert!(load < prompt);
}

#[tokio::test]
async fn submit_while_running_queues_and_drains() {
    let mut script = FakeScript::default();
    script
        .prompts
        .push_back(vec![Step::Sleep(300), Step::Finish("end_turn".into())]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    let first = ctl.submit(&session, "one", "k1").await;
    assert_eq!(first["queued"], false);
    assert_eq!(first["turn"], 1);
    let second = ctl.submit(&session, "two", "k2").await;
    assert_eq!(second["queued"], true);
    assert_eq!(second["position"], 1);
    let done1 = ctl.notification("_pcx/turn/completed").await;
    assert_eq!(done1["turn"], 1);
    let started2 = ctl.notification("_pcx/turn/started").await;
    let started2 =
        if started2["turn"] == 1 { ctl.notification("_pcx/turn/started").await } else { started2 };
    assert_eq!(started2["turn"], 2);
    assert_eq!(started2["submissionId"], second["submissionId"]);
    let done2 = ctl.notification("_pcx/turn/completed").await;
    assert_eq!(done2["turn"], 2);
    assert_eq!(done2["stopReason"], "end_turn");
    assert_eq!(s.fake.count("session/prompt"), 2);
}

#[tokio::test]
async fn duplicate_client_submission_is_idempotent() {
    let s = start(FakeScript::default()).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    let a = ctl.submit(&session, "hi", "same").await;
    let b = ctl.submit(&session, "hi", "same").await;
    assert_eq!(a, b);
    ctl.notification("_pcx/turn/completed").await;
    assert_eq!(s.fake.count("session/prompt"), 1);
}

#[tokio::test]
async fn error_responses_carry_pcx_code_prefix() {
    let s = start(replay_script("s1")).await;
    let mut early = Ctl::open(&s.hub);
    let refused = early.call("session/list", json!({})).await;
    assert_eq!(error_message(refused).code, code::INVALID_REQUEST);
    let mut ctl = Ctl::connected(&s.hub).await;
    ctl.call("_pcx/session/attach", json!({"sessionId": "s1", "cwd": "/w"}))
        .await
        .expect("attach");
    let stale = error_message(
        ctl.call("_pcx/session/window", json!({"sessionId": "s1", "generation": "nope"}))
            .await,
    );
    assert_eq!(stale.code, code::GENERATION_CHANGED);
    assert!(stale.message.starts_with("[acp.generation_changed] "));
    assert_eq!(stale.data, Some(json!({"pcxCode": "acp.generation_changed"})));
    let unknown = error_message(
        ctl.call("_pcx/session/attach", json!({"sessionId": "zzz"}))
            .await,
    );
    assert!(unknown.message.starts_with("[acp.session_not_loadable] "), "{}", unknown.message);
    let missing = error_message(ctl.call("session/new", json!({"cwd": ""})).await);
    assert!(missing.message.starts_with("[acp.cwd_required] "));
    let other = error_message(ctl.call("session/delete", json!({})).await);
    assert_eq!(other.code, code::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn update_meta_carries_item_for_tool_and_plan() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Update(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "ls", "kind": "execute", "status": "pending"})),
        Step::Update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"})),
        Step::Update(json!({"sessionUpdate": "plan", "entries": [{"content": "step", "priority": "high", "status": "pending"}]})),
        Step::Update(agent_chunk("done")),
        Step::Finish("end_turn".into()),
    ]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k").await;
    ctl.notification("_pcx/turn/completed").await;
    let updates: Vec<Value> = ctl
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            RpcMessage::Notification {
                method,
                params,
            } if method == "session/update" => Some(params),
            _ => None,
        })
        .collect();
    let by_tag = |tag: &str| -> Vec<&Value> {
        updates
            .iter()
            .filter(|u| u["update"]["sessionUpdate"] == tag)
            .collect()
    };
    let tool = by_tag("tool_call")[0];
    assert_eq!(tool["_meta"]["pcx"]["itemId"], "tc:t1");
    assert_eq!(tool["_meta"]["pcx"]["created"], true);
    assert_eq!(tool["_meta"]["pcx"]["item"]["tool"]["title"], "ls");
    let patch = by_tag("tool_call_update")[0];
    assert_eq!(patch["_meta"]["pcx"]["item"]["tool"]["status"], "completed");
    assert_eq!(patch["_meta"]["pcx"]["item"]["tool"]["title"], "ls");
    let plan = by_tag("plan")[0];
    assert_eq!(plan["_meta"]["pcx"]["itemId"], "p:1");
    assert_eq!(plan["_meta"]["pcx"]["item"]["plan"][0]["content"], "step");
    let chunk = by_tag("agent_message_chunk")[0];
    assert!(chunk["_meta"]["pcx"]["itemId"].is_string());
    assert!(chunk["_meta"]["pcx"].get("item").is_none());
    let seqs: Vec<u64> = updates
        .iter()
        .filter_map(|u| u["_meta"]["pcx"]["seq"].as_u64())
        .collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
}

#[tokio::test]
async fn authenticate_returns_in_progress_then_broadcasts_state() {
    let script = FakeScript {
        requires_auth: true,
        ..FakeScript::default()
    };
    let s = start(script).await;
    assert_eq!(s.hub.info().auth.status, "required");
    let mut ctl = Ctl::connected(&s.hub).await;
    let state = ctl
        .call("_pcx/auth/authenticate", json!({"methodId": "login"}))
        .await
        .expect("auth");
    assert_eq!(state["status"], "inProgress");
    loop {
        let hub = ctl.notification("_pcx/hub/state").await;
        if hub["auth"]["status"] == "ok" {
            break;
        }
    }
    assert_eq!(s.fake.count("authenticate"), 1);
    assert_eq!(s.hub.info().auth.status, "ok");
}

#[tokio::test]
async fn hub_level_elicitation_is_sent_to_every_connection() {
    let s = start(FakeScript::default()).await;
    let mut a = Ctl::connected(&s.hub).await;
    let mut b = Ctl::connected(&s.hub).await;
    let fake = s.fake.clone();
    let asked = tokio::spawn(async move {
        fake.request(
            "elicitation/create",
            json!({"message": "Sign in", "mode": "url", "requestId": 9, "elicitationId": "e1", "url": "https://example.com/device"}),
        )
        .await
    });
    let (ida, pa) = a.request("elicitation/create").await;
    let (idb, _) = b.request("elicitation/create").await;
    let mut late = Ctl::connected(&s.hub).await;
    let (_, pl) = late.request("elicitation/create").await;
    assert_eq!(pa["_meta"]["pcx"]["requestId"], pl["_meta"]["pcx"]["requestId"]);
    a.answer(ida, json!({"action": "accept"}));
    let answer = asked.await.expect("join").expect("answered");
    assert_eq!(answer["action"], "accept");
    let cancel = b.notification("$/cancel_request").await;
    assert_eq!(cancel["requestId"], json!(idb));
    for ctl in [&mut a, &mut b, &mut late] {
        let resolved = ctl.notification("_pcx/request/resolved").await;
        assert_eq!(resolved["requestId"], pa["_meta"]["pcx"]["requestId"]);
        assert!(resolved.get("sessionId").is_none());
    }
}

#[tokio::test]
async fn cancel_answers_pending_permission_with_cancelled() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![Step::RequestPermission {
        options: permission_options(),
    }]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k1").await;
    ctl.request("session/request_permission").await;
    ctl.submit(&session, "queued", "k2").await;
    ctl.notify("session/cancel", json!({"sessionId": session}));
    let failed = ctl.notification("_pcx/queue/failed").await;
    assert_eq!(failed["reason"], "cancelled");
    assert_eq!(failed["prompts"][0]["text"], "queued");
    ctl.notification("_pcx/request/resolved").await;
    let done = ctl.notification("_pcx/turn/completed").await;
    assert_eq!(done["stopReason"], "cancelled");
    s.fake.wait_answers(1).await;
    let (_, answer) = &s.fake.answers()[0];
    assert_eq!(answer.as_ref().expect("answer"), &json!({"outcome": {"outcome": "cancelled"}}));
    assert_eq!(s.fake.count("session/cancel"), 1);
}

#[tokio::test]
async fn permission_first_valid_answer_wins_and_others_get_cancel_request() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![Step::RequestPermission {
        options: permission_options(),
    }]);
    let s = start(script).await;
    let mut a = Ctl::connected(&s.hub).await;
    let session = a.new_session("/w").await;
    let mut b = Ctl::connected(&s.hub).await;
    b.call("_pcx/session/attach", json!({"sessionId": session}))
        .await
        .expect("attach");
    a.submit(&session, "go", "k").await;
    let (ida, pa) = a.request("session/request_permission").await;
    let (idb, pb) = b.request("session/request_permission").await;
    assert_eq!(pa["_meta"]["pcx"]["requestId"], pb["_meta"]["pcx"]["requestId"]);
    a.answer(ida, json!({"outcome": {"outcome": "selected", "optionId": "allow"}}));
    let cancel = b.notification("$/cancel_request").await;
    assert_eq!(cancel["requestId"], json!(idb));
    b.answer(idb, json!({"outcome": {"outcome": "selected", "optionId": "deny"}}));
    for ctl in [&mut a, &mut b] {
        let resolved = ctl.notification("_pcx/request/resolved").await;
        assert_eq!(resolved["sessionId"], json!(session));
        assert!(resolved["seq"].as_u64().is_some());
    }
    a.notification("_pcx/turn/completed").await;
    let answers = s.fake.answers();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1.as_ref().expect("answer")["outcome"]["optionId"], "allow");
}

#[tokio::test]
async fn invalid_option_is_resent_with_new_id_and_pending_kept() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![Step::RequestPermission {
        options: permission_options(),
    }]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k").await;
    let (first, _) = ctl.request("session/request_permission").await;
    ctl.answer(first.clone(), json!({"outcome": {"outcome": "selected", "optionId": "nope"}}));
    let (second, params) = ctl.request("session/request_permission").await;
    assert_ne!(first, second);
    assert!(params["_meta"]["pcx"]["rejected"]
        .as_str()
        .is_some_and(|r| r.contains("nope")));
    assert!(s.fake.answers().is_empty());
    ctl.answer(second, json!({"outcome": {"outcome": "selected", "optionId": "deny"}}));
    s.fake.wait_answers(1).await;
    assert_eq!(s.fake.answers()[0].1.as_ref().expect("answer")["outcome"]["optionId"], "deny");
}

#[tokio::test]
async fn attach_snapshot_seq_orders_against_live_notifications() {
    let mut script = FakeScript::default();
    let mut steps = Vec::new();
    for i in 0..200 {
        steps.push(Step::Update(json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1",
            "content": {"type": "text", "text": format!("{i},")}})));
        if i % 20 == 0 {
            steps.push(Step::Sleep(2));
        }
    }
    steps.push(Step::Finish("end_turn".into()));
    script.prompts.push_back(steps);
    let s = start(script).await;
    let mut a = Ctl::connected(&s.hub).await;
    let session = a.new_session("/w").await;
    a.submit(&session, "go", "k").await;
    tokio::time::sleep(Duration::from_millis(15)).await;
    let mut b = Ctl::connected(&s.hub).await;
    let snapshot = b
        .call("_pcx/session/attach", json!({"sessionId": session}))
        .await
        .expect("attach");
    let base_seq = snapshot["seq"].as_u64().expect("seq");
    let mut text = snapshot["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == "m:agent:m1"))
        .and_then(|i| i["content"][0]["text"].as_str())
        .unwrap_or("")
        .to_string();
    loop {
        let message = b.recv().await;
        let RpcMessage::Notification {
            method,
            params,
        } = message
        else {
            continue;
        };
        if let Some(seq) = params["seq"]
            .as_u64()
            .or(params["_meta"]["pcx"]["seq"].as_u64())
        {
            assert!(seq > base_seq, "{method} seq {seq} <= snapshot {base_seq}");
        }
        if method == "session/update" && params["update"]["sessionUpdate"] == "agent_message_chunk"
        {
            text.push_str(params["update"]["content"]["text"].as_str().unwrap_or(""));
        }
        if method == "_pcx/turn/completed" {
            break;
        }
    }
    let expected: String = (0..200).map(|i| format!("{i},")).collect();
    assert_eq!(text, expected);
}

#[tokio::test]
async fn pending_is_resent_to_late_subscriber() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![Step::RequestPermission {
        options: permission_options(),
    }]);
    let s = start(script).await;
    let mut a = Ctl::connected(&s.hub).await;
    let session = a.new_session("/w").await;
    a.call("_pcx/session/detach", json!({"sessionId": session}))
        .await
        .expect("detach");
    a.submit(&session, "go", "k").await;
    s.fake.wait_for("session/prompt", 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let running = a
        .call("_pcx/sessions/running", json!({}))
        .await
        .expect("running");
    assert_eq!(running["sessions"][0]["pending"], 1);
    let mut b = Ctl::connected(&s.hub).await;
    let attach_id = b.send("_pcx/session/attach", json!({"sessionId": session}));
    // The attach response comes before the re-sent request.
    let first = b.recv().await;
    assert!(matches!(first, RpcMessage::Response { ref id, .. } if *id == attach_id), "{first:?}");
    let (id, _) = b.request("session/request_permission").await;
    b.answer(id, json!({"outcome": {"outcome": "selected", "optionId": "allow"}}));
    s.fake.wait_answers(1).await;
}

#[tokio::test]
async fn form_and_url_elicitation_route_by_client_mode() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Elicit(json!({"message": "Name?", "mode": "form", "sessionId": "fake-1",
            "requestedSchema": {"type": "object", "properties": {"name": {"type": "string"}}}})),
        Step::Elicit(json!({"message": "Open", "mode": "url", "sessionId": "fake-1",
            "elicitationId": "e2", "url": "https://example.com"})),
        Step::Finish("end_turn".into()),
    ]);
    let s = start(script).await;
    let mut form = Ctl::open(&s.hub);
    form.init(true, false).await;
    let mut url = Ctl::open(&s.hub);
    url.init(false, true).await;
    let session = form.new_session("/w").await;
    assert_eq!(session, "fake-1");
    url.call("_pcx/session/attach", json!({"sessionId": session}))
        .await
        .expect("attach");
    form.submit(&session, "go", "k").await;
    let (id, params) = form.request("elicitation/create").await;
    assert_eq!(params["mode"], "form");
    form.answer(id, json!({"action": "accept", "content": {"name": "x"}}));
    let (id, params) = url.request("elicitation/create").await;
    assert_eq!(params["mode"], "url");
    url.answer(id, json!({"action": "accept"}));
    form.notification("_pcx/turn/completed").await;
    assert!(
        !form
            .drain()
            .iter()
            .any(|m| matches!(m, RpcMessage::Request { params, .. } if params["mode"] == "url")),
        "the form-only client never sees URL elicitations"
    );
    assert!(
        !url.drain()
            .iter()
            .any(|m| matches!(m, RpcMessage::Request { params, .. } if params["mode"] == "form")),
        "the URL-only client never sees form elicitations"
    );
    let answers = s.fake.answers();
    assert_eq!(answers[0].1.as_ref().expect("form")["content"]["name"], "x");
}

#[tokio::test]
async fn elicitation_complete_resolves_url_pending() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Elicit(json!({"message": "Open", "mode": "url", "sessionId": "fake-1",
            "elicitationId": "e3", "url": "https://example.com"})),
        Step::Finish("end_turn".into()),
    ]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k").await;
    let (id, _) = ctl.request("elicitation/create").await;
    s.fake
        .notify("elicitation/complete", json!({"elicitationId": "e3"}));
    let cancel = ctl.notification("$/cancel_request").await;
    assert_eq!(cancel["requestId"], json!(id));
    ctl.notification("_pcx/request/resolved").await;
    ctl.notification("_pcx/turn/completed").await;
    assert_eq!(s.fake.answers()[0].1.as_ref().expect("answer")["action"], "accept");
}

#[tokio::test(start_paused = true)]
async fn crash_fails_running_turn_clears_pending_and_restarts_with_backoff() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![Step::RequestPermission {
        options: permission_options(),
    }]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k1").await;
    let (id, _) = ctl.request("session/request_permission").await;
    ctl.submit(&session, "later", "k2").await;
    s.fake.crash();
    let done = ctl.notification("_pcx/turn/completed").await;
    assert_eq!(done["stopReason"], "_pcx_agent_exited");
    assert!(done["error"].as_str().is_some());
    let cancel = ctl.notification("$/cancel_request").await;
    assert_eq!(cancel["requestId"], json!(id));
    ctl.notification("_pcx/request/resolved").await;
    let failed = ctl.notification("_pcx/queue/failed").await;
    assert_eq!(failed["reason"], "agent_exited");
    let restarting = loop {
        let state = ctl.notification("_pcx/hub/state").await;
        if state["process"]["state"] == "restarting" {
            break state;
        }
    };
    assert_eq!(restarting["process"]["attempt"], 1);
    assert_eq!(restarting["process"]["retryInMs"], 1000);
    loop {
        let state = ctl.notification("_pcx/hub/state").await;
        if state["process"]["state"] == "ready" {
            break;
        }
    }
    assert_eq!(s.fake.connects(), 2);
    let attach = ctl
        .call("_pcx/session/attach", json!({"sessionId": session}))
        .await;
    assert!(attach.is_ok(), "the session reloads after restart: {attach:?}");
}

#[tokio::test(start_paused = true)]
async fn five_crashes_in_five_minutes_enter_failed() {
    let s = start(FakeScript::default()).await;
    for n in 1..=5u32 {
        s.fake.wait_connects(n).await;
        wait_process(&s.hub, |p| *p == ProcessState::Ready).await;
        s.fake.crash();
        if n < 5 {
            wait_process(
                &s.hub,
                |p| matches!(p, ProcessState::Restarting { attempt, .. } if *attempt == n),
            )
            .await;
        }
    }
    wait_process(&s.hub, |p| matches!(p, ProcessState::Failed { .. })).await;
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(s.fake.connects(), 5, "no restart after Failed");
}

#[tokio::test]
async fn auth_required_detected_from_list_and_new() {
    let mut script = FakeScript {
        requires_auth: true,
        ..FakeScript::default()
    };
    let s = start(script.clone()).await;
    assert_eq!(s.hub.info().auth.status, "required");
    let methods = s.hub.info().auth.methods;
    assert_eq!(methods[0].kind, "agent");
    assert!(methods[0].remote);

    script.initialize["agentCapabilities"]["sessionCapabilities"] = json!({});
    let s = start(script).await;
    assert_eq!(s.hub.info().auth.status, "unknown");
    let mut ctl = Ctl::connected(&s.hub).await;
    let error = error_message(ctl.call("session/new", json!({"cwd": "/w"})).await);
    assert_eq!(error.code, code::AUTH_REQUIRED);
    assert!(error.message.starts_with("[acp.auth_required] "));
    let state = ctl.notification("_pcx/hub/state").await;
    assert_eq!(state["auth"]["status"], "required");
}

#[derive(Default)]
struct RecordingTerminal(Mutex<Vec<TerminalLaunch>>);

impl TerminalLauncher for RecordingTerminal {
    fn open(&self, launch: &TerminalLaunch, _status: &std::path::Path) -> Result<(), AcpError> {
        self.0.lock().expect("terminal").push(launch.clone());
        Ok(())
    }
}

#[tokio::test]
async fn legacy_terminal_auth_requires_matching_command() {
    let mut script = FakeScript::default();
    script.initialize["authMethods"] = json!([
        {"id": "term-ok", "name": "Login", "description": "terminal",
         "_meta": {"terminal-auth": {"command": "fake-agent", "args": ["--login", "--hide-claude-auth"], "label": "Fake login"}}},
        {"id": "term-bad", "name": "Other", "description": "run `other login`",
         "_meta": {"terminal-auth": {"command": "other", "args": []}}},
        {"id": "spec", "name": "Spec", "type": "terminal", "args": ["auth", "login"], "env": {"X": "1"}}
    ]);
    let terminal = Arc::new(RecordingTerminal::default());
    let s = start_with(script, None, Some(terminal.clone())).await;
    let methods = s.hub.info().auth.methods;
    let find = |id: &str| methods.iter().find(|m| m.id == id).expect("method").clone();
    assert_eq!(find("term-ok").kind, "terminal");
    assert!(find("term-ok").available && !find("term-ok").remote);
    assert!(!find("term-bad").available);
    assert_eq!(find("spec").kind, "terminal");
    s.hub.terminal_login("term-ok").expect("legacy login");
    s.hub.terminal_login("spec").expect("spec login");
    assert!(s.hub.terminal_login("term-bad").is_err());
    let launches = terminal.0.lock().expect("launches").clone();
    assert_eq!(launches[0].program, PathBuf::from("/opt/fake/fake-agent"));
    assert_eq!(launches[0].args, vec!["--login".to_string()], "launch-only args are stripped");
    assert_eq!(launches[0].title, "Fake login");
    assert_eq!(launches[1].args, vec!["dist/index.js".to_string(), "auth".into(), "login".into()]);
    assert_eq!(launches[1].env.get("X").map(String::as_str), Some("1"));
    let mut ctl = Ctl::connected(&s.hub).await;
    let remote = error_message(
        ctl.call("_pcx/auth/authenticate", json!({"methodId": "spec"}))
            .await,
    );
    assert!(remote.message.starts_with("[acp.host_only] "));
}

fn gateway_script() -> FakeScript {
    let mut script = FakeScript::default();
    script.initialize["authMethods"] = json!([
        {"id": "login", "name": "Log in"},
        {"id": "gateway", "name": "Custom model gateway", "_meta": {"gateway": {"protocol": "anthropic"}}}
    ]);
    script
}

fn gateway(base: &str, token: &str) -> GatewayAuth {
    GatewayAuth {
        method_id: None,
        base_url: base.into(),
        headers: BTreeMap::from([("Authorization".into(), format!("Bearer {token}"))]),
        provider_name: None,
    }
}

#[tokio::test]
async fn gateway_authenticate_runs_before_ready() {
    let s =
        start_with(gateway_script(), Some(gateway("https://relay.example.com", "sk-test")), None)
            .await;
    let methods = s.fake.methods();
    assert_eq!(&methods[..2], &["initialize".to_string(), "authenticate".to_string()]);
    let auth = &s.fake.params_of("authenticate")[0];
    assert_eq!(auth["methodId"], "gateway");
    assert_eq!(auth["_meta"]["gateway"]["baseUrl"], "https://relay.example.com");
    assert_eq!(auth["_meta"]["gateway"]["headers"]["Authorization"], "Bearer sk-test");
    let info = s.hub.info();
    assert_eq!(info.auth.status, "ok");
    let gw = info
        .auth
        .methods
        .iter()
        .find(|m| m.id == "gateway")
        .expect("gateway method");
    assert_eq!(gw.kind, "gateway");
    assert!(gw.gateway_configured && !gw.remote);
    assert_eq!(gw.gateway_protocol.as_deref(), Some("anthropic"));
    s.hub.restart().await.expect("restart");
    let methods = s.fake.methods();
    let second_init = methods
        .iter()
        .rposition(|m| m == "initialize")
        .expect("init");
    assert_eq!(methods[second_init + 1], "authenticate");
    assert_eq!(s.fake.count("authenticate"), 2);
}

#[tokio::test]
async fn restart_rereads_launch_provider() {
    let s =
        start_with(gateway_script(), Some(gateway("https://one.example.com", "sk-1")), None).await;
    {
        let mut launch = s.launch.lock().expect("launch");
        launch.0.version = Some("2.0.0".into());
        launch.1 = Some(gateway("https://two.example.com", "sk-2"));
    }
    s.hub.restart().await.expect("restart");
    assert_eq!(s.hub.info().agent_version.as_deref(), Some("2.0.0"));
    let auth = s.fake.params_of("authenticate");
    assert_eq!(auth[1]["_meta"]["gateway"]["baseUrl"], "https://two.example.com");
    assert_eq!(s.fake.connects(), 2);
}

#[tokio::test]
async fn gateway_failure_reports_required_without_leaking_token() {
    let mut script = gateway_script();
    script.authenticate_error = Some("invalid key sk-secret-token".into());
    let s = start_with(script, Some(gateway("https://relay.example.com", "sk-secret-token")), None)
        .await;
    let auth = s.hub.info().auth;
    assert_eq!(auth.status, "required");
    let message = auth.message.expect("message");
    assert!(message.contains("网关登录失败"), "{message}");
    assert!(!message.contains("sk-secret-token"), "{message}");
}

#[tokio::test]
async fn fs_read_write_confined_to_session_cwd_and_rejects_symlink_escape() {
    let work = tempfile::tempdir().expect("work");
    let outside = tempfile::tempdir().expect("outside");
    let cwd = work.path().canonicalize().expect("cwd");
    std::fs::write(cwd.join("in.txt"), "inside").expect("write");
    std::fs::write(outside.path().join("secret.txt"), "secret").expect("write");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), cwd.join("link.txt"))
        .expect("link");
    let p = |name: &str| cwd.join(name).to_string_lossy().into_owned();
    let mut script = FakeScript::default();
    let call = |method: &str, params: Value| Step::Call {
        method: method.into(),
        params,
    };
    script.prompts.push_back(vec![
        call("fs/read_text_file", json!({"sessionId": "fake-1", "path": p("in.txt")})),
        call(
            "fs/read_text_file",
            json!({"sessionId": "fake-1", "path": outside.path().join("secret.txt")}),
        ),
        call("fs/read_text_file", json!({"sessionId": "fake-1", "path": p("link.txt")})),
        call(
            "fs/write_text_file",
            json!({"sessionId": "fake-1", "path": p("new.txt"), "content": "made"}),
        ),
        call(
            "fs/write_text_file",
            json!({"sessionId": "fake-1", "path": p("link.txt"), "content": "x"}),
        ),
        call("fs/read_text_file", json!({"sessionId": "fake-1", "path": "relative.txt"})),
        Step::Finish("end_turn".into()),
    ]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session(&cwd.to_string_lossy()).await;
    ctl.submit(&session, "go", "k").await;
    ctl.notification("_pcx/turn/completed").await;
    let answers: Vec<Result<Value, RpcError>> =
        s.fake.answers().into_iter().map(|(_, r)| r).collect();
    assert_eq!(answers[0].as_ref().expect("read inside")["content"], "inside");
    let outside_error = answers[1].as_ref().expect_err("outside");
    assert_eq!(outside_error.code, code::INVALID_PARAMS);
    assert!(outside_error
        .message
        .contains("outside the session directory"));
    if cfg!(unix) {
        assert!(answers[2].is_err(), "symlink escape on read");
        assert!(answers[4].is_err(), "write through a symlink");
        assert_eq!(
            std::fs::read_to_string(outside.path().join("secret.txt")).expect("secret"),
            "secret"
        );
    }
    assert!(answers[3].is_ok());
    assert_eq!(std::fs::read_to_string(cwd.join("new.txt")).expect("new"), "made");
    assert!(answers[5].is_err(), "relative paths are refused");
}

#[tokio::test]
async fn oversized_line_becomes_notice_item() {
    let mut big = vec![b'x'; pocket_codex_host_svc::acp::MAX_LINE_BYTES + 10];
    big.push(b'\n');
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Raw(big),
        Step::Update(agent_chunk("after")),
        Step::Finish("end_turn".into()),
    ]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.submit(&session, "go", "k").await;
    ctl.notification("_pcx/turn/completed").await;
    let updates: Vec<Value> = ctl
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            RpcMessage::Notification {
                method,
                params,
            } if method == "session/update" => Some(params),
            _ => None,
        })
        .collect();
    let notice = updates
        .iter()
        .find(|u| u["update"]["sessionUpdate"] == "_pcx_notice")
        .expect("notice update");
    assert_eq!(notice["_meta"]["pcx"]["item"]["kind"], "notice");
    assert!(notice["_meta"]["pcx"]["itemId"]
        .as_str()
        .is_some_and(|id| id.starts_with("n:1:")));
    assert!(updates
        .iter()
        .any(|u| u["update"]["content"]["text"] == "after"));
}

#[tokio::test(start_paused = true)]
async fn idle_session_is_closed_after_ten_minutes() {
    let s = start(FakeScript::default()).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let session = ctl.new_session("/w").await;
    ctl.call("_pcx/session/detach", json!({"sessionId": session}))
        .await
        .expect("detach");
    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
    assert_eq!(s.fake.count("session/close"), 0);
    tokio::time::sleep(Duration::from_secs(6 * 60 + 5)).await;
    s.fake.wait_for("session/close", 1).await;
    assert_eq!(s.fake.params_of("session/close")[0]["sessionId"], json!(session));
    // Reopening resumes: the transcript is still current.
    ctl.call("_pcx/session/attach", json!({"sessionId": session}))
        .await
        .expect("attach");
    assert_eq!(s.fake.count("session/resume"), 1);
}

#[tokio::test]
async fn divergent_reload_broadcasts_generation() {
    let mut script = replay_script("s1");
    script.replays.insert("s1".into(), vec![
        user_chunk("hi"),
        agent_chunk("hello"),
        user_chunk("more"),
    ]);
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let first = ctl
        .call("_pcx/session/attach", json!({"sessionId": "s1", "cwd": "/w"}))
        .await
        .expect("attach");
    s.fake.update_script(|script| {
        // Only a change before the last item counts as a divergence.
        script.replays.insert("s1".into(), vec![
            user_chunk("hi"),
            agent_chunk("rewritten"),
            user_chunk("more"),
        ]);
    });
    let reloaded = ctl
        .call("_pcx/session/reload", json!({"sessionId": "s1"}))
        .await
        .expect("reload");
    let generation = ctl.notification("_pcx/session/generation").await;
    assert_ne!(generation["generation"], first["generation"]);
    assert_eq!(reloaded["generation"], generation["generation"]);
    assert_eq!(reloaded["items"][1]["content"][0]["text"], "rewritten");
    assert_eq!(s.fake.count("session/load"), 2);
    // An identical reload keeps the generation.
    let same = ctl
        .call("_pcx/session/reload", json!({"sessionId": "s1"}))
        .await
        .expect("reload");
    assert_eq!(same["generation"], reloaded["generation"]);
}

fn python3() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("python3"))
        .find(|p| p.is_file())
}

#[cfg(unix)]
#[tokio::test]
async fn process_connector_spawns_and_terminates_tree() {
    use pocket_codex_host_svc::acp::ProcessConnector;
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("child.pid");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/acp/fake_agent.py");
    let spec = LaunchSpec {
        agent_id: "py".into(),
        display_name: "Python fake".into(),
        program: python,
        args: vec![
            script.to_string_lossy().into_owned(),
            "--spawn-child".into(),
            "--pid-file".into(),
            pid_file.to_string_lossy().into_owned(),
        ],
        ..LaunchSpec::default()
    };
    let options = HubOptions {
        instance: "py".into(),
        launch: Arc::new(move || Ok((spec.clone(), None))),
        state_dir: dir.path().join("acp"),
        log_file: Some(dir.path().join("logs/acp-py.log")),
        connector: Arc::new(ProcessConnector),
        terminal: None,
    };
    let hub = AcpHub::start(options).await.expect("python agent starts");
    assert!(hub.info().pid.is_some());
    let mut ctl = Ctl::connected(&hub).await;
    let session = ctl.new_session(&dir.path().to_string_lossy()).await;
    ctl.submit(&session, "echo me", "k").await;
    let done = ctl.notification("_pcx/turn/completed").await;
    assert_eq!(done["stopReason"], "end_turn");
    let child: i32 = tokio::time::timeout(WAIT, async {
        loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse().ok())
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("child pid");
    hub.shutdown(Duration::from_millis(200)).await;
    let gone = tokio::time::timeout(WAIT, async {
        loop {
            let alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(child), None).is_ok();
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(gone.is_ok(), "the agent's child process was terminated");
    assert_eq!(hub.info().process, ProcessState::Stopped);
}

fn history_script() -> FakeScript {
    let user = |text: &str| user_chunk(text);
    let agent = |text: &str, mid: &str| json!({"sessionUpdate": "agent_message_chunk", "messageId": mid, "content": {"type": "text", "text": text}});
    FakeScript {
        sessions: vec![listed("h1", "/w", Some("2026-01-01T00:00:00Z"))],
        replays: [(
            "h1".to_string(),
            vec![
                user("q1"),
                agent("a1", "m1"),
                json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "ls", "status": "completed"}),
                user("q2"),
                agent("a2", "m2"),
                user("q3"),
                agent("a3", "m3"),
            ],
        )]
        .into(),
        ..FakeScript::default()
    }
}

#[tokio::test]
async fn history_source_windows_items_and_groups() {
    use pocket_codex_core::history_sync::WindowQuery;
    use pocket_codex_host_svc::{
        acp::{serve_meta, AcpHistorySource},
        history_sync::SessionHistorySource,
        store::{ConfigStore, HostStore},
    };
    let s = start(history_script()).await;
    let source = AcpHistorySource::new(s.hub.clone());
    assert_eq!(source.provider(), "acp/hub-v1");
    let query = |collection: &str, cursor: Option<&str>, limit: u32, projection: Option<&str>| {
        WindowQuery {
            session: "h1".into(),
            collection: collection.into(),
            group: None,
            cursor: cursor.map(str::to_string),
            limit,
            projection: projection.map(str::to_string),
        }
    };
    let page = source
        .read_window(&query("items", None, 3, None))
        .await
        .expect("items");
    assert_eq!(page.order, vec!["m:agent:m3", "s:3:0", "m:agent:m2"]);
    assert_eq!(page.metadata["nextCursor"], "b:m:agent:m2");
    assert_eq!(page.metadata["hasOlder"], true);
    assert_eq!(page.documents["m:agent:m3"]["content"][0]["text"], "a3");
    let generation = page.generation.clone();
    let page = source
        .read_window(&query("items", Some("b:m:agent:m2"), 3, Some("desc")))
        .await
        .expect("older");
    assert_eq!(page.order, vec!["s:2:0", "tc:t1", "m:agent:m1"]);
    let page = source
        .read_window(&query("items", Some("b:m:agent:m1"), 3, None))
        .await
        .expect("oldest");
    assert_eq!(page.order, vec!["s:1:0"]);
    assert!(page.metadata.get("nextCursor").is_none());
    assert_eq!(page.metadata["hasOlder"], false);
    assert_eq!(page.metadata["olderUnavailable"], false);
    let asc = source
        .read_window(&query("items", None, 4, Some("asc")))
        .await
        .expect("asc");
    assert_eq!(asc.order, vec!["s:1:0", "m:agent:m1", "tc:t1", "s:2:0"]);
    assert_eq!(asc.metadata["nextCursor"], "a:s:2:0");
    let groups = source
        .read_window(&query("groups", None, 2, None))
        .await
        .expect("groups");
    assert_eq!(groups.order, vec!["t3", "t2"]);
    assert_eq!(groups.metadata["nextCursor"], "t:2");
    assert_eq!(groups.documents["t3"]["userPreview"], "q3");
    let groups = source
        .read_window(&query("groups", Some("t:2"), 2, None))
        .await
        .expect("groups");
    assert_eq!(groups.order, vec!["t1"]);
    let mut in_turn = query("items", None, 10, None);
    in_turn.group = Some("t2".into());
    let turn = source.read_window(&in_turn).await.expect("turn items");
    assert_eq!(turn.order, vec!["m:agent:m2", "s:2:0"]);
    assert_eq!(turn.metadata["hasOlder"], true);
    let meta = source
        .read_window(&query("metadata", None, 1, None))
        .await
        .expect("metadata");
    assert!(meta.order.is_empty());
    assert_eq!(meta.metadata["turns"], 3);
    assert_eq!(meta.generation, generation);
    let bad = source
        .read_window(&query("items", Some("b:nope"), 3, None))
        .await
        .expect_err("cursor");
    let message = format!("{bad:#}");
    assert!(message.contains("invalid") && message.contains("cursor"), "{message}");
    assert!(source
        .read_window(&query("groups", None, 2, Some("asc")))
        .await
        .is_err());
    assert_eq!(s.fake.count("session/load"), 1, "history reads load once");

    // The meta service exposes the same source.
    let dir = tempfile::tempdir().expect("dir");
    let store = Arc::new(
        ConfigStore::open(dir.path().join("threads.json"))
            .await
            .expect("store"),
    );
    let host = Arc::new(
        HostStore::open(dir.path().join("host.json"))
            .await
            .expect("host"),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hub = s.hub.clone();
    tokio::spawn(async move {
        let _ = serve_meta(listener, store, host, dir.path().join("uploads"), hub).await;
    });
    let caps: Value = reqwest::get(format!("http://{addr}/history/v1/capabilities"))
        .await
        .expect("capabilities")
        .json()
        .await
        .expect("json");
    assert_eq!(caps["provider"], "acp/hub-v1");
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("healthz");
    assert!(health.status().is_success());
}

#[tokio::test]
async fn slow_consumer_is_disconnected_with_1013() {
    use futures::{SinkExt, StreamExt};
    use pocket_codex_host_svc::acp::serve_ws;
    use tokio_tungstenite::tungstenite::{protocol::frame::coding::CloseCode, Message};

    let chunk = "y".repeat(2048);
    let mut steps: Vec<Step> = (0..6000)
        .map(|_| Step::Update(agent_chunk(&chunk)))
        .collect();
    steps.push(Step::Finish("end_turn".into()));
    let mut script = FakeScript::default();
    script.prompts.push_back(steps);
    let s = start(script).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hub = s.hub.clone();
    tokio::spawn(async move {
        let _ = serve_ws(listener, hub).await;
    });
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/acp"))
        .await
        .expect("connect");
    let call = |id: i64, method: &str, params: Value| {
        Message::text(
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        )
    };
    ws.send(call(1, "initialize", json!({"protocolVersion": 1})))
        .await
        .expect("send");
    ws.send(call(2, "session/new", json!({"cwd": "/w"})))
        .await
        .expect("send");
    let mut session = None;
    while session.is_none() {
        let Some(Ok(Message::Text(text))) = ws.next().await else { panic!("closed early") };
        let value: Value = serde_json::from_str(&text).expect("json");
        if value["id"] == 2 {
            session = value["result"]["sessionId"].as_str().map(str::to_string);
        }
    }
    let session = session.expect("session");
    ws.send(call(
        3,
        "_pcx/session/submit",
        json!({"sessionId": session, "prompt": [{"type": "text", "text": "flood"}], "clientSubmissionId": "k"}),
    ))
    .await
    .expect("send");
    // Stop reading while the hub floods this connection.
    s.fake.wait_for("session/prompt", 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let close = tokio::time::timeout(WAIT, async {
        while let Some(message) = ws.next().await {
            match message {
                Ok(Message::Close(frame)) => return frame,
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    })
    .await
    .expect("close in time");
    let frame = close.expect("a close frame");
    assert_eq!(frame.code, CloseCode::from(1013));
    // The hub keeps running for other controllers.
    let mut ctl = Ctl::connected(&s.hub).await;
    ctl.call("_pcx/sessions/running", json!({}))
        .await
        .expect("hub still serves");
}

#[tokio::test]
async fn ws_rejects_non_loopback_listener() {
    let s = start(FakeScript::default()).await;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .expect("bind");
    let error = pocket_codex_host_svc::acp::serve_ws(listener, s.hub.clone())
        .await
        .expect_err("refused");
    assert!(error.to_string().contains("loopback"), "{error}");
}

fn model_options(current: &str) -> Value {
    json!({"configOptions": [{
        "id": "model", "name": "Model", "category": "model", "type": "select",
        "currentValue": current,
        "options": [{"value": "p/a", "name": "A"}, {"value": "p/b", "name": "B"}]
    }]})
}

#[tokio::test]
async fn defaults_probe_once_in_a_hidden_closed_session() {
    let script = FakeScript {
        session_setup: model_options("p/a"),
        sessions: vec![
            listed("fake-1", "/elsewhere", Some("2026-01-02T00:00:00Z")),
            listed("real", "/w", Some("2026-01-01T00:00:00Z")),
        ],
        ..FakeScript::default()
    };
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let init_meta = ctl.drain();
    assert!(init_meta.is_empty(), "{init_meta:?}");

    let found = ctl
        .call("_pcx/hub/defaults", json!({}))
        .await
        .expect("defaults");
    assert_eq!(found["configOptions"][0]["currentValue"], "p/a");
    let state = ctl.notification("_pcx/hub/state").await;
    assert_eq!(state["caps"]["configOptions"], true);
    assert_eq!(state["defaultConfigOptions"][0]["id"], "model");

    let probe = s.fake.params_of("session/new");
    assert_eq!(probe.len(), 1);
    let cwd = probe[0]["cwd"].as_str().expect("cwd");
    assert_eq!(PathBuf::from(cwd), s._dir.path().join("acp").join("probe"));
    s.fake.wait_for("session/close", 1).await;
    assert_eq!(s.fake.params_of("session/close")[0]["sessionId"], "fake-1");

    // Known now: no second probe, and the probe session is never listed.
    ctl.call("_pcx/hub/defaults", json!({}))
        .await
        .expect("defaults again");
    assert_eq!(s.fake.count("session/new"), 1);
    let listing = ctl.call("session/list", json!({})).await.expect("list");
    let ids: Vec<&str> = listing["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter_map(|s| s["sessionId"].as_str())
        .collect();
    assert_eq!(ids, ["real"]);
}

#[tokio::test]
async fn defaults_without_options_probe_once_per_version() {
    let s = start(FakeScript::default()).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    for _ in 0..2 {
        let found = ctl
            .call("_pcx/hub/defaults", json!({}))
            .await
            .expect("defaults");
        assert_eq!(found["configOptions"], json!([]));
    }
    assert_eq!(s.fake.count("session/new"), 1);
}

#[tokio::test]
async fn defaults_never_probe_an_agent_that_cannot_close() {
    let mut script = FakeScript {
        session_setup: model_options("p/a"),
        ..FakeScript::default()
    };
    script.initialize["agentCapabilities"]["sessionCapabilities"] = json!({"list": {}});
    let s = start(script).await;
    let mut ctl = Ctl::connected(&s.hub).await;
    let found = ctl
        .call("_pcx/hub/defaults", json!({}))
        .await
        .expect("defaults");
    assert_eq!(found["configOptions"], json!([]));
    assert_eq!(s.fake.count("session/new"), 0);
}

#[tokio::test]
async fn defaults_survive_a_restart_without_probing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options_in = |script: FakeScript| {
        let (connector, fake) = FakeAgent::spawn(script);
        let options = HubOptions {
            instance: "test".into(),
            launch: Arc::new(|| Ok((spec(), None))),
            state_dir: dir.path().join("acp"),
            log_file: None,
            connector: Arc::new(connector),
            terminal: None,
        };
        (options, fake)
    };
    let script = FakeScript {
        session_setup: model_options("p/b"),
        ..FakeScript::default()
    };
    let (options, _first) = options_in(script.clone());
    let hub = AcpHub::start(options).await.expect("first hub");
    let mut ctl = Ctl::connected(&hub).await;
    ctl.new_session("/w").await;
    hub.shutdown(Duration::from_millis(200)).await;

    let (options, fake) = options_in(script);
    let hub = AcpHub::start(options).await.expect("second hub");
    let mut ctl = Ctl::open(&hub);
    let init = ctl.init(true, true).await;
    let pcx = &init["_meta"]["pcx"];
    assert_eq!(pcx["caps"]["configOptions"], true);
    assert_eq!(pcx["defaultConfigOptions"][0]["currentValue"], "p/b");
    let found = ctl
        .call("_pcx/hub/defaults", json!({}))
        .await
        .expect("defaults");
    assert_eq!(found["configOptions"][0]["currentValue"], "p/b");
    assert_eq!(fake.count("session/new"), 0);
}

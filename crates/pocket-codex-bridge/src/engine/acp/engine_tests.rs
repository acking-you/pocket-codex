//! The controller engine against an in-process hub (TRD §8.2 "bridge 引擎",
//! T19): a FakeAgent behind `AcpHub`, served by the real `serve_ws` and
//! `serve_meta` on loopback, reached through `connect_url`.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use pocket_codex_core::acp::{ContentBlock, SessionInfo};
use pocket_codex_host_svc::{
    acp::{
        serve_meta, serve_ws,
        testing::{FakeAgent, FakeHandle, FakeScript, Step},
        AcpHub, HubOptions, LaunchSpec,
    },
    store::{ConfigStore, HostConfig, HostStore},
};
use reqwest::Url;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::{
    net::TcpListener,
    sync::{broadcast, watch},
};

use super::{connect_url, ctx, disconnect, is_connected, pcx_code, subscribe_events};
use crate::engine::{app_session::AppEvent, runtime};

const WAIT: Duration = Duration::from_secs(40);

/// Drops every proxied connection on `kill` while keeping the listener, so a
/// reconnect reaches the same hub.
struct Proxy {
    addr: SocketAddr,
    kill: watch::Sender<u64>,
}

impl Proxy {
    fn start(target: SocketAddr) -> Self {
        let rt = runtime::runtime();
        let listener = rt
            .block_on(TcpListener::bind("127.0.0.1:0"))
            .expect("bind proxy");
        let addr = listener.local_addr().expect("addr");
        let (kill, gen) = watch::channel(0u64);
        rt.spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                let mut gen = gen.clone();
                // Only a kill after this connection was accepted ends it.
                gen.borrow_and_update();
                tokio::spawn(async move {
                    let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {},
                        _ = gen.changed() => {},
                    }
                });
            }
        });
        Self {
            addr,
            kill,
        }
    }

    fn kill(&self) {
        self.kill.send_modify(|g| *g += 1);
    }
}

struct Env {
    key: String,
    hub: Arc<AcpHub>,
    fake: FakeHandle,
    host: Arc<HostStore>,
    proxy: Option<Proxy>,
    _dir: TempDir,
}

impl Drop for Env {
    fn drop(&mut self) {
        disconnect(&self.key);
    }
}

fn spec() -> LaunchSpec {
    LaunchSpec {
        agent_id: "fake".into(),
        display_name: "Fake Agent".into(),
        program: PathBuf::from("/opt/fake/agent"),
        version: Some("1.0.0".into()),
        pinned: true,
        ..LaunchSpec::default()
    }
}

fn start_env(name: &str, script: FakeScript, proxied: bool) -> Env {
    runtime::init(std::env::temp_dir()).expect("runtime");
    let rt = runtime::runtime();
    let dir = tempfile::tempdir().expect("dir");
    let (connector, fake) = FakeAgent::spawn(script);
    let options = HubOptions {
        instance: name.into(),
        launch: Arc::new(|| Ok((spec(), None))),
        state_dir: dir.path().join("acp"),
        log_file: None,
        connector: Arc::new(connector),
        terminal: None,
    };
    let hub = rt.block_on(AcpHub::start(options)).expect("hub");
    let ws = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind ws");
    let ws_addr = ws.local_addr().expect("ws addr");
    rt.spawn(serve_ws(ws, hub.clone()));
    let store = Arc::new(
        rt.block_on(ConfigStore::open(dir.path().join("threads.json")))
            .expect("store"),
    );
    let host = Arc::new(
        rt.block_on(HostStore::open(dir.path().join("host.json")))
            .expect("host"),
    );
    let meta = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind meta");
    let meta_addr = meta.local_addr().expect("meta addr");
    rt.spawn(serve_meta(meta, store, host.clone(), dir.path().join("uploads"), hub.clone()));
    let proxy = proxied.then(|| Proxy::start(ws_addr));
    let target = proxy.as_ref().map_or(ws_addr, |p| p.addr);
    let key = format!("pcx:enginetest:acp:{name}");
    let meta_url = Url::parse(&format!("http://{meta_addr}")).expect("meta url");
    connect_url(&key, &format!("ws://{target}/acp"), Some(meta_url)).expect("connect");
    Env {
        key,
        hub,
        fake,
        host,
        proxy,
        _dir: dir,
    }
}

fn env(name: &str, script: FakeScript) -> Env {
    start_env(name, script, false)
}

fn listed(id: &str, title: &str, updated: &str) -> SessionInfo {
    SessionInfo {
        session_id: id.into(),
        cwd: "/w".into(),
        title: Some(title.into()),
        updated_at: Some(updated.into()),
        meta: None,
    }
}

fn user(text: &str) -> Value {
    json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}})
}

fn agent(text: &str, message: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "messageId": message, "content": {"type": "text", "text": text}})
}

/// `turns` user/agent exchanges for session `id`.
fn replay(id: &str, turns: usize) -> FakeScript {
    let mut updates = Vec::new();
    for n in 1..=turns {
        updates.push(user(&format!("q{n}")));
        updates.push(agent(&format!("a{n}"), &format!("m{n}")));
    }
    FakeScript {
        sessions: vec![listed(id, "Session", "2026-01-02T03:04:05Z")],
        replays: [(id.to_string(), updates)].into(),
        ..FakeScript::default()
    }
}

fn wait_event(
    rx: &mut broadcast::Receiver<AppEvent>,
    pred: impl Fn(&AppEvent) -> bool,
) -> AppEvent {
    runtime::runtime().block_on(async {
        tokio::time::timeout(WAIT, async {
            loop {
                match rx.recv().await {
                    Ok(e) if pred(&e) => return e,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => panic!("event stream closed"),
                }
            }
        })
        .await
        .expect("event in time")
    })
}

fn kind(kind: &'static str) -> impl Fn(&AppEvent) -> bool {
    move |e| e.kind == kind
}

/// Events until (and including) the first `turn/completed`.
fn until_turn_completed(rx: &mut broadcast::Receiver<AppEvent>) -> Vec<AppEvent> {
    let mut out = Vec::new();
    loop {
        let e = wait_event(rx, |_| true);
        let done = e.kind == "turn/completed";
        out.push(e);
        if done {
            return out;
        }
    }
}

fn raw(e: &AppEvent) -> Value {
    serde_json::from_str(&e.raw).expect("raw json")
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn new_session(env: &Env) -> String {
    super::thread_start(&env.key, None, Some("/w".into())).expect("thread_start")
}

fn permission_options() -> Vec<Value> {
    vec![
        json!({"optionId": "once", "name": "Allow", "kind": "allow_once"}),
        json!({"optionId": "always", "name": "Always", "kind": "allow_always"}),
        json!({"optionId": "deny", "name": "Deny", "kind": "reject_once"}),
    ]
}

#[test]
fn connect_reads_caps_and_emits_capabilities_event() {
    assert_eq!(super::capabilities("pcx:enginetest:acp:none"), super::Capabilities::default());
    let env = env("caps", FakeScript::default());
    assert!(is_connected(&env.key));
    let caps = super::capabilities(&env.key);
    assert!(caps.connected && caps.images);
    assert_eq!(caps.agent_name, "Fake Agent");
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    wait_event(&mut rx, kind("acp/capabilities"));
    let state = wait_event(&mut rx, kind("acp/hub/state"));
    assert!(state.thread_id.is_none());
    let meta = raw(&state);
    assert_eq!(meta["caps"]["list"], true);
    assert_eq!(meta["agent"]["id"], "fake");
    let dto = crate::api::bridge::app_capabilities(env.key.clone());
    assert_eq!(dto.provider, "acp");
    assert!(!dto.steer && !dto.rename && !dto.compact && !dto.git_diff && !dto.fast);
    assert!(dto.images && dto.approval_options && dto.url_elicitation && dto.session_reload);
    assert!(dto.running_via_threads && dto.history_prefetch && dto.multi_select_questions);
    assert_eq!(dto.agent_name, "Fake Agent");
    let auth = super::auth_state(&env.key).expect("auth");
    assert_eq!(auth.status, "ok", "session/list answered, so no login is needed");
    let _ = env.hub.info();
}

#[test]
fn thread_list_maps_session_info() {
    let script = FakeScript {
        page_size: 1,
        sessions: vec![
            listed("a", "Alpha", "2026-01-02T03:04:05Z"),
            SessionInfo {
                session_id: "b".into(),
                cwd: "/x".into(),
                ..SessionInfo::default()
            },
            listed("c", "Gamma", "not a date"),
        ],
        ..FakeScript::default()
    };
    let env = env("list", script);
    let threads = super::thread_list(&env.key).expect("list");
    let a = threads.iter().find(|t| t.id == "a").expect("a");
    assert_eq!(
        (a.preview.as_str(), a.name.as_deref(), a.cwd.as_str()),
        ("Alpha", Some("Alpha"), "/w")
    );
    assert_eq!(a.updated_at, 1_767_323_045);
    let b = threads.iter().find(|t| t.id == "b").expect("b");
    assert_eq!((b.preview.as_str(), b.name.as_deref(), b.updated_at), ("", None, 0));
    assert_eq!(threads.iter().find(|t| t.id == "c").map(|t| t.updated_at), Some(0));
    assert_eq!(threads.len(), 3);
}

#[test]
fn thread_read_maps_items_and_turns() {
    let mut script = replay("s1", 2);
    if let Some(updates) = script.replays.get_mut("s1") {
        updates.push(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "List", "kind": "execute",
            "status": "completed", "rawInput": {"command": "ls"}}));
    }
    let env = env("read", script);
    let history = super::thread_read(&env.key, "s1").expect("read");
    let shape: Vec<(&str, &str)> = history
        .items
        .iter()
        .map(|i| (i.item_type.as_str(), i.turn_id.as_str()))
        .collect();
    assert_eq!(shape, vec![
        ("userMessage", "t1"),
        ("agentMessage", "t1"),
        ("userMessage", "t2"),
        ("agentMessage", "t2"),
        ("commandExecution", "t2"),
    ]);
    assert_eq!(history.items[3].text, "a2");
    assert_eq!(history.items[4].title, "ls");
    assert_eq!(history.turns.len(), 2);
    assert_eq!(history.turns[0].user_text, "q1");
    assert!(history.turns.iter().all(|t| t.loaded));
    assert_eq!(history.first_turn_id.as_deref(), Some("t1"));
    assert!(!history.running && !history.has_older && !history.older_unavailable);
    assert_eq!(history.cwd.as_deref(), Some("/w"));
    assert!(history.config_confirmed);
    let generation = ctx(&env.key).expect("ctx").shared().sessions["s1"]
        .generation
        .clone();
    assert_eq!(history.history_epoch.as_deref(), Some(generation.as_str()));
    assert!(history.turn_pages.is_empty());
}

#[test]
fn older_page_and_turn_page_use_windows() {
    let env = env("pages", replay("s1", 30));
    let history = super::thread_read(&env.key, "s1").expect("read");
    assert_eq!(history.items.len(), 20);
    assert!(history.has_older);
    assert_eq!(history.items[0].turn_id, "t21");
    let older = super::thread_older_page(&env.key, "s1").expect("older");
    assert_eq!(older.items.len(), 40);
    assert!(!older.has_older && !older.older_unavailable);
    assert_eq!(older.items[0].text, "q1");
    assert_eq!(older.items.last().map(|i| i.turn_id.as_str()), Some("t20"));
    let empty = super::thread_older_page(&env.key, "s1").expect("at start");
    assert!(empty.items.is_empty() && !empty.has_older);
    let turn = super::thread_turn_page(&env.key, "s1", "t3", false, false).expect("turn");
    assert_eq!(turn.turn_id, "t3");
    assert_eq!(
        turn.items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        vec!["q3", "a3"]
    );
    assert!(!turn.has_more);
    assert_eq!(
        super::thread_turn_items(&env.key, "s1", "t5")
            .expect("items")
            .len(),
        2
    );
    assert!(super::thread_turn_page(&env.key, "s1", "bogus", false, false).is_err());
}

fn long_turn_script() -> FakeScript {
    let mut updates = vec![user("q1")];
    updates.extend((0..70).map(|n| agent(&format!("part {n}"), &format!("m{n}"))));
    FakeScript {
        sessions: vec![listed("s1", "Long", "2026-01-02T03:04:05Z")],
        replays: [("s1".to_string(), updates)].into(),
        ..FakeScript::default()
    }
}

#[test]
fn turn_page_delta_only_returns_new_items() {
    let env = env("delta", long_turn_script());
    super::thread_read(&env.key, "s1").expect("read");
    let first = super::thread_turn_page(&env.key, "s1", "t1", false, true).expect("first");
    assert_eq!(first.items.len(), 60);
    assert!(first.has_more);
    let more = super::thread_turn_page(&env.key, "s1", "t1", true, true).expect("more");
    assert_eq!(more.items.len(), 11);
    assert_eq!(more.items[0].text, "part 59");
    assert!(!more.has_more);
    let again = super::thread_turn_page(&env.key, "s1", "t1", false, false).expect("again");
    assert_eq!(again.items.len(), 60);
    let cumulative =
        super::thread_turn_page(&env.key, "s1", "t1", true, false).expect("cumulative");
    assert_eq!(cumulative.items.len(), 71);
    assert_eq!(
        super::thread_turn_items(&env.key, "s1", "t1")
            .expect("all")
            .len(),
        71
    );
}

fn config_setup() -> Value {
    json!({"configOptions": [
        {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "m1",
         "options": [{"value": "m1", "name": "One"}, {"value": "m2", "name": "Two"}, {"value": "m3", "name": "Three"}]},
        {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "low",
         "options": [{"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "code",
         "options": [{"value": "code", "name": "Code"}, {"value": "plan", "name": "Plan"}]}
    ]})
}

#[test]
fn model_list_before_any_session_reads_hub_defaults() {
    let script = FakeScript {
        session_setup: config_setup(),
        ..FakeScript::default()
    };
    let env = env("defaults", script);
    assert!(!super::capabilities(&env.key).plan_mode);
    let models = super::model_list(&env.key).expect("models");
    assert_eq!(models.len(), 3);
    assert!(models[0].is_default);
    assert_eq!(env.fake.count("session/new"), 1);
    // Remembered from then on: plan mode is known and nothing probes again.
    assert!(super::capabilities(&env.key).plan_mode);
    assert_eq!(super::model_list(&env.key).expect("models").len(), 3);
    assert_eq!(env.fake.count("session/new"), 1);
}

#[test]
fn turn_start_applies_config_then_submits() {
    let script = FakeScript {
        session_setup: config_setup(),
        ..FakeScript::default()
    };
    let env = env("config", script);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    let session =
        super::thread_start(&env.key, Some("m2".into()), Some("/w".into())).expect("start");
    let models = super::model_list(&env.key).expect("models");
    assert_eq!(models.len(), 3);
    assert_eq!(models[0].supported_reasoning_efforts, vec!["low".to_string(), "high".into()]);
    let config = super::thread_runtime_config(&env.key, &session).expect("config");
    assert_eq!(config.model.as_deref(), Some("m2"));
    assert!(super::capabilities(&env.key).plan_mode);
    super::turn_start(
        &env.key,
        &session,
        "go".into(),
        vec!["data:image/png;base64,AAAA".into()],
        Some("m3".into()),
        Some("plan".into()),
        Some("high".into()),
    )
    .expect("turn");
    until_turn_completed(&mut rx);
    super::turn_start(
        &env.key,
        &session,
        "back".into(),
        Vec::new(),
        None,
        Some("default".into()),
        None,
    )
    .expect("turn 2");
    until_turn_completed(&mut rx);
    let sets: Vec<(String, String)> = env
        .fake
        .params_of("session/set_config_option")
        .iter()
        .map(|p| {
            (
                p["configId"].as_str().unwrap_or("").to_string(),
                p["value"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(sets, vec![
        pair("model", "m2"),
        pair("model", "m3"),
        pair("effort", "high"),
        pair("mode", "plan"),
        pair("mode", "code"),
    ]);
    let methods = env.fake.methods();
    let last_set = methods
        .iter()
        .rposition(|m| m == "session/set_config_option")
        .expect("set");
    let last_prompt = methods
        .iter()
        .rposition(|m| m == "session/prompt")
        .expect("prompt");
    assert!(last_set < last_prompt, "config is applied before the prompt");
    let prompt = &env.fake.params_of("session/prompt")[0]["prompt"];
    assert_eq!(prompt[0]["text"], "go");
    assert_eq!(prompt[1]["type"], "image");
    assert_eq!(prompt[1]["mimeType"], "image/png");
    let options = super::config_options(&env.key, &session).expect("options");
    assert_eq!(options.len(), 3);
    super::set_config_option(&env.key, &session, "mode", "plan", false).expect("set mode");
    assert_eq!(
        env.fake
            .params_of("session/set_config_option")
            .last()
            .expect("last")["value"],
        "plan"
    );
}

#[test]
fn stream_emits_started_delta_completed_in_order() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Update(agent("a", "m1")),
        Step::Update(agent("b", "m1")),
        Step::Update(json!({"sessionUpdate": "tool_call", "toolCallId": "x", "title": "Run", "kind": "execute",
            "status": "in_progress", "rawInput": {"command": "make"}})),
        Step::Update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "x", "status": "completed",
            "rawOutput": {"stdout": "ok", "exitCode": 0}})),
        Step::Update(json!({"sessionUpdate": "plan", "entries": [{"content": "S", "priority": "high", "status": "pending"}]})),
        Step::Finish("end_turn".into()),
    ]);
    let env = env("stream", script);
    let session = new_session(&env);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "hi".into(), Vec::new(), None, None, None).expect("turn");
    let events: Vec<AppEvent> = until_turn_completed(&mut rx)
        .into_iter()
        .filter(|e| e.thread_id.as_deref() == Some(session.as_str()))
        .collect();
    let shape: Vec<(String, Option<String>, Option<String>)> = events
        .iter()
        .map(|e| (e.kind.clone(), e.item_type.clone(), e.text.clone()))
        .collect();
    let s = |v: &str| Some(v.to_string());
    assert_eq!(shape, vec![
        ("turn/started".into(), None, None),
        ("item/started".into(), s("agentMessage"), s("")),
        ("item/agentMessage/delta".into(), s("agentMessage"), s("a")),
        ("item/agentMessage/delta".into(), s("agentMessage"), s("b")),
        ("item/started".into(), s("commandExecution"), s("")),
        ("item/completed".into(), s("commandExecution"), s("ok\n[exit 0]")),
        ("item/completed".into(), s("plan"), s("- [ ] S")),
        ("item/completed".into(), s("agentMessage"), s("ab")),
        ("turn/completed".into(), None, None),
    ]);
    let started = raw(&events[0]);
    assert_eq!(started["turnId"], "t1");
    assert_eq!(started["turn"]["status"], "inProgress");
    assert_eq!(raw(events.last().expect("done"))["turn"]["status"], "completed");
    assert_eq!(events[1].item_id, events[7].item_id);
}

#[test]
fn permission_maps_to_approval_and_decisions_map_to_options() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::RequestPermission {
            options: permission_options(),
        },
        Step::RequestPermission {
            options: permission_options(),
        },
        Step::Finish("end_turn".into()),
    ]);
    let env = env("perm", script);
    let session = new_session(&env);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "go".into(), Vec::new(), None, None, None).expect("turn");
    let first = wait_event(&mut rx, kind("item/commandExecution/requestApproval"));
    let body = raw(&first);
    assert_eq!(body["threadId"], json!(session));
    assert_eq!(body["command"], "Run");
    assert_eq!(body["cwd"], "/w");
    assert_eq!(body["acpOptions"].as_array().map(Vec::len), Some(3));
    assert_eq!(body["acpOptions"][1]["kind"], "allow_always");
    let id = first.request_id.clone().expect("request id");
    assert!(id.starts_with('r'), "the hub request id: {id}");
    assert!(super::respond_approval(&env.key, "r999", "accept").is_err());
    super::respond_approval(&env.key, &id, "acceptForSession").expect("approve");
    let second = wait_event(&mut rx, kind("item/commandExecution/requestApproval"));
    let second_id = second.request_id.clone().expect("id");
    assert_ne!(second_id, id);
    assert!(super::respond_permission_option(&env.key, &second_id, "nope").is_err());
    super::respond_permission_option(&env.key, &second_id, "deny").expect("deny");
    until_turn_completed(&mut rx);
    let answers: Vec<Value> = env
        .fake
        .answers()
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .map(|v| v["outcome"]["optionId"].clone())
        .collect();
    assert_eq!(answers, vec![json!("always"), json!("deny")]);
}

#[test]
fn form_elicitation_round_trip() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Elicit(json!({"message": "Details?", "mode": "form", "sessionId": "fake-1",
            "requestedSchema": {"type": "object", "required": ["n"], "properties": {
                "name": {"type": "string"}, "n": {"type": "integer"}}}})),
        Step::Finish("end_turn".into()),
    ]);
    let env = env("form", script);
    let session = new_session(&env);
    assert_eq!(session, "fake-1");
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "go".into(), Vec::new(), None, None, None).expect("turn");
    let card = wait_event(&mut rx, kind("item/tool/requestUserInput"));
    let body = raw(&card);
    assert_eq!(body["title"], "Details?");
    assert_eq!(body["questions"][0]["id"], "n");
    let id = card.request_id.clone().expect("id");
    let bad =
        super::respond_user_input(&env.key, &id, r#"{"n":["many"]}"#).expect_err("not a number");
    assert!(bad.to_string().contains("integer"), "{bad}");
    super::respond_user_input(&env.key, &id, r#"{"name":["Ada"],"n":["3"]}"#).expect("answer");
    until_turn_completed(&mut rx);
    let (_, answer) = env.fake.answers().into_iter().next().expect("answer");
    assert_eq!(
        answer.expect("ok"),
        json!({"action": "accept", "content": {"name": "Ada", "n": 3}})
    );
}

#[test]
fn url_elicitation_accept_and_decline() {
    let url = |id: &str| {
        Step::Elicit(
            json!({"message": "Sign in", "mode": "url", "sessionId": "fake-1", "elicitationId": id,
            "url": "https://login.example.com/device?code=1"}),
        )
    };
    let mut script = FakeScript::default();
    script
        .prompts
        .push_back(vec![url("e1"), url("e2"), Step::Finish("end_turn".into())]);
    let env = env("url", script);
    let session = new_session(&env);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "go".into(), Vec::new(), None, None, None).expect("turn");
    let first = wait_event(&mut rx, kind("acp/elicitation/url"));
    let body = raw(&first);
    assert_eq!(body["host"], "login.example.com");
    assert_eq!(body["message"], "Sign in");
    assert_eq!(first.thread_id.as_deref(), Some(session.as_str()));
    super::respond_elicitation_url(&env.key, first.request_id.as_deref().expect("id"), true)
        .expect("accept");
    let second = wait_event(&mut rx, kind("acp/elicitation/url"));
    super::respond_elicitation_url(&env.key, second.request_id.as_deref().expect("id"), false)
        .expect("decline");
    until_turn_completed(&mut rx);
    let actions: Vec<Value> = env
        .fake
        .answers()
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .map(|v| v["action"].clone())
        .collect();
    assert_eq!(actions, vec![json!("accept"), json!("decline")]);
}

#[test]
fn interrupt_sends_cancel() {
    let mut script = FakeScript::default();
    script
        .prompts
        .push_back(vec![Step::Sleep(20_000), Step::Finish("end_turn".into())]);
    let env = env("cancel", script);
    let session = new_session(&env);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "go".into(), Vec::new(), None, None, None).expect("turn");
    wait_event(&mut rx, kind("turn/started"));
    super::turn_interrupt(&env.key, &session, Some("t1".into())).expect("interrupt");
    let done = wait_event(&mut rx, kind("turn/completed"));
    assert_eq!(raw(&done)["turn"]["status"], "interrupted");
    assert_eq!(env.fake.count("session/cancel"), 1);
}

#[test]
fn reconnect_reattaches_and_resubmits_unknown_submission() {
    let env = start_env("reconnect", FakeScript::default(), true);
    let session = new_session(&env);
    let generation = ctx(&env.key).expect("ctx").shared().sessions[&session]
        .generation
        .clone();
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    ctx(&env.key)
        .expect("ctx")
        .shared()
        .unacked
        .insert("lost-1".into(), (session.clone(), vec![ContentBlock::text("again")]));
    env.proxy.as_ref().expect("proxy").kill();
    wait_until("the resubmitted prompt", || env.fake.count("session/prompt") >= 1);
    assert_eq!(env.fake.params_of("session/prompt")[0]["prompt"][0]["text"], "again");
    let done = wait_event(&mut rx, kind("turn/completed"));
    assert_eq!(done.thread_id.as_deref(), Some(session.as_str()));
    wait_until("the unacked entry is cleared", || {
        ctx(&env.key).expect("ctx").shared().unacked.is_empty()
    });
    assert!(is_connected(&env.key));
    let view_generation = ctx(&env.key).expect("ctx").shared().sessions[&session]
        .generation
        .clone();
    assert_eq!(view_generation, generation);
    // The re-attached session still streams to this controller.
    super::turn_start(&env.key, &session, "after".into(), Vec::new(), None, None, None)
        .expect("turn");
    let done = wait_event(&mut rx, kind("turn/completed"));
    assert_eq!(raw(&done)["turn"]["status"], "completed");
    assert_eq!(env.fake.count("session/prompt"), 2);
}

#[test]
fn chunks_merge_by_hub_item_id_beyond_tail() {
    let mut script = replay("s1", 30);
    script.prompts.push_back(vec![
        Step::Update(agent("x", "live")),
        Step::Update(json!({"sessionUpdate": "agent_message_chunk", "messageId": "live",
            "content": {"type": "image", "data": "AAAA", "mimeType": "image/png"}})),
        Step::Update(agent("y", "live")),
        Step::Finish("end_turn".into()),
    ]);
    let env = env("merge", script);
    super::thread_read(&env.key, "s1").expect("read");
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, "s1", "more".into(), Vec::new(), None, None, None).expect("turn");
    let events = until_turn_completed(&mut rx);
    let completed = events
        .iter()
        .find(|e| e.kind == "item/completed" && e.item_type.as_deref() == Some("agentMessage"))
        .expect("agent completed");
    assert_eq!(completed.text.as_deref(), Some("xy"));
    assert_eq!(completed.images, vec!["data:image/png;base64,AAAA".to_string()]);
    let ctx = ctx(&env.key).expect("ctx");
    let mut s = ctx.shared();
    let view = s.sessions.get_mut("s1").expect("view");
    let id = completed.item_id.clone().expect("id");
    let item = view.item(&id).expect("merged item");
    assert_eq!(item.turn, 31);
    assert_eq!(item.content.len(), 2, "text merged into one block, image appended");
    // An id outside the retained window is created from the chunk.
    assert!(view.item("m:agent:m1").is_none(), "turn 1 is beyond the tail");
    let created = view.append_chunk("m:agent:m1", "agent", 1, &ContentBlock::text("late"));
    assert_eq!((created.kind.as_str(), created.turn), ("agent", 1));
    view.append_chunk("m:agent:m1", "agent", 1, &ContentBlock::text(" more"));
    assert_eq!(view.item("m:agent:m1").and_then(|i| i.content[0].as_text()), Some("late more"));
}

#[test]
fn thread_read_waits_for_loaded_notification() {
    let script = FakeScript {
        load_delay_ms: 21_000,
        ..replay("s1", 1)
    };
    let env = env("loading", script);
    let started = Instant::now();
    let history = super::thread_read(&env.key, "s1").expect("read after loading");
    assert!(started.elapsed() >= Duration::from_secs(20), "the first attach answered loading");
    assert_eq!(history.items.len(), 2);
    assert_eq!(env.fake.count("session/load"), 1);
}

#[test]
fn thread_start_without_cwd_uses_default_project() {
    let env = env("cwd", FakeScript::default());
    let missing = super::thread_start(&env.key, None, None).expect_err("no default project");
    assert!(missing.to_string().starts_with("[acp.cwd_required] "), "{missing}");
    assert_eq!(pcx_code(&missing).as_deref(), Some("acp.cwd_required"));
    runtime::runtime()
        .block_on(env.host.put(HostConfig {
            project_roots: vec!["/proj".into()],
            default_project: Some("/proj".into()),
        }))
        .expect("put");
    let session = super::thread_start(&env.key, None, None).expect("start");
    assert_eq!(env.fake.params_of("session/new")[0]["cwd"], "/proj");
    assert!(ctx(&env.key)
        .expect("ctx")
        .shared()
        .sessions
        .contains_key(&session));
}

#[test]
fn pcx_code_is_parsed_from_error_message() {
    let env = env("codes", replay("s1", 30));
    super::thread_read(&env.key, "s1").expect("read");
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    ctx(&env.key)
        .expect("ctx")
        .shared()
        .sessions
        .get_mut("s1")
        .expect("view")
        .generation = "stale".into();
    let error = super::thread_older_page(&env.key, "s1").expect_err("stale generation");
    assert_eq!(pcx_code(&error).as_deref(), Some("acp.generation_changed"));
    assert!(error.to_string().starts_with("[acp.generation_changed] "), "{error}");
    wait_event(&mut rx, kind("acp/session/generation"));
    let wrapped = anyhow::anyhow!("[acp.session_loading] still loading").context("reading history");
    assert_eq!(pcx_code(&wrapped).as_deref(), Some("acp.session_loading"));
    assert_eq!(pcx_code(&anyhow::anyhow!("plain failure")), None);
    let unknown = super::thread_read(&env.key, "missing").expect_err("unknown session");
    assert_eq!(pcx_code(&unknown).as_deref(), Some("acp.session_not_loadable"));
    assert!(!ctx(&env.key)
        .expect("ctx")
        .shared()
        .sessions
        .contains_key("missing"));
}

#[test]
fn generation_change_emits_event() {
    let mut script = replay("s1", 3);
    script.replays.insert("s1".into(), vec![
        user("q1"),
        agent("a1", "m1"),
        user("q2"),
        agent("a2", "m2"),
    ]);
    let env = env("generation", script);
    super::thread_read(&env.key, "s1").expect("read");
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    env.fake.update_script(|s| {
        s.replays.insert("s1".into(), vec![
            user("q1"),
            agent("rewritten", "m1"),
            user("q2"),
            agent("a2", "m2"),
        ]);
    });
    super::thread_reload(&env.key, "s1").expect("reload");
    wait_event(&mut rx, kind("acp/session/generation"));
    // Another controller reloads: the hub's `_pcx/session/generation` reaches
    // this one too.
    env.fake.update_script(|s| {
        s.replays.insert("s1".into(), vec![
            user("q1"),
            agent("again", "m1"),
            user("q2"),
            agent("a2", "m2"),
        ]);
    });
    let _runtime = runtime::runtime().enter();
    let (other, mut other_rx) = env.hub.open_connection();
    use pocket_codex_core::acp::rpc::{RequestId, RpcMessage};
    other.handle(RpcMessage::Request {
        id: RequestId::Number(1),
        method: "initialize".into(),
        params: json!({"protocolVersion": 1}),
    });
    other.handle(RpcMessage::Request {
        id: RequestId::Number(2),
        method: "_pcx/session/reload".into(),
        params: json!({"sessionId": "s1"}),
    });
    runtime::runtime().block_on(async {
        tokio::time::timeout(WAIT, async {
            while let Some(m) = other_rx.recv().await {
                if matches!(&m, RpcMessage::Response {
                    id: RequestId::Number(2),
                    ..
                }) {
                    return;
                }
            }
        })
        .await
        .expect("reload answered")
    });
    let event = wait_event(&mut rx, kind("acp/session/generation"));
    assert_eq!(event.thread_id.as_deref(), Some("s1"));
    let history = super::thread_read(&env.key, "s1").expect("re-read");
    assert_eq!(history.items[1].text, "again");
}

#[test]
fn usage_maps_to_token_usage_shape() {
    let mut script = FakeScript::default();
    script.prompts.push_back(vec![
        Step::Update(json!({"sessionUpdate": "usage_update", "used": 1200, "size": 200_000})),
        Step::Finish("end_turn".into()),
    ]);
    let env = env("usage", script);
    let session = new_session(&env);
    let mut rx = subscribe_events(&env.key).expect("subscribe");
    super::turn_start(&env.key, &session, "go".into(), Vec::new(), None, None, None).expect("turn");
    let usage = wait_event(&mut rx, kind("thread/tokenUsage/updated"));
    let body = raw(&usage);
    assert_eq!(body["threadId"], json!(session));
    assert_eq!(body["tokenUsage"]["last"]["totalTokens"], 1200);
    assert_eq!(body["tokenUsage"]["modelContextWindow"], 200_000);
    until_turn_completed(&mut rx);
    let history = super::thread_read(&env.key, &session).expect("read");
    assert_eq!((history.tokens_used, history.context_window), (Some(1200), Some(200_000)));
}

#[test]
fn steer_rename_compact_diff_are_unsupported() {
    use crate::api::bridge;
    let env = env("unsupported", FakeScript::default());
    let session = new_session(&env);
    let key = env.key.clone();
    let unsupported = |result: anyhow::Result<()>| {
        let error = result.expect_err("unsupported");
        assert!(error.to_string().contains("not supported by ACP agents"), "{error}");
    };
    unsupported(
        bridge::app_turn_steer(key.clone(), session.clone(), None, "x".into(), None).map(|_| ()),
    );
    unsupported(bridge::app_set_thread_name(key.clone(), session.clone(), "n".into()));
    unsupported(bridge::app_compact(key.clone(), session.clone()));
    unsupported(bridge::app_git_diff(key.clone(), "/w".into()).map(|_| ()));
    unsupported(bridge::app_rate_limits(key.clone()).map(|_| ()));
    unsupported(bridge::app_force_resume(key.clone(), session.clone()).map(|_| ()));
    let summary =
        runtime::runtime().block_on(bridge::app_thread_summary(key.clone(), session.clone()));
    assert!(summary.expect("summary").is_none());
    let running = bridge::app_running_threads(key).expect("running");
    assert!(running.is_empty());
}

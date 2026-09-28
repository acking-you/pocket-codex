//! The engine against a fake OpenCode gateway: every request the engine
//! makes is recorded, and canned native responses come back.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
};

use super::{conns, lock, Conn, Pending, Shared};
use crate::engine::runtime;

type Log = Arc<Mutex<Vec<(String, String, Value)>>>;

fn exchange(n: u64) -> [Value; 2] {
    [
        json!({"id": format!("msg_u{n}"), "type": "user", "time": {"created": n * 10}, "text": format!("q{n}")}),
        json!({"id": format!("msg_a{n}"), "type": "assistant", "time": {"created": n * 10 + 1},
            "content": [{"type": "text", "text": format!("a{n}")}]}),
    ]
}

/// Turns 1..=30 fill the newest 60-message page; turn 0 is one page back.
/// Pages come newest first, as the native API sends them.
fn messages(target: &str) -> Value {
    let mut data: Vec<Value> = if target.contains("type=user") {
        (0..=30).map(|n| exchange(n)[0].clone()).collect()
    } else if target.contains("cursor=c1") {
        exchange(0).to_vec()
    } else {
        (1..=30).flat_map(exchange).collect()
    };
    data.reverse();
    let next = (data.len() == 60).then_some("c1");
    json!({"data": data, "cursor": {"next": next}})
}

fn upstream(method: &str, target: &str, body: &Value) -> (u16, Value) {
    let path = target.split('?').next().unwrap_or("");
    let form = json!({"id": "frm_1", "sessionID": "ses_1", "title": "Pick",
        "fields": [{"key": "color", "type": "string",
            "options": [{"label": "Red", "value": "red"}, {"label": "Blue", "value": "blue"}]}]});
    match (method, path) {
        ("GET", "/api/session/ses_1") => (
            200,
            json!({"data": {
            "id": "ses_1", "agent": "build", "model": {"providerID": "p", "id": "m"},
            "location": {"directory": "/w"}}}),
        ),
        ("POST", "/api/session/ses_1/prompt") => (
            200,
            json!({"data": {
            "id": "msg_u1", "sessionID": "ses_1", "type": "user", "delivery": body["delivery"]}}),
        ),
        ("POST", "/api/session/ses_1/interrupt") => (200, json!({"interrupted": true})),
        ("GET", "/api/session/ses_1/form") => (200, json!({"data": [form]})),
        ("GET", "/api/session") => (
            200,
            json!({"data": [{
            "id": "ses_1", "title": "One", "location": {"directory": "/w"}, "time": {"updated": 5}}]}),
        ),
        ("GET", "/api/session/active") => (200, json!({"data": {"ses_1": {"type": "running"}}})),
        ("GET", "/api/session/ses_1/message") => (200, messages(target)),
        ("GET", "/api/vcs/diff") => (
            200,
            json!({"data": [
            {"file": "a.txt", "patch": "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-x\n+y", "status": "modified"},
            {"file": "clean.txt", "patch": "", "status": "modified"}]}),
        ),
        ("GET", _) => (404, json!({})),
        _ => (200, json!({})),
    }
}

async fn serve_one(mut socket: TcpStream, log: Log) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_string)
        })
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + length {
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
    let mut request = head.lines().next().unwrap_or("").split(' ');
    let method = request.next().unwrap_or("").to_string();
    let target = request.next().unwrap_or("").to_string();
    let (status, reply) = upstream(&method, &target, &body);
    log.lock().expect("log").push((method, target, body));
    let text = reply.to_string();
    let response = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: \
         {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
    socket.write_all(response.as_bytes()).await
}

/// Register a connection named `name` to a fresh fake gateway.
fn connect_fake(
    name: &str,
) -> (String, Log, Arc<Mutex<Shared>>, broadcast::Receiver<super::AppEvent>) {
    runtime::init(std::env::temp_dir()).expect("runtime");
    let listener = runtime::runtime()
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let log: Log = Arc::default();
    let server_log = log.clone();
    runtime::runtime().spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve_one(socket, server_log.clone()));
        }
    });
    let key = format!("pcx:test:opencode:{name}");
    let client = pocket_codex_host_svc::opencode::Client::new(&format!("http://{addr}/"), None)
        .expect("client");
    let (tx, rx) = broadcast::channel(64);
    let shared = Arc::new(Mutex::new(Shared::default()));
    let task = runtime::runtime().spawn(std::future::pending::<()>());
    conns().insert(key.clone(), Conn {
        client,
        tx,
        task,
        shared: shared.clone(),
    });
    (key, log, shared, rx)
}

fn requests(log: &Log) -> Vec<(String, String, Value)> {
    log.lock().expect("log").clone()
}

fn posts(log: &Log) -> Vec<(String, Value)> {
    requests(log)
        .into_iter()
        .filter(|(method, _, _)| method != "GET")
        .map(|(method, target, body)| (format!("{method} {target}"), body))
        .collect()
}

#[test]
fn a_plan_turn_selects_model_variant_and_agent_before_queueing() {
    let (key, log, shared, _rx) = connect_fake("turn");
    super::turn_start(
        &key,
        "ses_1",
        "hi".into(),
        vec!["data:image/jpeg;base64,AA".into()],
        Some("p/m".into()),
        Some("plan".into()),
        Some("high".into()),
    )
    .expect("turn");
    let sent = posts(&log);
    assert_eq!(sent[0].0, "POST /api/session/ses_1/model");
    assert_eq!(sent[0].1["model"], json!({"providerID": "p", "id": "m", "variant": "high"}));
    assert_eq!(sent[1], ("POST /api/session/ses_1/agent".into(), json!({"agent": "plan"})));
    assert_eq!(sent[2].0, "POST /api/session/ses_1/prompt");
    assert_eq!(sent[2].1["delivery"], "queue");
    assert_eq!(sent[2].1["files"][0]["name"], "image-1.jpeg");
    assert_eq!(sent.len(), 3);
    let config = super::thread_runtime_config(&key, "ses_1").expect("config");
    assert_eq!(config.model.as_deref(), Some("p/m"));
    assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(config.collaboration_mode.as_deref(), Some("plan"));
    assert_eq!(lock(&shared).translator.turn_id("ses_1"), "msg_u1");
    super::disconnect(&key);
}

#[test]
fn an_unchanged_default_turn_only_prompts() {
    let (key, log, _, _rx) = connect_fake("same");
    super::turn_start(
        &key,
        "ses_1",
        "hi".into(),
        Vec::new(),
        Some("p/m".into()),
        Some("default".into()),
        None,
    )
    .expect("turn");
    let sent = posts(&log);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "POST /api/session/ses_1/prompt");
    super::disconnect(&key);
}

#[test]
fn data_url_images_become_prompt_files_with_names() {
    let (key, log, _, _rx) = connect_fake("images");
    let png = "data:image/png;base64,iVBORw0KGgo=".to_string();
    let jpeg = "data:image/jpeg;base64,/9j/4AAQ".to_string();
    super::turn_start(
        &key,
        "ses_1",
        "look".into(),
        vec![png.clone(), jpeg.clone()],
        None,
        None,
        None,
    )
    .expect("turn");
    let sent = posts(&log);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "POST /api/session/ses_1/prompt");
    assert_eq!(sent[0].1["text"], "look");
    assert_eq!(
        sent[0].1["files"],
        json!([{"uri": png, "name": "image-1.png"}, {"uri": jpeg, "name": "image-2.jpeg"}])
    );
    super::disconnect(&key);
}

#[test]
fn steering_needs_a_running_turn_and_interrupt_needs_none() {
    let (key, log, shared, _rx) = connect_fake("steer");
    assert!(super::turn_steer(&key, "ses_1", None, "more", &[]).is_err());
    {
        let mut s = lock(&shared);
        s.active.insert("ses_1".into());
        s.translator.turns.insert("ses_1".into(), "msg_u0".into());
    }
    assert_eq!(super::turn_steer(&key, "ses_1", None, "more", &[]).expect("steer"), "msg_u0");
    super::turn_interrupt(&key, "ses_1", None).expect("interrupt");
    let sent = posts(&log);
    assert_eq!(sent[0].1["delivery"], "steer");
    assert_eq!(sent[1].0, "POST /api/session/ses_1/interrupt");
    super::disconnect(&key);
}

#[test]
fn replies_settle_pending_requests() {
    let (key, log, shared, mut rx) = connect_fake("reply");
    {
        let mut s = lock(&shared);
        s.pending.insert("per_1".into(), Pending::Permission {
            session: "ses_1".into(),
        });
    }
    assert!(super::respond_user_input(&key, "per_1", "{}").is_err());
    super::respond_approval(&key, "per_1", "acceptForSession").expect("approve");
    assert!(super::respond_approval(&key, "per_1", "accept").is_err());
    let resolved = rx.try_recv().expect("resolved");
    assert_eq!(resolved.kind, "serverRequest/resolved");

    let form = serde_json::from_value(
        upstream("GET", "/api/session/ses_1/form", &Value::Null).1["data"][0].clone(),
    )
    .expect("form");
    lock(&shared)
        .pending
        .insert("frm_1".into(), Pending::Form(form));
    super::respond_user_input(&key, "frm_1", r#"{"color":["Blue"]}"#).expect("answer");
    let sent = posts(&log);
    assert_eq!(
        sent[0],
        ("POST /api/session/ses_1/permission/per_1/reply".into(), json!({"decision": "always"}))
    );
    assert_eq!(
        sent[1],
        ("POST /api/session/ses_1/form/frm_1/reply".into(), json!({"answer": {"color": "blue"}}))
    );
    assert!(lock(&shared).pending.is_empty());
    super::disconnect(&key);
}

#[test]
fn an_empty_answer_cancels_the_form() {
    let (key, log, shared, _rx) = connect_fake("cancel");
    let form = serde_json::from_value(
        upstream("GET", "/api/session/ses_1/form", &Value::Null).1["data"][0].clone(),
    )
    .expect("form");
    lock(&shared)
        .pending
        .insert("frm_1".into(), Pending::Form(form));
    super::respond_user_input(&key, "frm_1", "{}").expect("cancel");
    assert_eq!(posts(&log)[0].0, "DELETE /api/session/ses_1/form/frm_1");
    super::disconnect(&key);
}

#[test]
fn the_working_tree_diff_is_one_unified_text() {
    let (key, log, _, _rx) = connect_fake("diff");
    let diff = super::git_diff(&key, "/w").expect("diff");
    assert_eq!(diff, "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-x\n+y\n");
    assert!(requests(&log)[0].1.contains("location%5Bdirectory%5D=%2Fw"));
    super::disconnect(&key);
}

#[test]
fn sessions_list_read_and_page_back_to_the_first_turn() {
    let (key, _log, _, _rx) = connect_fake("history");
    let threads = super::thread_list(&key).expect("list");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].cwd, "/w");
    assert_eq!(super::running_sessions(&key).expect("running"), vec!["ses_1".to_string()]);

    let history = super::thread_read(&key, "ses_1").expect("read");
    assert!(history.running);
    assert!(history.has_older);
    assert_eq!(history.active_turn_id.as_deref(), Some("msg_u30"));
    assert_eq!(history.first_turn_id.as_deref(), Some("msg_u0"));
    assert_eq!(history.turns.len(), 31);
    assert_eq!(history.items.len(), 60);
    assert_eq!(history.cwd.as_deref(), Some("/w"));
    assert_eq!(history.model.as_deref(), Some("p/m"));
    assert_eq!(history.collaboration_mode.as_deref(), Some("default"));

    let turn = super::thread_turn_page(&key, "ses_1", "msg_u0", false).expect("jump");
    let ids: Vec<&str> = turn.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec!["msg_u0", "msg_a0:t0"]);
    let older = super::thread_older_page(&key, "ses_1").expect("older");
    assert!(older.items.is_empty(), "the jump already read the first page");
    assert!(!older.has_older);
    super::disconnect(&key);
}

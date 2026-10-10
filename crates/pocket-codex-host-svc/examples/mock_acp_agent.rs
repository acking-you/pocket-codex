//! A scripted ACP v1 agent for tests. It is a Cargo example, so it is built
//! by `cargo test` but never installed or packaged.
//!
//! Flags select one of three deliberately different capability sets:
//! `--profile alpha` (core only), `--profile beta` (load, list, resume,
//! close, images, config options and modes) and `--profile gamma` (list and
//! resume, no load). Other flags:
//!
//! - `--version N` answers `initialize` with another protocol version;
//! - `--auth-required` refuses `session/new`, `--auth-prompt` refuses
//!   `session/prompt`, both with ACP `auth_required`;
//! - `--list-cwd PATH` lists the earlier session under `PATH`;
//! - `--open-delay-ms N` delays the `session/load` / `session/resume` answer,
//!   and `--fail-open` then fails it;
//! - `--init-delay-ms N` delays the `initialize` answer;
//! - `--pid-file PATH` writes the agent's process id to `PATH` at start;
//! - `--clear-title` makes `session/load` clear the title (`title: null`)
//!   instead of naming the session.
//!
//! The agent title echoes the raw argument vector so tests can verify it
//! arrived verbatim.
//!
//! Prompts are scripts: `hello` streams two chunks; `permission` asks for a
//! permission and reports the chosen option; `slow` streams until cancelled
//! and then sends a trailing update; `stream` sends forty chunks over about
//! a second; `flood` sends 5000 chunks at once and then waits for a cancel;
//! `count` reports how many prompts reached the agent; `fs` asks the client
//! to read a file and reports the error code it got; `crash` exits
//! immediately; `orphan` leaves a child holding the output open and exits;
//! `linger` ends by itself after 400 ms whatever happens; `probe` waits
//! 300 ms and reports whether a cancel reached it (`probe:cancelled` /
//! `probe:clean`); `deaf` asks for a permission and then never reads its
//! input again (the process stays alive), so the client's writes stall.

use std::{
    collections::HashMap,
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{oneshot, Notify},
};

#[derive(Clone)]
struct Agent {
    profile: String,
    args: Vec<String>,
    version: u64,
    auth_required: bool,
    auth_prompt: bool,
    list_cwd: Option<String>,
    open_delay: Duration,
    fail_open: bool,
    init_delay: Duration,
    clear_title: bool,
    next_id: Arc<Mutex<i64>>,
    waiting: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    cancels: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    sessions: Arc<Mutex<u64>>,
    prompts: Arc<AtomicU64>,
}

fn emit(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, value);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn respond(id: &Value, result: Value) {
    emit(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn fail(id: &Value, code: i64, message: &str) {
    emit(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
}

fn update(session: &str, update: Value) {
    emit(&json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}}));
}

fn text(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

impl Agent {
    fn has_load(&self) -> bool {
        self.profile == "beta"
    }

    fn has_list_and_resume(&self) -> bool {
        matches!(self.profile.as_str(), "beta" | "gamma")
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        let id = {
            let mut next = self.next_id.lock().expect("lock");
            *next += 1;
            *next
        };
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().expect("lock").insert(id, tx);
        emit(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        rx.await.unwrap_or(Value::Null)
    }

    fn cancel_signal(&self, session: &str) -> Arc<Notify> {
        self.cancels
            .lock()
            .expect("lock")
            .entry(session.to_string())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    fn settings(&self) -> Value {
        if self.profile != "beta" {
            return json!({});
        }
        json!({
            "configOptions": [{
                "id": "model", "name": "Model", "category": "model", "type": "select",
                "currentValue": "fast-1",
                "options": [
                    {"value": "fast-1", "name": "Fast"},
                    {"group": "big", "name": "Large", "options": [{"value": "large-2", "name": "Large 2"}]},
                ],
            }],
            "modes": {"currentModeId": "ask", "availableModes": [
                {"id": "ask", "name": "Ask"}, {"id": "code", "name": "Code"},
            ]},
        })
    }

    async fn handle(
        self,
        id: Value,
        method: String,
        params: Value,
        cancelled: Option<Arc<Notify>>,
    ) {
        match method.as_str() {
            "initialize" => {
                tokio::time::sleep(self.init_delay).await;
                let mut capabilities = json!({});
                if self.has_load() {
                    capabilities = json!({
                        "loadSession": true,
                        "promptCapabilities": {"image": true},
                        "sessionCapabilities": {"list": {}, "resume": {}, "close": {}},
                    });
                } else if self.has_list_and_resume() {
                    capabilities = json!({"sessionCapabilities": {"list": {}, "resume": {}}});
                }
                respond(
                    &id,
                    json!({
                        "protocolVersion": self.version,
                        "agentCapabilities": capabilities,
                        "agentInfo": {
                            "name": format!("{}-agent", self.profile),
                            "title": serde_json::to_string(&self.args).unwrap_or_default(),
                            "version": "0.0.1",
                        },
                        "authMethods": [{"id": "agent-login", "name": "Agent login"}],
                        "_meta": {"ignored": true},
                    }),
                );
            },
            "session/new" => {
                if self.auth_required {
                    fail(&id, -32000, "Authentication required");
                    return;
                }
                let n = {
                    let mut sessions = self.sessions.lock().expect("lock");
                    *sessions += 1;
                    *sessions
                };
                let mut result = self.settings();
                result["sessionId"] = json!(format!("{}-s{n}", self.profile));
                respond(&id, result);
            },
            "session/list" => {
                let cwd = self
                    .list_cwd
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
                respond(
                    &id,
                    json!({"sessions": [{
                        "sessionId": "old-1", "cwd": cwd, "title": "Earlier work",
                        "updatedAt": "2026-10-01T00:00:00Z",
                    }]}),
                );
            },
            "session/load" | "session/resume" => {
                let session = params["sessionId"].as_str().unwrap_or("").to_string();
                if method == "session/load" {
                    for (question, answer) in
                        [("first question", "first answer"), ("second question", "second answer")]
                    {
                        update(&session, text("user_message_chunk", question));
                        update(&session, text("agent_message_chunk", answer));
                    }
                    // Reported while the reopen is still pending.
                    let title = if self.clear_title { Value::Null } else { json!("Loaded title") };
                    update(
                        &session,
                        json!({"sessionUpdate": "session_info_update", "title": title}),
                    );
                }
                tokio::time::sleep(self.open_delay).await;
                if self.fail_open {
                    fail(&id, -32603, "the session could not be restored");
                } else {
                    respond(&id, self.settings());
                }
            },
            "session/close" | "session/set_mode" => respond(&id, json!({})),
            "session/set_config_option" => {
                let mut settings = self.settings();
                settings["configOptions"][0]["currentValue"] = params["value"].clone();
                respond(&id, json!({"configOptions": settings["configOptions"]}));
            },
            "session/prompt" => {
                let cancelled = cancelled.unwrap_or_else(|| Arc::new(Notify::new()));
                self.prompt(id, params, cancelled).await;
            },
            _ => fail(&id, -32601, "unknown method"),
        }
    }

    async fn prompt(self, id: Value, params: Value, cancelled: Arc<Notify>) {
        let session = params["sessionId"].as_str().unwrap_or("").to_string();
        let script = params["prompt"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if self.auth_prompt {
            fail(&id, -32000, "Authentication required");
            return;
        }
        match script.as_str() {
            "crash" => std::process::exit(3),
            "orphan" => {
                // The child inherits stdout and keeps it open after we exit.
                let child = std::process::Command::new("/bin/sleep")
                    .arg("30")
                    .stdin(std::process::Stdio::null())
                    .spawn();
                let pid = child.as_ref().map_or(0, std::process::Child::id);
                update(&session, text("agent_message_chunk", &format!("orphan:{pid}")));
                // Let the host read the report before the exit is noticed.
                std::thread::sleep(Duration::from_millis(300));
                std::process::exit(0);
            },
            "permission" => {
                update(
                    &session,
                    json!({"sessionUpdate": "tool_call", "toolCallId": "call-1",
                    "title": "Run ls", "kind": "execute", "status": "pending"}),
                );
                let answer = self
                    .call("session/request_permission", json!({
                        "sessionId": session,
                        "toolCall": {"toolCallId": "call-1", "title": "Run ls"},
                        "options": [
                            {"optionId": "opt:allow ✓", "name": "Allow once", "kind": "allow_once"},
                            {"optionId": "opt:deny", "name": "Deny", "kind": "reject_once"},
                        ],
                    }))
                    .await;
                let outcome = &answer["result"]["outcome"];
                let report = match outcome["outcome"].as_str() {
                    Some("selected") => {
                        format!("selected:{}", outcome["optionId"].as_str().unwrap_or(""))
                    },
                    Some("cancelled") => "cancelled".to_string(),
                    _ => format!("unexpected:{answer}"),
                };
                update(
                    &session,
                    json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-1",
                    "status": "completed"}),
                );
                update(&session, text("agent_message_chunk", &report));
                let stop = if report == "cancelled" { "cancelled" } else { "end_turn" };
                respond(&id, json!({"stopReason": stop}));
            },
            "deaf" => {
                // `main` stops reading once this prompt arrived; the answer
                // to this request can never be read.
                let _ = self
                    .call(
                        "session/request_permission",
                        json!({
                            "sessionId": session,
                            "toolCall": {"toolCallId": "call-deaf", "title": "Run ls"},
                            "options": [
                                {"optionId": "allow", "name": "Allow once", "kind": "allow_once"},
                            ],
                        }),
                    )
                    .await;
            },
            "slow" => {
                update(&session, text("agent_message_chunk", "working"));
                cancelled.notified().await;
                // Finishing takes a while, as real tool aborts do.
                tokio::time::sleep(Duration::from_millis(400)).await;
                update(
                    &session,
                    json!({"sessionUpdate": "tool_call", "toolCallId": "late",
                    "title": "Trailing update", "status": "failed"}),
                );
                respond(&id, json!({"stopReason": "cancelled"}));
            },
            "stream" => {
                for n in 0..40 {
                    update(&session, text("agent_message_chunk", &format!("part-{n} ")));
                    if n == 20 {
                        update(
                            &session,
                            json!({"sessionUpdate": "tool_call", "toolCallId": "mid",
                            "title": "Look around", "kind": "read", "status": "completed"}),
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                respond(&id, json!({"stopReason": "end_turn"}));
            },
            "flood" => {
                for _ in 0..5000 {
                    update(&session, text("agent_message_chunk", "x"));
                }
                cancelled.notified().await;
                update(&session, text("agent_message_chunk", "tail"));
                respond(&id, json!({"stopReason": "cancelled"}));
            },
            "linger" => {
                tokio::time::sleep(Duration::from_millis(400)).await;
                update(&session, text("agent_message_chunk", "lingered"));
                respond(&id, json!({"stopReason": "end_turn"}));
            },
            "probe" => {
                let hit = tokio::time::timeout(Duration::from_millis(300), cancelled.notified())
                    .await
                    .is_ok();
                let report = if hit { "probe:cancelled" } else { "probe:clean" };
                update(&session, text("agent_message_chunk", report));
                let stop = if hit { "cancelled" } else { "end_turn" };
                respond(&id, json!({"stopReason": stop}));
            },
            "count" => {
                let count = self.prompts.load(Ordering::SeqCst);
                update(&session, text("agent_message_chunk", &format!("prompts:{count}")));
                respond(&id, json!({"stopReason": "end_turn"}));
            },
            "fs" => {
                let answer = self
                    .call("fs/read_text_file", json!({"sessionId": session, "path": "/etc/hosts"}))
                    .await;
                let code = answer["error"]["code"].as_i64().unwrap_or(0);
                update(&session, text("agent_message_chunk", &format!("fs:{code}")));
                respond(&id, json!({"stopReason": "end_turn"}));
            },
            _ => {
                update(&session, text("agent_message_chunk", "Hi"));
                update(&session, text("agent_message_chunk", " there"));
                update(&session, json!({"sessionUpdate": "usage_update", "used": 10, "size": 100}));
                update(&session, json!({"sessionUpdate": "future_update_kind", "x": 1}));
                respond(&id, json!({"stopReason": "end_turn"}));
            },
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |name: &str| args.iter().any(|arg| arg == name);
    let agent = Agent {
        profile: flag("--profile").unwrap_or_else(|| "alpha".into()),
        version: flag("--version").and_then(|v| v.parse().ok()).unwrap_or(1),
        auth_required: has("--auth-required"),
        auth_prompt: has("--auth-prompt"),
        list_cwd: flag("--list-cwd"),
        open_delay: Duration::from_millis(
            flag("--open-delay-ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        ),
        fail_open: has("--fail-open"),
        init_delay: Duration::from_millis(
            flag("--init-delay-ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        ),
        clear_title: has("--clear-title"),
        args: args.clone(),
        next_id: Arc::new(Mutex::new(0)),
        waiting: Arc::new(Mutex::new(HashMap::new())),
        cancels: Arc::new(Mutex::new(HashMap::new())),
        sessions: Arc::new(Mutex::new(0)),
        prompts: Arc::new(AtomicU64::new(0)),
    };
    if let Some(path) = flag("--pid-file") {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(frame) = serde_json::from_str::<Value>(&line) else { continue };
        match (frame.get("id").cloned(), frame["method"].as_str()) {
            (Some(id), Some(method)) => {
                // A turn's cancel signal is installed here, in line order,
                // so a `session/cancel` that follows the prompt always
                // reaches this turn and never an earlier or later one.
                let cancelled = (method == "session/prompt").then(|| {
                    agent.prompts.fetch_add(1, Ordering::SeqCst);
                    let session = frame["params"]["sessionId"].as_str().unwrap_or("");
                    let signal = Arc::new(Notify::new());
                    agent
                        .cancels
                        .lock()
                        .expect("lock")
                        .insert(session.to_string(), signal.clone());
                    signal
                });
                tokio::spawn(agent.clone().handle(
                    id,
                    method.to_string(),
                    frame["params"].clone(),
                    cancelled,
                ));
                if method == "session/prompt" && frame["params"]["prompt"][0]["text"] == "deaf" {
                    // Alive, but never reading again.
                    std::future::pending::<()>().await;
                }
            },
            (None, Some("session/cancel")) => {
                let session = frame["params"]["sessionId"].as_str().unwrap_or("");
                agent.cancel_signal(session).notify_one();
            },
            (Some(id), None) => {
                if let Some(waiter) = id
                    .as_i64()
                    .and_then(|id| agent.waiting.lock().expect("lock").remove(&id))
                {
                    let _ = waiter.send(frame);
                }
            },
            _ => {},
        }
    }
}

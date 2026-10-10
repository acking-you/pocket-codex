//! Opt-in acceptance against a real ACP executable and relay. Run this ignored
//! test in two separate processes, with `PCX_ACP_LIVE_ROLE=host|controller` and
//! `PCX_ACP_LIVE_ROOT` pointing to a private fixture directory. The directory
//! supplies `agent.json`, `fixture/`, and separate `host-support/config.toml`
//! and `controller-support/config.toml`. Never use an ordinary App support dir.
//! The controller writes `stop` on completion (including panic); the host also
//! has a ten-minute deadline. Only the supplied fixture receives agent edits.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use pocket_codex_host_svc::acp::AgentSpec;
use serde_json::{json, Value};
use tokio::sync::broadcast::Receiver;

use crate::engine::{
    app_session::AppEvent,
    meta, runtime, serve, serve_acp,
    session_engine::{self, SessionEngine, StartOptions, TurnOptions},
    transport,
};

const WAIT: Duration = Duration::from_secs(120);

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("stop"), b"stop");
    }
}

struct Hosted;

impl Drop for Hosted {
    fn drop(&mut self) {
        serve_acp::stop_all();
    }
}

fn save(root: &Path, name: &str, value: &Value) {
    let temporary = root.join(format!("{name}.tmp"));
    std::fs::write(&temporary, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    std::fs::rename(temporary, root.join(name)).unwrap();
}

fn event(rx: &mut Receiver<AppEvent>, mut hit: impl FnMut(&AppEvent) -> bool) -> AppEvent {
    runtime::runtime().block_on(async {
        tokio::time::timeout(WAIT, async {
            loop {
                let ev = rx.recv().await.expect("live event stream must remain open");
                if hit(&ev) {
                    return ev;
                }
            }
        })
        .await
        .expect("timed out waiting for the real ACP agent")
    })
}

fn completed(rx: &mut Receiver<AppEvent>, tid: &str, status: &str) -> usize {
    let mut deltas = 0;
    let end = event(rx, |ev| {
        if ev.thread_id.as_deref() != Some(tid) {
            return false;
        }
        deltas += usize::from(ev.kind == "item/agentMessage/delta");
        ev.kind == "turn/completed"
    });
    let body: Value = serde_json::from_str(&end.raw).unwrap();
    assert_eq!(body["turn"]["status"], status, "{body}");
    deltas
}

fn prompt(engine: &dyn SessionEngine, key: &str, tid: &str, text: &str) {
    engine
        .turn_start(key, tid, TurnOptions {
            text: text.into(),
            ..Default::default()
        })
        .expect("admit real prompt through relay");
}

fn host(root: &Path, name: &str) {
    runtime::init(root.join("host-support")).unwrap();
    let _cleanup = Hosted;
    let spec: AgentSpec = serde_json::from_slice(&std::fs::read(root.join("agent.json")).unwrap())
        .expect("explicit isolated executable specification");
    let hosted = serve_acp::start(Some(name.into()), spec).expect("start real ACP host");
    let deadline = Instant::now() + WAIT;
    loop {
        if serve::serve_status()
            .iter()
            .any(|s| s.name == name && s.provider == "acp" && s.app_registered && s.meta_registered)
        {
            break;
        }
        assert!(Instant::now() < deadline, "ACP and meta registration must succeed");
        std::thread::sleep(Duration::from_millis(200));
    }
    save(
        root,
        "ready.json",
        &json!({"serviceKey": hosted.service_key,
        "metaServiceKey": hosted.meta_service_key, "hostPid": std::process::id()}),
    );
    let deadline = Instant::now() + Duration::from_secs(600);
    while !root.join("stop").exists() && Instant::now() < deadline {
        if root.join("restart").exists() {
            std::fs::remove_file(root.join("restart")).unwrap();
            serve_acp::restart(name).expect("restart the real hosted agent");
            std::fs::write(root.join("restarted"), b"ready").unwrap();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    serve_acp::stop_all();
    let transport = transport::resolve_blocking().unwrap();
    let withdrawn = runtime::runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let keys = pocket_codex_pb::keys(&transport.session).await.unwrap();
                if !keys.contains(&hosted.service_key) && !keys.contains(&hosted.meta_service_key) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .is_ok()
    });
    assert!(withdrawn, "both owned registrations must disappear from the relay");
    save(root, "cleanup.json", &json!({"relayKeysWithdrawn": true}));
}

fn permission(
    engine: &dyn SessionEngine,
    key: &str,
    tid: &str,
    rx: &mut Receiver<AppEvent>,
    kind: &str,
) {
    let request = event(rx, |ev| {
        ev.thread_id.as_deref() == Some(tid) && ev.kind == "acp/permission/requested"
    });
    let body: Value = serde_json::from_str(&request.raw).unwrap();
    let option = body["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["kind"] == kind)
        .expect("agent advertised permission option");
    engine
        .respond_approval(
            key,
            request.request_id.as_deref().unwrap(),
            option["optionId"].as_str().unwrap(),
        )
        .expect("answer exact agent option through relay");
}

fn controller(root: &Path, name: &str) {
    let _cleanup = Cleanup(root.to_path_buf());
    runtime::init(root.join("controller-support")).unwrap();
    let ready: Value =
        serde_json::from_slice(&std::fs::read(root.join("ready.json")).unwrap()).unwrap();
    assert_ne!(ready["hostPid"], std::process::id());
    assert!(!serve_acp::is_hosting(name), "a separate process must use the relay path");
    let key = ready["serviceKey"].as_str().unwrap();
    let engine = session_engine::engine(key);
    engine
        .connect(key.into(), 0)
        .expect("connect through public relay");
    let caps = engine.capabilities(key);
    assert!(caps.negotiated && caps.permission_options && !caps.auth_required);
    assert_eq!(caps.protocol, "acp");
    let mut rx = engine.subscribe_events(key).unwrap();
    let fixture = root.join("fixture").canonicalize().unwrap();
    assert!(
        !fixture.join("denied.txt").exists() && !fixture.join("accepted.txt").exists(),
        "use a fresh disposable fixture; never overwrite an existing file"
    );
    let tid = engine
        .thread_start(key, StartOptions {
            cwd: Some(fixture.to_string_lossy().into()),
            ..Default::default()
        })
        .expect("create real ACP session");
    save(
        root,
        "session.json",
        &json!({"threadId": tid, "capabilities": format!("{caps:?}"),
        "settings": engine.session_settings(key, &tid)}),
    );

    // Use values the real agent advertised, never a guessed mode/model id.
    let settings = engine.session_settings(key, &tid).unwrap();
    for option in settings["configOptions"].as_array().unwrap() {
        let id = option["id"].as_str().unwrap();
        let original = option["currentValue"].as_str().unwrap();
        let selected = option["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["value"] != original)
            .unwrap_or(&option["options"][0])["value"]
            .as_str()
            .unwrap();
        engine
            .set_session_config(key, &tid, id, selected)
            .expect("real agent config change");
        let changed = engine.session_settings(key, &tid).unwrap();
        assert!(changed["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == id && v["currentValue"] == selected));
        engine
            .set_session_config(key, &tid, id, original)
            .expect("restore real agent config");
    }

    prompt(engine, key, &tid, "What is 17 + 25? Answer with the number; do not use tools.");
    let deltas = completed(&mut rx, &tid, "completed");
    let history = engine.thread_read(key, &tid, true).unwrap();
    save(root, "first-history.json", &serde_json::to_value(&history.items).unwrap());
    assert!(deltas > 0, "reply must arrive as live deltas");
    assert!(history
        .items
        .iter()
        .any(|i| i.item_type == "agentMessage" && i.text.contains("42")));
    assert!(engine.thread_list(key).unwrap().iter().any(|t| t.id == tid));
    let retained: Vec<_> = history
        .items
        .iter()
        .map(|i| (i.id.clone(), i.text.clone()))
        .collect();
    engine.disconnect(key);
    engine
        .connect(key.into(), 0)
        .expect("reconnect through relay");
    engine
        .thread_resume(key, &tid)
        .expect("resume real session");
    let recovered = engine.thread_read(key, &tid, true).unwrap();
    for item in &retained {
        assert!(
            recovered
                .items
                .iter()
                .any(|i| (&i.id, &i.text) == (&item.0, &item.1)),
            "reconnect must retain item ids and content"
        );
    }
    rx = engine.subscribe_events(key).unwrap();

    prompt(
        engine,
        key,
        &tid,
        "Use the edit/write tool to create denied.txt in the current directory containing DENIED. \
         Do not use shell commands. If permission is rejected, stop; do not retry or use another \
         tool.",
    );
    permission(engine, key, &tid, &mut rx, "reject_once");
    completed(&mut rx, &tid, "completed");
    assert!(!fixture.join("denied.txt").exists());

    prompt(
        engine,
        key,
        &tid,
        "Use the edit/write tool to create accepted.txt in the current directory containing \
         exactly ACP_PERMISSION_OK. Do not use shell commands or touch any other files. After \
         writing, reply DONE.",
    );
    permission(engine, key, &tid, &mut rx, "allow_once");
    completed(&mut rx, &tid, "completed");
    assert_eq!(
        std::fs::read_to_string(fixture.join("accepted.txt"))
            .unwrap()
            .trim(),
        "ACP_PERMISSION_OK"
    );
    let preview = meta::file_preview(key, Some(&tid), "accepted.txt")
        .expect("read the authorized session file through the separate meta relay");
    assert_eq!(String::from_utf8(preview.bytes).unwrap().trim(), "ACP_PERMISSION_OK");

    prompt(
        engine,
        key,
        &tid,
        "Without using tools, write the integers from 1 to 2000 in words, one per line.",
    );
    event(&mut rx, |ev| {
        ev.thread_id.as_deref() == Some(tid.as_str()) && ev.kind == "item/agentMessage/delta"
    });
    engine
        .turn_interrupt(key, &tid, None)
        .expect("cancel real model turn through relay");
    completed(&mut rx, &tid, "interrupted");
    assert!(!engine.thread_read(key, &tid, true).unwrap().running);
    engine.disconnect(key);

    std::fs::write(root.join("restart"), b"restart").unwrap();
    let deadline = Instant::now() + WAIT;
    while !root.join("restarted").exists() {
        assert!(Instant::now() < deadline, "real host restart deadline");
        std::thread::sleep(Duration::from_millis(200));
    }
    engine
        .connect(key.into(), 0)
        .expect("connect to restarted agent");
    assert!(engine.capabilities(key).generation > caps.generation);
    assert!(engine.thread_list(key).unwrap().iter().any(|t| t.id == tid));
    engine
        .thread_resume(key, &tid)
        .expect("load persisted real agent session after restart");
    let replay = engine.thread_read(key, &tid, true).unwrap();
    assert!(replay
        .items
        .iter()
        .any(|i| i.item_type == "agentMessage" && i.text.contains("42")));
    rx = engine.subscribe_events(key).unwrap();
    prompt(
        engine,
        key,
        &tid,
        "What is 7 multiplied by 8? Answer with the number; do not use tools.",
    );
    completed(&mut rx, &tid, "completed");
    assert!(engine
        .thread_read(key, &tid, true)
        .unwrap()
        .items
        .iter()
        .any(|i| i.item_type == "agentMessage" && i.text.contains("56")));
    engine.disconnect(key);
    save(
        root,
        "acceptance.json",
        &json!({"status": "passed", "streamDeltas": deltas,
        "separateProcesses": true, "create": true, "inventory": true,
        "reconnectStableHistory": true, "permissionReject": true, "permissionAllow": true,
        "cancel": true, "sessionConfig": true, "metaFilePreview": true,
        "agentRestartLoadReplay": true, "promptAfterRestart": true,
        "sessionId": tid, "serviceKey": key}),
    );
}

#[test]
#[ignore = "requires an explicitly configured real ACP agent, model gateway and test relay"]
fn acp_live_remote_control() {
    let root = PathBuf::from(std::env::var_os("PCX_ACP_LIVE_ROOT").expect("private fixture root"));
    let name = std::env::var("PCX_ACP_LIVE_NAME").expect("unique test service name");
    match std::env::var("PCX_ACP_LIVE_ROLE").as_deref() {
        Ok("host") => host(&root, &name),
        Ok("controller") => controller(&root, &name),
        _ => panic!("set PCX_ACP_LIVE_ROLE to host or controller"),
    }
}

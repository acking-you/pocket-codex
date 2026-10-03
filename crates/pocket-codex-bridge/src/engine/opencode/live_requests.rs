//! Opt-in live check of pending permissions and forms without any model call:
//! the host raises them on a fresh session under `$TMPDIR/pocket-opencode-e2e`
//! (as a tool would) and the controller path answers them.
//!
//! `PCX_OPENCODE_LIVE_SUPPORT=<App support dir> cargo test -p
//! pocket_codex_bridge  opencode_live_requests -- --nocapture --test-threads=1`

use std::time::Duration;

use pocket_codex_host_svc::opencode::discovery;
use serde_json::json;

use super::live_tests::{note, wait};
use crate::engine::{runtime, serve, transport};

#[test]
fn opencode_live_requests() {
    let Some(support) = std::env::var_os("PCX_OPENCODE_LIVE_SUPPORT") else {
        return;
    };
    runtime::init(support.into()).expect("runtime");
    let hosted = crate::engine::serve_opencode::start(Some("opencode".into()), None).expect("host");
    let key = hosted.service_key.clone();
    super::connect(key.clone(), 0, &transport::resolve_blocking().expect("transport"))
        .expect("connect");
    let mut rx = super::subscribe_events(&key).expect("events");
    let e2e = std::env::temp_dir().join("pocket-opencode-e2e");
    std::fs::create_dir_all(&e2e).expect("dir");
    let tid = super::thread_start(&key, None, Some(e2e.display().to_string())).expect("session");
    let host = runtime::runtime()
        .block_on(discovery::discover())
        .expect("host client")
        .client;

    // A permission, declined from the controller. This machine's OpenCode
    // allows everything, so the test session asks for its own probe action.
    let rules = json!([{"action": "pocket-e2e", "resource": "*", "effect": "ask"}]);
    runtime::runtime()
        .block_on(host.set_session_permissions(&tid, rules))
        .expect("session rules");
    let asker = host.clone();
    let session = tid.clone();
    let pending = runtime::runtime().spawn(async move {
        asker
            .request_permission(&session, "pocket-e2e", &["probe"])
            .await
    });
    let asked = wait(&mut rx, Duration::from_secs(30), |e| {
        e.thread_id.as_deref() == Some(tid.as_str()) && e.request_id.is_some()
    });
    note(&format!("6 permission card: {:?}", asked.as_ref().map(|e| (&e.kind, &e.raw))));
    if asked.is_none() {
        let listed = runtime::runtime().block_on(host.permissions(&tid));
        note(&format!("6 host sees pending={:?}", listed.map(|l| l.len())));
        let done = runtime::runtime()
            .block_on(async { tokio::time::timeout(Duration::from_secs(2), pending).await });
        note(&format!("6 raising call: {done:?}"));
        panic!("no permission card");
    }
    let asked = asked.expect("a permission card");
    let id = asked.request_id.clone().unwrap_or_default();
    super::respond_approval(&key, &id, "decline").expect("decline");
    let upstream = runtime::runtime()
        .block_on(async { tokio::time::timeout(Duration::from_secs(30), pending).await });
    note(&format!("6 declined; raising call returned {upstream:?}"));
    let left = runtime::runtime()
        .block_on(host.permissions(&tid))
        .expect("pending permissions");
    note(&format!("6 pending after decline={}", left.len()));
    assert!(left.is_empty());

    // A form answered from the controller (multiselect + boolean).
    let fields = json!([
        {"key": "ok", "type": "boolean", "title": "Proceed?"},
        {"key": "tags", "type": "multiselect", "options": [
            {"value": "a", "label": "Alpha"}, {"value": "b", "label": "Beta"}]}
    ]);
    let form = runtime::runtime()
        .block_on(host.create_form(&tid, "Pocket e2e", fields))
        .expect("form");
    let card = wait(&mut rx, Duration::from_secs(30), |e| {
        e.request_id.as_deref() == Some(form.id.as_str())
    });
    note(&format!("C10 form card: {:?}", card.as_ref().map(|e| &e.raw)));
    // The card may predate the subscription; resume re-announces it.
    if card.is_none() {
        super::thread_resume(&key, &tid).expect("resume");
    }
    let answers = json!({"ok": ["是 / Yes"], "tags": ["Alpha", "Beta"]}).to_string();
    super::respond_user_input(&key, &form.id, &answers).expect("answer");
    let left = runtime::runtime()
        .block_on(host.forms(&tid))
        .expect("forms");
    note(&format!("C10 pending forms after answer={}", left.len()));
    assert!(left.is_empty());
    super::disconnect(&key);
    serve::serve_stop("opencode").expect("stop");
}

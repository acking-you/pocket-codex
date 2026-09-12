//! Manual end-to-end verification harness (`#[ignore]`d — never runs in CI).
//!
//! Drives the REAL stack exactly as the Flutter app does through FRB: external
//! codex host → relay tunnel → `turn/start` with an image
//! attachment → live event stream → `thread/read` history echo. Needs a
//! signed-in account in the app's support dir and spends one real model call.
//!
//! ```text
//! POCKET_CODEX_E2E_IMAGE=path\to\image.png \
//! cargo test -p pocket_codex_bridge e2e_manual -- --ignored --nocapture
//! ```
//!
//! Without `POCKET_CODEX_E2E_IMAGE` a built-in 1×1 PNG is attached (enough for
//! the wire/echo assertions; pass a real image to eyeball the model's
//! description of it).

use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// 1×1 red PNG, pre-encoded.
const TINY_PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn init_and_host(name: &str) -> crate::api::bridge::AppServeDto {
    use crate::api::bridge as api;
    // The app's real support dir (the account login lives there).
    let support = std::env::var("POCKET_CODEX_E2E_SUPPORT").unwrap_or_else(|_| {
        let appdata = std::env::var("APPDATA").expect("APPDATA not set");
        format!("{appdata}\\io.github.acking_you\\pocket_codex")
    });
    api::init_bridge(support).expect("init_bridge");
    // POCKET_CODEX_E2E_PORT: adopt a codex you started yourself on that port
    // (`codex app-server --listen ws://127.0.0.1:<port>`) instead of spawning
    // one — serve_start reuses a live listener on the requested port. Useful
    // when the installed codex shim's spawn behaviour is flaky.
    let port: u16 = std::env::var("POCKET_CODEX_E2E_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let host =
        api::app_serve_start(port, None, Some(name.into()), None, false).expect("app_serve_start");
    println!("hosting: app={} (pid {})", host.app_service_key, host.pid);
    host
}

#[test]
#[ignore = "manual e2e: needs a signed-in account and spends a real model call"]
fn image_turn_round_trips_through_a_real_host() {
    use crate::api::bridge as api;
    // A per-run name: a crashed previous run can leave a hollow registration
    // for the old name on the relay (until lease expiry), and a fresh
    // subscribe may route to it — fresh names sidestep that entirely.
    let name = format!("e2e-img-{}", std::process::id());
    let host = init_and_host(&name);
    let result = std::panic::catch_unwind(|| run_image_turn(&host.app_service_key));
    let _ = api::app_serve_stop(name);
    if let Err(p) = result {
        std::panic::resume_unwind(p);
    }
}

/// The document analogue: upload a file over the META service (loopback here;
/// the same code path a remote controller takes through the relay),
/// reference its HOST path in the turn text, and require the agent to read the
/// file's exact sentinel content back.
///
/// EXTERNAL codex with a `read-only` sandbox — the reference path: reading the
/// file exercises the agent's shell tools inside the OS sandbox.
#[test]
#[ignore = "manual e2e: needs a signed-in account and spends a real model call"]
fn file_turn_round_trips_through_a_real_host() {
    use crate::api::bridge as api;
    let name = format!("e2e-file-{}", std::process::id());
    let host = init_and_host(&name);
    // An external codex may predate thread/settings/updated — don't assert it.
    let result = std::panic::catch_unwind(|| {
        run_file_turn(&host.app_service_key, "read-only", /* expect_settings_update */ false)
    });
    let _ = api::app_serve_stop(name);
    if let Err(p) = result {
        std::panic::resume_unwind(p);
    }
}

/// Connect with retries: an EXTERNAL codex (an npm shim spawning node) can
/// accept TCP before its websocket endpoint finishes booting, so the first
/// handshake may fail where the app's own reconnect logic would retry.
fn connect_with_retry(key: &str) {
    use crate::api::bridge as api;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match api::app_connect(key.to_string(), 0) {
            Ok(()) => return,
            Err(e) if Instant::now() < deadline => {
                println!("connect retry: {e:#}");
                std::thread::sleep(Duration::from_secs(2));
            },
            Err(e) => panic!("app_connect: {e:#}"),
        }
    }
}

fn run_file_turn(key: &str, sandbox: &str, expect_settings_update: bool) {
    use crate::api::bridge as api;

    const SENTINEL: &str = "pocket-codex-e2e-sentinel-7429";
    let content = format!("The magic word is: {SENTINEL}\n");
    let host_path = api::meta_upload_file(
        key.to_string(),
        "sentinel notes.txt".to_string(),
        content.clone().into_bytes(),
    )
    .expect("meta_upload_file");
    println!("uploaded to host path: {host_path}");
    assert!(host_path.ends_with("sentinel notes.txt"), "basename preserved: {host_path}");

    connect_with_retry(key);
    let mut rx = crate::engine::app_session::subscribe_events(key).expect("subscribe_events");

    let cwd = std::env::temp_dir().join("pcx-e2e-file");
    std::fs::create_dir_all(&cwd).expect("temp cwd");
    let tid = api::app_thread_start(
        key.to_string(),
        None,
        Some(cwd.to_string_lossy().into_owned()),
        Some("never".into()),
        Some(sandbox.to_string()),
    )
    .expect("app_thread_start");

    // The thread/start response reports the runtime config the thread actually
    // runs with — the ground truth the UI's active-model indicator shows.
    let runtime = api::app_thread_runtime_config(key.to_string(), tid.clone())
        .expect("runtime config cached from the thread/start response");
    println!(
        "RUNTIME CONFIG: model={:?} provider={:?} effort={:?} approval={:?} sandbox={:?}",
        runtime.model,
        runtime.model_provider,
        runtime.reasoning_effort,
        runtime.approval_policy,
        runtime.sandbox_mode
    );
    assert!(
        runtime.model.as_deref().is_some_and(|m| !m.is_empty()),
        "thread/start must report the effective model"
    );
    assert_eq!(
        runtime.sandbox_mode.as_deref(),
        Some(sandbox),
        "thread/start must echo the effective sandbox mode"
    );

    // The exact text shape the Flutter composer sends (attachment_refs.dart).
    // The explicit effort override exercises the settings round-trip: newer
    // servers apply it and push `thread/settings/updated`, which the engine
    // caches as the CONFIRMED runtime config (asserted after the turn).
    let text = format!(
        "Read the attached file and reply with the exact magic word it contains.\n\n## Attached \
         files (read them from this machine's filesystem):\n- \"{host_path}\""
    );
    api::app_turn_start(
        key.to_string(),
        tid.clone(),
        text,
        Vec::new(),
        None,
        None,
        None,
        None,
        Some("low".into()),
    )
    .expect("app_turn_start");

    let deadline = Instant::now() + Duration::from_secs(300);
    let mut reply = String::new();
    let rt = crate::engine::runtime::runtime();
    loop {
        assert!(Instant::now() < deadline, "turn timed out; reply so far: {reply}");
        let ev =
            rt.block_on(async { tokio::time::timeout(Duration::from_secs(15), rx.recv()).await });
        let Ok(ev) = ev else { continue };
        let ev = ev.expect("event stream closed");
        match ev.kind.as_str() {
            "item/agentMessage/delta" => reply.push_str(ev.text.as_deref().unwrap_or_default()),
            "turn/completed" => break,
            "turn/failed" | "error" => panic!("turn failed: {}", ev.raw),
            k if k.starts_with("item/")
                && ev.item_type.as_deref() == Some("agentMessage")
                && !k.contains("delta") =>
            {
                if let Some(t) = ev.text.as_deref().filter(|t| !t.is_empty()) {
                    reply = t.to_string();
                }
            },
            _ => {},
        }
    }
    println!("AGENT REPLY: {reply}");
    assert!(
        reply.contains(SENTINEL),
        "the agent must have READ the uploaded file to know the sentinel; reply: {reply}"
    );

    // The effort override must be visible as SERVER-confirmed runtime config:
    // the vendored app-server pushes `thread/settings/updated` when a turn's
    // overrides change the thread settings, and the engine's forwarder caches
    // it. (An older external codex may not notify — then this stays a
    // best-effort print above rather than an assertion.)
    let after = api::app_thread_runtime_config(key.to_string(), tid)
        .expect("runtime config still cached after the turn");
    println!(
        "RUNTIME AFTER TURN: model={:?} effort={:?} confirmed_by_update={}",
        after.model, after.reasoning_effort, after.confirmed_by_update
    );
    if expect_settings_update {
        assert!(
            after.confirmed_by_update,
            "the server should push thread/settings/updated for the effort override"
        );
        assert_eq!(
            after.reasoning_effort.as_deref(),
            Some("low"),
            "the confirmed runtime config must carry the overridden effort"
        );
    }
}

fn run_image_turn(key: &str) {
    use crate::api::bridge as api;

    connect_with_retry(key);
    let mut rx = crate::engine::app_session::subscribe_events(key).expect("subscribe_events");

    let image = match std::env::var("POCKET_CODEX_E2E_IMAGE") {
        Ok(path) => {
            let bytes = std::fs::read(&path).expect("reading POCKET_CODEX_E2E_IMAGE");
            format!("data:image/png;base64,{}", BASE64.encode(bytes))
        },
        Err(_) => TINY_PNG_DATA_URL.to_string(),
    };

    let cwd = std::env::temp_dir().join("pcx-e2e-img");
    std::fs::create_dir_all(&cwd).expect("temp cwd");
    let tid = api::app_thread_start(
        key.to_string(),
        None,
        Some(cwd.to_string_lossy().into_owned()),
        Some("never".into()),
        Some("read-only".into()),
    )
    .expect("app_thread_start");
    println!("thread: {tid}");

    let prompt = "Describe this image in one short sentence: name the shapes, their colors, and \
                  any text you can read.";
    api::app_turn_start(
        key.to_string(),
        tid.clone(),
        prompt.to_string(),
        vec![image.clone()],
        None,
        None,
        None,
        None,
        None,
    )
    .expect("app_turn_start with image");

    // Stream events until the turn ends, accumulating the agent's reply.
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut reply = String::new();
    let rt = crate::engine::runtime::runtime();
    loop {
        assert!(Instant::now() < deadline, "turn timed out; reply so far: {reply}");
        let ev =
            rt.block_on(async { tokio::time::timeout(Duration::from_secs(15), rx.recv()).await });
        let Ok(ev) = ev else { continue }; // idle tick — keep waiting
        let ev = ev.expect("event stream closed");
        match ev.kind.as_str() {
            "item/agentMessage/delta" => reply.push_str(ev.text.as_deref().unwrap_or_default()),
            "turn/completed" => break,
            "turn/failed" | "error" => panic!("turn failed: {}", ev.raw),
            k if k.starts_with("item/")
                && ev.item_type.as_deref() == Some("agentMessage")
                && !k.contains("delta") =>
            {
                if let Some(t) = ev.text.as_deref().filter(|t| !t.is_empty()) {
                    reply = t.to_string(); // completed snapshot wins over
                                           // deltas
                }
            },
            _ => {},
        }
    }
    println!("AGENT REPLY: {reply}");
    assert!(!reply.trim().is_empty(), "agent reply should not be empty");

    // History must echo the user message with the image data URL intact — this
    // is what a re-opened conversation (and a second device) renders from.
    let h = api::app_thread_read(key.to_string(), tid).expect("app_thread_read");
    let user = h
        .items
        .iter()
        .find(|i| i.item_type == "userMessage")
        .expect("user message present in history");
    assert_eq!(user.text, prompt, "history echoes the typed text");
    assert_eq!(user.images, vec![image], "history echoes the image data URL verbatim");
    println!("HISTORY OK: userMessage echoed with {} image(s)", user.images.len());
}

//! Opt-in isolated integration against an installed external Codex executable.
//! Set PCX_TEST_CODEX_BINARY to the native binary and run this test with
//! --ignored. All sessions, configuration, indices and listeners are temporary.
//! No model turn is started and no existing host or session is contacted.

use std::{
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{ensure, Context, Result};
use pocket_codex_codex::client::AppClient;
use pocket_codex_core::history_sync::{
    self as wire, HistoryWindow, SyncRequest, SyncResponse, WindowQuery,
};
use pocket_codex_host_svc::history_sync::{router, CodexHistorySource};
use serde_json::json;

#[tokio::test]
#[ignore = "requires PCX_TEST_CODEX_BINARY; creates an isolated external app-server"]
async fn real_codex_history_survives_client_and_meta_restart() -> Result<()> {
    let binary = PathBuf::from(
        std::env::var_os("PCX_TEST_CODEX_BINARY").context("PCX_TEST_CODEX_BINARY required")?,
    );
    let temporary = tempfile::tempdir()?;
    let home = temporary.path().join("codex-home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(
        home.join("config.toml"),
        r#"
model = "fixture-model"
model_provider = "fixture"
[model_providers.fixture]
name = "Isolated history fixture"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
requires_openai_auth = false
"#,
    )?;
    let id = "f24519ab-582d-4bed-a192-1aff303defa3".to_owned();
    let sessions = home.join("sessions/2026/09/25");
    std::fs::create_dir_all(&sessions)?;
    let rollout = sessions.join(format!("rollout-2026-09-25T00-00-00-{id}.jsonl"));
    let mut file = std::fs::File::create(&rollout)?;
    writeln!(
        file,
        "{}",
        line(
            "session_meta",
            json!({"id": id, "session_id": id,
        "timestamp": "2026-09-25T00:00:00Z", "cwd": temporary.path(), "originator": "codex", "cli_version": "0.0.0",
        "source": "cli", "model_provider": "fixture", "history_mode": "legacy", "selected_capability_roots": []})
        )
    )?;
    write_turn(&mut file, "turn-one", &"a".repeat(1_000_000))?;
    file.sync_all()?;
    let reserved = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = reserved.local_addr()?;
    drop(reserved);
    let log = std::fs::File::create(temporary.path().join("app-server.log"))?;
    let mut process = tokio::process::Command::new(binary)
        .args(["app-server", "--listen", &format!("ws://{address}")])
        .env("CODEX_HOME", &home)
        .current_dir(temporary.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let (controller, events) = loop {
        if let Ok(client) = AppClient::connect(&format!("ws://{address}")).await {
            break client;
        }
        ensure!(
            Instant::now() < deadline && process.try_wait()?.is_none(),
            "isolated app-server did not start"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    drop(events);
    controller
        .initialize("pocket-codex-isolated-history-test", true)
        .await?;
    let query = WindowQuery {
        session: id.clone(),
        collection: "items".into(),
        group: None,
        cursor: None,
        limit: 20,
        projection: Some("desc".into()),
    };
    let cache_file = temporary.path().join("controller-cache.json");
    let client = reqwest::Client::new();
    let mut cold_bytes = 0;
    for restart in 0..2 {
        let source = CodexHistorySource::with_history_directory(
            address,
            home.clone(),
            temporary.path().join("source-index"),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/history/v1/window", listener.local_addr()?);
        let server =
            tokio::spawn(async move { axum::serve(listener, router(Arc::new(source))).await });
        let retained: Option<HistoryWindow> = std::fs::read(&cache_file)
            .ok()
            .map(|b| serde_json::from_slice(&b))
            .transpose()?;
        let request = SyncRequest {
            version: wire::VERSION,
            query: query.clone(),
            known: retained.as_ref().map(HistoryWindow::manifest).transpose()?,
        };
        let reply = client.post(&url).json(&request).send().await?;
        let status = reply.status();
        let bytes = reply.bytes().await?;
        ensure!(status.is_success(), "sync failed: {}", String::from_utf8_lossy(&bytes));
        let delta: SyncResponse = serde_json::from_slice(&bytes)?;
        if restart == 0 {
            cold_bytes = bytes.len();
            ensure!(
                cold_bytes > 1_000_000,
                "large fixture missing from initial history: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(512)])
            );
        } else {
            ensure!(
                delta.changes.is_empty() && bytes.len() < 4096,
                "restart retransmitted unchanged history"
            );
            eprintln!(
                "real Codex: cold={cold_bytes} bytes, client+meta restart={} bytes",
                bytes.len()
            );
        }
        let retained = wire::apply(retained.as_ref(), &delta)?;
        std::fs::write(&cache_file, serde_json::to_vec(&retained)?)?;
        if restart == 1 {
            let mut file = std::fs::OpenOptions::new().append(true).open(&rollout)?;
            write_turn(&mut file, "turn-two", "New isolated item")?;
            file.sync_all()?;
            let reply = client
                .post(&url)
                .json(&SyncRequest {
                    version: wire::VERSION,
                    query: query.clone(),
                    known: Some(retained.manifest()?),
                })
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let delta: SyncResponse = serde_json::from_slice(&reply)?;
            ensure!(
                !delta.changes.is_empty() && reply.len() < 4096,
                "new item retransmitted old large message"
            );
            let updated = wire::apply(Some(&retained), &delta)?;
            ensure!(
                serde_json::to_string(&updated)?.contains("New isolated item"),
                "new item missing"
            );
            eprintln!("real Codex: new-item delta={} bytes", reply.len());
        }
        server.abort();
        let _ = server.await;
    }
    drop(controller);
    process.kill().await?;
    process.wait().await?;
    Ok(())
}

fn line(kind: &str, payload: serde_json::Value) -> serde_json::Value {
    json!({"timestamp": "2026-09-25T00:00:00Z", "type": kind, "payload": payload})
}

fn write_turn(file: &mut std::fs::File, turn: &str, text: &str) -> Result<()> {
    for value in [
        line(
            "event_msg",
            json!({"type": "task_started", "turn_id": turn, "model_context_window": 200000}),
        ),
        line(
            "response_item",
            json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Isolated fixture"}]}),
        ),
        line(
            "event_msg",
            json!({"type": "user_message", "message": "Isolated fixture", "images": [], "local_images": [], "text_elements": []}),
        ),
        line(
            "response_item",
            json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}),
        ),
        line("event_msg", json!({"type": "agent_message", "message": text})),
        line(
            "event_msg",
            json!({"type": "task_complete", "turn_id": turn, "last_agent_message": text}),
        ),
    ] {
        writeln!(file, "{value}")?;
    }
    Ok(())
}

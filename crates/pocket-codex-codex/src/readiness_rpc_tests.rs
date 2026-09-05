use futures::{SinkExt, StreamExt};
use pocket_codex_core::state::CodexProcessInfo;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::*;

async fn adopted_server(answer_list: bool, wildcard: bool) -> Result<(), StartupFailure> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake server");
    let addr = listener.local_addr().expect("listen address");
    let peer = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.expect("HTTP connection");
        let mut request = [0u8; 512];
        let received = http.read(&mut request).await.expect("read readyz");
        assert!(received > 0);
        http.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("readyz response");
        drop(http);
        let mut ws = accept_async(listener.accept().await.expect("RPC connection").0)
            .await
            .expect("websocket handshake");
        for method in ["initialize", "thread/list"] {
            let text = ws
                .next()
                .await
                .expect("RPC frame")
                .expect("valid frame")
                .into_text()
                .expect("text frame");
            let request: Value = serde_json::from_str(&text).expect("JSON request");
            assert_eq!(request["method"], method);
            if method == "thread/list" && !answer_list {
                std::future::pending::<()>().await;
            }
            ws.send(Message::text(
                json!({"id": request["id"], "result": {"data": []}}).to_string(),
            ))
            .await
            .expect("RPC response");
        }
        ws
    });
    let report = SpawnReport {
        info: CodexProcessInfo {
            pid: std::process::id(),
            listen: if wildcard {
                format!("ws://0.0.0.0:{}", addr.port())
            } else {
                format!("ws://{addr}")
            },
            log_file: PathBuf::from("no-child-log"),
            started_at: String::new(),
        },
        reused: true,
        log_offset: 0,
        listener_confirmed: true,
    };
    let budget = if answer_list { Duration::from_secs(3) } else { Duration::from_millis(200) };
    let result = tokio::task::spawn_blocking(move || verify_ready(&report, budget))
        .await
        .expect("readiness task");
    if answer_list {
        peer.await.expect("fake server completed");
    } else {
        peer.abort();
    }
    result
}

#[tokio::test]
async fn adopted_listener_must_answer_a_real_thread_request() {
    let failure = adopted_server(false, false)
        .await
        .expect_err("wedged listener must not be adopted");
    assert!(failure.to_string().contains("thread/list timed out"));
    assert!(!failure.process_exited);
}

#[tokio::test]
async fn functional_adopted_listener_is_preserved() {
    adopted_server(true, false)
        .await
        .expect("healthy listener should be reused");
}

#[tokio::test]
async fn adopted_wildcard_listener_is_probed_over_loopback() {
    adopted_server(true, true)
        .await
        .expect("wildcard listener should be reused");
}

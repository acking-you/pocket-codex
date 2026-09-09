use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, WebSocketStream};

use super::*;

async fn connection() -> (AppClient, mpsc::UnboundedReceiver<Inbound>, WebSocketStream<TcpStream>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test operation");
    let url = format!("ws://{}", listener.local_addr().expect("test operation"));
    let (client, server) = tokio::join!(AppClient::connect(&url), async {
        accept_async(listener.accept().await.expect("test operation").0)
            .await
            .expect("test operation")
    });
    let (client, inbound) = client.expect("test operation");
    (client, inbound, server)
}

#[tokio::test]
async fn peer_close_marks_dead_and_rejects_future_requests() {
    let (client, mut inbound, mut server) = connection().await;
    server.close(None).await.expect("test operation");
    assert!(tokio::time::timeout(Duration::from_secs(1), inbound.recv())
        .await
        .expect("test operation")
        .is_none());
    assert!(!client.is_alive(), "a closed reader must never advertise a healthy socket");
    let error = client
        .request("thread/list", json!({}))
        .await
        .expect_err("operation must fail");
    assert!(error.to_string().contains("connection closed"), "{error:#}");
}

#[tokio::test]
async fn cancelling_a_request_removes_its_pending_entry() {
    let (client, _inbound, mut server) = connection().await;
    let client = Arc::new(client);
    let requesting = Arc::clone(&client);
    let task = tokio::spawn(async move { requesting.request("thread/list", json!({})).await });
    server
        .next()
        .await
        .expect("test operation")
        .expect("test operation");
    task.abort();
    assert!(task.await.expect_err("operation must fail").is_cancelled());
    assert!(
        client.pending.lock().expect("test operation").is_empty(),
        "cancelled probes must not leak in-flight requests"
    );
    assert!(client.is_alive(), "caller cancellation alone does not prove the socket died");
}

#[tokio::test]
async fn slow_rpc_does_not_disconnect_other_requests_or_late_replies() {
    let (client, _inbound, mut server) = connection().await;
    let peer = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let frame = server
                .next()
                .await
                .expect("frame")
                .expect("read")
                .into_text()
                .expect("text");
            requests.push(serde_json::from_str::<Value>(&frame).expect("json"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        for request in requests.into_iter().rev() {
            server
                .send(WsMessage::text(
                    json!({"id": request["id"], "result": {"ok": true}}).to_string(),
                ))
                .await
                .expect("reply");
        }
        let frame = server
            .next()
            .await
            .expect("frame")
            .expect("read")
            .into_text()
            .expect("text");
        let request: Value = serde_json::from_str(&frame).expect("json");
        server
            .send(WsMessage::text(json!({"id": request["id"], "result": {"ok": true}}).to_string()))
            .await
            .expect("reply");
        server
    });
    let (slow, other) = tokio::join!(
        client.request_inner("thread/resume", Some(json!({})), Duration::from_millis(30)),
        client.request("thread/read", json!({"threadId": "other"})),
    );
    let error = slow.expect_err("slow handler").to_string();
    assert!(error.contains("timed out"), "{error}");
    assert!(!error.contains("connection closed"), "{error}");
    assert_eq!(other.expect("other request survives")["ok"], true);
    assert!(client.is_alive());
    assert_eq!(
        client
            .request("thread/list", json!({}))
            .await
            .expect("still usable")["ok"],
        true
    );
    assert!(client.pending.lock().expect("pending").is_empty());
    let _server = peer.await.expect("peer");
}

#[tokio::test]
async fn large_history_frame_and_repeated_switches_keep_the_same_connection() {
    let (client, _inbound, mut server) = connection().await;
    let peer = tokio::spawn(async move {
        for i in 0..100 {
            let frame = server
                .next()
                .await
                .expect("frame")
                .expect("read")
                .into_text()
                .expect("text");
            let request: Value = serde_json::from_str(&frame).expect("json");
            let text = if i == 0 { "x".repeat(17 << 20) } else { i.to_string() };
            server
                .send(WsMessage::text(
                    json!({"id": request["id"], "result": {"text": text}}).to_string(),
                ))
                .await
                .expect("reply");
        }
        server
    });
    for i in 0..100 {
        let reply = client
            .request("thread/read", json!({"threadId": format!("t{}", i % 5)}))
            .await
            .expect("history");
        assert_eq!(
            reply["text"].as_str().expect("text").len(),
            if i == 0 { 17 << 20 } else { i.to_string().len() }
        );
    }
    let _server = peer.await.expect("peer");
    assert!(client.is_alive());
}

#[tokio::test]
async fn websocket_protocol_failure_preserves_the_underlying_reason() {
    use tokio::io::AsyncWriteExt;
    let (client, mut inbound, mut server) = connection().await;
    // An unmasked, final reserved opcode is invalid regardless of payload.
    server
        .get_mut()
        .write_all(&[0x83, 0x00])
        .await
        .expect("invalid frame");
    assert!(inbound.recv().await.is_none());
    let error = client
        .request("thread/list", json!({}))
        .await
        .expect_err("invalid protocol")
        .to_string();
    assert!(error.contains("websocket read failed"), "{error}");
    assert!(error.contains("invalid opcode"), "{error}");
}

#[tokio::test]
async fn request_deadline_includes_waiting_for_the_writer() {
    let (client, _inbound, _server) = connection().await;
    let _blocked_writer = client.sink.lock().await;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        client.request_inner("thread/list", Some(json!({})), Duration::from_millis(50)),
    )
    .await
    .expect("test operation");
    assert!(result
        .expect_err("operation must fail")
        .to_string()
        .contains("timed out"));
    assert!(!client.is_alive());
    assert!(client.pending.lock().expect("test operation").is_empty());
}

#[tokio::test]
async fn rpc_errors_preserve_a_working_connection() {
    let (client, _inbound, mut server) = connection().await;
    let peer = tokio::spawn(async move {
        for result in [
            json!({"error": {"code": -32602, "message": "invalid thread"}}),
            json!({"result": {"data": []}}),
        ] {
            let frame = server
                .next()
                .await
                .expect("test operation")
                .expect("test operation")
                .into_text()
                .expect("test operation");
            let request: Value = serde_json::from_str(&frame).expect("test operation");
            let mut response = result;
            response["id"] = request["id"].clone();
            server
                .send(WsMessage::text(response.to_string()))
                .await
                .expect("test operation");
        }
        server
    });
    assert!(client
        .request("thread/read", json!({}))
        .await
        .expect_err("operation must fail")
        .to_string()
        .contains("invalid thread"));
    assert!(client.is_alive());
    assert_eq!(
        client
            .request("thread/list", json!({}))
            .await
            .expect("test operation")["data"],
        json!([])
    );
    let _server = peer.await.expect("test operation");
    assert!(client.is_alive());
}

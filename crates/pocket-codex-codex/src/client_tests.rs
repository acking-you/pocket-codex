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
async fn rpc_timeout_closes_even_a_peer_that_keeps_sending_pongs() {
    let (client, mut inbound, mut server) = connection().await;
    let peer = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            tokio::select! {
                frame = server.next() => match frame {
                    Some(Ok(WsMessage::Text(_))) | Some(Ok(WsMessage::Ping(_))) => {},
                    _ => break,
                },
                _ = tick.tick() => {
                    if server.send(WsMessage::Pong(Vec::new().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (timed_out, other) = tokio::join!(
        client.request_inner("thread/list", Some(json!({})), Duration::from_millis(100)),
        client.request("thread/read", json!({"threadId": "other"})),
    );
    assert!(timed_out
        .expect_err("operation must fail")
        .to_string()
        .contains("timed out"));
    assert!(other
        .expect_err("operation must fail")
        .to_string()
        .contains("connection closed"));
    assert!(!client.is_alive());
    assert!(tokio::time::timeout(Duration::from_secs(1), inbound.recv())
        .await
        .expect("test operation")
        .is_none());
    assert!(client.pending.lock().expect("test operation").is_empty());
    peer.abort();
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

use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};

use super::*;

fn load_window(next_cursor: Value) -> LoadedHistory {
    runtime::init(std::env::temp_dir()).expect("pagination test operation");
    let turn = json!({"id": "turn-1", "status": "completed", "items": [
        {"id": "user-1", "type": "userMessage", "content": [
            {"type": "text", "text": "First part"},
            {"type": "text", "text": "Second part"}
        ]},
        {"id": "agent-1", "type": "agentMessage", "text": "Answer"}
    ]});
    let replies = [
        ("thread/turns/list", json!({"data": [turn.clone()], "nextCursor": null})),
        (
            "thread/items/list",
            json!({"data": [{"turnId": "turn-1", "item": turn["items"][1]}], "nextCursor": next_cursor}),
        ),
        ("thread/turns/list", json!({"data": [turn], "nextCursor": null})),
    ];
    let (client, peer) = mock_client(replies.into());
    let history = load_paginated_window(&client, "pagination-test", "thread-1")
        .expect("pagination test operation");
    runtime::runtime()
        .block_on(peer)
        .expect("pagination test operation");
    history
}

fn mock_client(
    replies: Vec<(&'static str, Value)>,
) -> (Arc<AppClient>, tokio::task::JoinHandle<WebSocketStream<TcpStream>>) {
    runtime::runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("pagination test operation");
        let url = format!("ws://{}", listener.local_addr().expect("pagination test operation"));
        let peer = tokio::spawn(async move {
            let mut socket = accept_async(
                listener
                    .accept()
                    .await
                    .expect("pagination test operation")
                    .0,
            )
            .await
            .expect("pagination test operation");
            for (method, result) in replies {
                let frame = socket
                    .next()
                    .await
                    .expect("pagination test operation")
                    .expect("pagination test operation")
                    .into_text()
                    .expect("pagination test operation");
                let request: Value =
                    serde_json::from_str(&frame).expect("pagination test operation");
                assert_eq!(request["method"], method);
                socket
                    .send(Message::text(json!({"id": request["id"], "result": result}).to_string()))
                    .await
                    .expect("pagination test operation");
            }
            socket
        });
        let (client, _) = AppClient::connect(&url)
            .await
            .expect("pagination test operation");
        (Arc::new(client), peer)
    })
}

#[test]
fn a_single_long_turn_keeps_its_older_item_pages_reachable() {
    let history = load_window(json!("older-items"));
    assert_eq!(history.skeletons.len(), 1);
    assert!(history.has_older, "the latest turn can span multiple item pages");
}

#[test]
fn exhausted_item_cursor_ends_pagination() {
    assert!(!load_window(Value::Null).has_older);
}

#[test]
fn turn_summary_reads_the_upstream_user_content_array() {
    let history = load_window(Value::Null);
    assert_eq!(history.skeletons[0].user_text, "First part\nSecond part");
    assert_eq!(history.skeletons[0].assistant_text, "Answer");
}

struct TestSession(String);

impl TestSession {
    fn new(client: Arc<AppClient>) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let key =
            format!("history-test-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        sessions()
            .lock()
            .expect("test sessions")
            .insert(key.clone(), Session {
                client,
                events: broadcast::channel(16).0,
                forwarder: runtime::runtime().spawn(std::future::pending()),
                active_turns: Default::default(),
                runtime_config: Default::default(),
                pending_approvals: Default::default(),
                transcript: Default::default(),
                pagination: Default::default(),
            });
        Self(key)
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        disconnect(&self.0);
    }
}

#[test]
fn paginated_items_keep_timing_beyond_status_shells_and_on_later_reads() {
    runtime::init(std::env::temp_dir()).expect("init test runtime");
    let turns: Vec<Value> = (1..=8)
        .rev()
        .map(|id| {
            json!({
                "id": format!("t{id}"), "status": "completed",
                "completedAt": 1000 + id, "durationMs": id * 100, "items": []
            })
        })
        .collect();
    let entry = |id| {
        json!({"turnId": format!("t{id}"), "item": {
            "id": format!("a{id}"), "type": "agentMessage", "text": "answer"
        }})
    };
    let (client, peer) = mock_client(vec![
        ("thread/turns/list", json!({"data": turns[..5], "nextCursor": "older-shells"})),
        ("thread/items/list", json!({"data": [entry(8), entry(3)], "nextCursor": "older-items"})),
        ("thread/turns/list", json!({"data": turns, "nextCursor": null})),
        ("thread/items/list", json!({"data": [entry(2)], "nextCursor": null})),
        ("thread/items/list", json!({"data": [entry(1)], "nextCursor": null})),
    ]);
    let session = TestSession::new(client.clone());
    let history = load_paginated_window(&client, &session.0, "thread").expect("load window");
    assert_eq!(history.items[0].turn_completed_at, Some(1003));
    assert_eq!(history.items[0].turn_duration_ms, Some(300));
    assert!(!history.skeletons[2].loaded, "an agent tail is not a navigable user row");
    let older = thread_older_page(&session.0, "thread").expect("older page");
    assert_eq!(older.items[0].turn_completed_at, Some(1002));
    assert_eq!(older.items[0].turn_duration_ms, Some(200));
    let turn = thread_turn_items(&session.0, "thread", "t1").expect("turn items");
    assert_eq!(turn[0].turn_completed_at, Some(1001));
    assert_eq!(turn[0].turn_duration_ms, Some(100));
    runtime::runtime().block_on(peer).expect("test peer");
}

#[test]
fn pending_summary_yields_on_a_single_async_worker() {
    use std::time::{Duration, Instant};
    runtime::init(std::env::temp_dir()).expect("init test runtime");
    let received = Arc::new(tokio::sync::Notify::new());
    let notify = received.clone();
    let (client, peer) = runtime::runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("address"));
        let peer = tokio::spawn(async move {
            let mut ws = accept_async(listener.accept().await.expect("accept").0)
                .await
                .expect("ws");
            let first: Value = serde_json::from_str(
                &ws.next()
                    .await
                    .expect("summary")
                    .expect("frame")
                    .into_text()
                    .expect("text"),
            )
            .expect("json");
            assert_eq!(first["method"], "thread/turns/list");
            notify.notify_one();
            // Bound the peer so a blocking regression fails instead of hanging.
            if let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_secs(3), ws.next()).await
            {
                let next: Value =
                    serde_json::from_str(&frame.into_text().expect("text")).expect("json");
                assert_eq!(next["method"], "thread/list");
                ws.send(Message::text(
                    json!({"id": next["id"], "result": {"data": []}}).to_string(),
                ))
                .await
                .expect("interactive reply");
            }
            ws.send(Message::text(json!({"id": first["id"], "result": {"data": []}}).to_string()))
                .await
                .expect("summary reply");
            ws
        });
        (Arc::new(AppClient::connect(&url).await.expect("client").0), peer)
    });
    let session = TestSession::new(client.clone());
    let single = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("single worker");
    single.block_on(async {
        let summary = tokio::spawn(crate::api::bridge::app_thread_summary(
            session.0.clone(),
            "thread".into(),
        ));
        // Wait until the blocking summary is actually in flight.
        tokio::time::timeout(Duration::from_secs(1), received.notified())
            .await
            .expect("summary started");
        let started = Instant::now();
        client
            .request("thread/list", json!({"limit": 1}))
            .await
            .expect("interactive request");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(summary.await.expect("summary task").expect("summary"), None);
    });
    runtime::runtime().block_on(peer).expect("test peer");
}

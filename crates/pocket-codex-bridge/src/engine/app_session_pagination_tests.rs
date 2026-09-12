use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    accept_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
    WebSocketStream,
};

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
    let session = TestSession::new(client.clone());
    let history =
        load_paginated_window(&client, &session.0, "thread-1").expect("pagination test operation");
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
            let mut socket = accept_async_with_config(
                listener
                    .accept()
                    .await
                    .expect("pagination test operation")
                    .0,
                Some(WebSocketConfig::default()),
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
                if method == "thread/resume" {
                    assert_eq!(
                        request["params"]["excludeTurns"], true,
                        "resume must not hydrate full history"
                    );
                }
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
            let mut ws = accept_async_with_config(
                listener.accept().await.expect("accept").0,
                Some(WebSocketConfig::default()),
            )
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

#[test]
fn resume_requests_metadata_and_retains_the_runtime_configuration() {
    runtime::init(std::env::temp_dir()).expect("init runtime");
    let (client, peer) = mock_client(vec![(
        "thread/resume",
        json!({
            "thread": {"id": "thread"}, "model": "test-model", "reasoningEffort": "high"
        }),
    )]);
    let session = TestSession::new(client);
    thread_resume(&session.0, "thread").expect("resume");
    assert_eq!(
        thread_runtime_config(&session.0, "thread")
            .expect("config")
            .model
            .as_deref(),
        Some("test-model")
    );
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
#[ignore = "manual: PCX_SOAK_WS and PCX_SOAK_THREAD_IDS select existing idle threads"]
fn real_session_switch_soak() {
    use std::time::Instant;
    runtime::init(std::env::temp_dir()).expect("init runtime");
    let addr = std::env::var("PCX_SOAK_WS").expect("PCX_SOAK_WS host:port");
    let threads = std::env::var("PCX_SOAK_THREAD_IDS").expect("comma-separated idle thread ids");
    let threads: Vec<&str> = threads.split(',').collect();
    let rounds = std::env::var("PCX_SOAK_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let session = TestSession("session-switch-soak".into());
    establish(session.0.clone(), &addr).expect("connect");
    let mut times = Vec::new();
    for i in 0..rounds {
        let thread = threads[i % threads.len()];
        let start = Instant::now();
        thread_resume(&session.0, thread).expect("resume");
        let history = thread_read(&session.0, thread).expect("history");
        assert!(!history.items.is_empty(), "fixture should include a transcript");
        assert!(is_connected(&session.0), "same connection survives every switch");
        times.push(start.elapsed());
    }
    times.sort();
    eprintln!(
        "{rounds} switches, p50={:?}, p95={:?}, p99={:?}, max={:?}",
        times[rounds / 2],
        times[rounds * 95 / 100],
        times[rounds * 99 / 100],
        times.last().expect("samples")
    );
}

#[test]
fn reopening_an_idle_thread_reuses_pages_and_the_exhausted_cursor() {
    runtime::init(std::env::temp_dir()).expect("init");
    let metadata = json!({"thread": {"id": "cached", "historyMode": "paginated", "updatedAt": 1}});
    let turn = json!({"id": "t1", "status": "completed", "items": []});
    let entry = |id: &str| {
        json!({"turnId": "t1", "item": {
            "id": id, "type": "agentMessage", "text": id
        }})
    };
    let (client, peer) = mock_client(vec![
        ("thread/read", metadata.clone()),
        ("thread/turns/list", json!({"data": [turn.clone()], "nextCursor": null})),
        ("thread/items/list", json!({"data": [entry("new")], "nextCursor": "older"})),
        ("thread/turns/list", json!({"data": [turn], "nextCursor": null})),
        ("thread/items/list", json!({"data": [entry("old")], "nextCursor": null})),
        // No turns/items requests follow this metadata check.
        ("thread/read", metadata),
    ]);
    let session = TestSession::new(client);
    assert!(
        thread_read(&session.0, "cached")
            .expect("first read")
            .has_older
    );
    assert!(
        !thread_older_page(&session.0, "cached")
            .expect("older")
            .has_older
    );
    let restored = thread_read(&session.0, "cached").expect("cached read");
    assert!(!restored.has_older);
    assert_eq!(
        restored
            .items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        ["old", "new"]
    );
    assert!(thread_older_page(&session.0, "cached")
        .expect("end")
        .items
        .is_empty());
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
fn empty_older_page_ends_pagination_even_with_a_bogus_cursor() {
    runtime::init(std::env::temp_dir()).expect("init");
    let (client, peer) =
        mock_client(vec![("thread/items/list", json!({"data": [], "nextCursor": "phantom"}))]);
    let session = TestSession::new(client);
    set_pagination(&session.0, "empty", ThreadPagination {
        next_item_cursor: Some("older".into()),
        ..Default::default()
    });
    assert!(
        !thread_older_page(&session.0, "empty")
            .expect("empty page")
            .has_older
    );
    assert!(
        !thread_older_page(&session.0, "empty")
            .expect("no second request")
            .has_older
    );
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
fn repeated_older_cursor_is_never_requested_twice() {
    runtime::init(std::env::temp_dir()).expect("init");
    let (client, peer) = mock_client(vec![(
        "thread/items/list",
        json!({"data": [{"turnId": "t1", "item": {
            "id": "old", "type": "agentMessage", "text": "old"
        }}], "nextCursor": "older"}),
    )]);
    let session = TestSession::new(client);
    set_pagination(&session.0, "repeat", ThreadPagination {
        next_item_cursor: Some("older".into()),
        ..Default::default()
    });
    assert!(
        !thread_older_page(&session.0, "repeat")
            .expect("page")
            .has_older
    );
    assert!(thread_older_page(&session.0, "repeat")
        .expect("end")
        .items
        .is_empty());
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
fn new_tail_preserves_the_cached_prefix_and_exhausted_cursor() {
    runtime::init(std::env::temp_dir()).expect("init");
    let turn = json!({"id": "t1", "status": "completed", "items": []});
    let entry =
        |id: &str| json!({"turnId": "t1", "item": {"id": id, "type": "agentMessage", "text": id}});
    let (client, peer) = mock_client(vec![
        (
            "thread/read",
            json!({"thread": {"id": "cached", "historyMode": "paginated", "updatedAt": 1}}),
        ),
        ("thread/turns/list", json!({"data": [turn.clone()]})),
        ("thread/items/list", json!({"data": [entry("tail")], "nextCursor": "older"})),
        ("thread/turns/list", json!({"data": [turn.clone()]})),
        ("thread/items/list", json!({"data": [entry("prefix")], "nextCursor": null})),
        (
            "thread/read",
            json!({"thread": {"id": "cached", "historyMode": "paginated", "updatedAt": 2}}),
        ),
        ("thread/turns/list", json!({"data": [turn.clone()]})),
        (
            "thread/items/list",
            json!({"data": [entry("new"), entry("tail")], "nextCursor": "older-again"}),
        ),
        ("thread/turns/list", json!({"data": [turn]})),
    ]);
    let session = TestSession::new(client);
    thread_read(&session.0, "cached").expect("first");
    thread_older_page(&session.0, "cached").expect("prefix");
    let refreshed = thread_read(&session.0, "cached").expect("refresh");
    assert_eq!(
        refreshed
            .items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        ["prefix", "tail", "new"]
    );
    assert!(!refreshed.has_older);
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
fn an_inflight_read_cannot_undo_live_cache_invalidation() {
    runtime::init(std::env::temp_dir()).expect("init");
    let (client, peer) = mock_client(vec![]);
    let session = TestSession::new(client);
    let mut pending = ensure_pagination(&session.0, "thread");
    pending.metadata = Some(json!({"updatedAt": 1}));
    let pages = sessions().lock().expect("sessions")[&session.0]
        .pagination
        .clone();
    let event = |method: &str| Inbound {
        method: method.into(),
        params: Some(json!({"threadId": "thread"})),
        request_id: None,
    };
    invalidate_history(&pages, &event("item/completed"));
    assert!(set_pagination(&session.0, "thread", pending.clone()));
    let current = pagination_of(&session.0, "thread").expect("state");
    assert!(current.metadata.is_none());
    assert_eq!(current.source_revision, 1);

    invalidate_history(&pages, &event("thread/compacted"));
    assert!(!set_pagination(&session.0, "thread", pending));
    assert_eq!(
        pagination_of(&session.0, "thread")
            .expect("state")
            .generation,
        1
    );
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
fn selected_turn_pages_are_cached_and_continue_past_three_pages() {
    runtime::init(std::env::temp_dir()).expect("init");
    let replies = (0..4).map(|i| (
        "thread/items/list",
        json!({"data": [{"turnId": "turn", "item": {"id": format!("item-{i}"), "type": "agentMessage", "text": "text"}}],
               "nextCursor": if i < 3 { Some(format!("cursor-{i}")) } else { None }}),
    )).collect();
    let (client, peer) = mock_client(replies);
    let session = TestSession::new(client);
    let first = thread_turn_page(&session.0, "thread", "turn", false).expect("first page");
    assert_eq!(first.items.len(), 1);
    assert!(first.has_more);
    assert_eq!(
        thread_turn_page(&session.0, "thread", "turn", false)
            .expect("cached")
            .items
            .len(),
        1
    );
    for count in 2..=4 {
        let page = thread_turn_page(&session.0, "thread", "turn", true).expect("next");
        assert_eq!(page.items.len(), count);
        assert_eq!(page.has_more, count < 4);
    }
    let done = thread_turn_page(&session.0, "thread", "turn", true).expect("no more requests");
    assert!(!done.has_more);
    assert_eq!(done.items.len(), 4);
    runtime::runtime().block_on(peer).expect("peer");
}

#[test]
#[ignore = "manual: PCX_HISTORY_WS and PCX_HISTORY_THREAD_ID select an existing long idle thread"]
fn real_history_gap_smoke() {
    runtime::init(std::env::temp_dir()).expect("init");
    let addr = std::env::var("PCX_HISTORY_WS").expect("app-server address");
    let thread = std::env::var("PCX_HISTORY_THREAD_ID").expect("thread id");
    let session = TestSession("real-history-gap-smoke".into());
    establish(session.0.clone(), &addr).expect("connect");
    let start = Instant::now();
    let history = thread_read(&session.0, &thread).expect("tail");
    let cold = start.elapsed();
    let first = history.first_turn_id.expect("enumerated beginning");
    let start = Instant::now();
    let page = thread_turn_page(&session.0, &thread, &first, false).expect("opening turn");
    let jump = start.elapsed();
    assert!(!page.items.is_empty());
    assert!(page
        .items
        .iter()
        .any(|item| item.item_type == "userMessage"));
    let start = Instant::now();
    let reopened = thread_read(&session.0, &thread).expect("reopen");
    let reused = reopened
        .turn_pages
        .iter()
        .find(|page| page.turn_id == first)
        .expect("cached selected turn");
    assert_eq!(reused.items.len(), page.items.len());
    eprintln!(
        "cold={cold:?}, first turn={jump:?}, reopen={:?}; turns={}, first-turn items={}, more={}",
        start.elapsed(),
        reopened.turns.len(),
        page.items.len(),
        page.has_more
    );
}

#[test]
fn evicted_turn_continues_without_advertising_a_suffix_as_its_opening() {
    runtime::init(std::env::temp_dir()).expect("init");
    let entry = |id: &str, next: Option<&str>| {
        (
            "thread/items/list",
            json!({"data": [{"turnId": "turn", "item": {"id": id, "type": "agentMessage", "text": id}}], "nextCursor": next}),
        )
    };
    let (client, peer) = mock_client(vec![
        entry("first", Some("next")),
        entry("second", None),
        entry("first", Some("next")),
    ]);
    let session = TestSession::new(client);
    thread_turn_page(&session.0, "thread", "turn", false).expect("opening");
    let mut state = pagination_of(&session.0, "thread").expect("state");
    let window = state.turn_pages.get_mut("turn").expect("window");
    window.items = Arc::default();
    window.evicted = true;
    set_pagination(&session.0, "thread", state);
    let suffix = thread_turn_page(&session.0, "thread", "turn", true).expect("continue");
    assert_eq!(suffix.items[0].id, "second");
    assert!(!suffix.has_more);
    assert!(cached_turn_pages(&session.0, "thread").is_empty());
    let opening = thread_turn_page(&session.0, "thread", "turn", false).expect("reopen");
    assert_eq!(opening.items[0].id, "first");
    assert!(opening.has_more);
    runtime::runtime().block_on(peer).expect("peer");
}

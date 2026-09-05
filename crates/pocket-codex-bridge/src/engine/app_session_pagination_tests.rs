use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

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
    let (client, peer) = runtime::runtime().block_on(async {
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
    });
    let history = load_paginated_window(&client, "pagination-test", "thread-1")
        .expect("pagination test operation");
    runtime::runtime()
        .block_on(peer)
        .expect("pagination test operation");
    history
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

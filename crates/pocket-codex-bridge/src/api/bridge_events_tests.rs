use tokio::sync::broadcast;

use super::forward_app_events;
use crate::engine::app_session::AppEvent;

fn event(kind: &str) -> AppEvent {
    AppEvent {
        kind: kind.into(),
        thread_id: Some("thread".into()),
        item_id: None,
        item_type: None,
        title: None,
        text: None,
        images: Vec::new(),
        request_id: None,
        raw: "{}".into(),
    }
}

#[tokio::test]
async fn forwards_items_and_completion_in_order() {
    let (tx, rx) = broadcast::channel(4);
    for kind in ["item/completed", "turn/completed"] {
        tx.send(event(kind)).unwrap();
    }
    drop(tx);
    let mut received = Vec::new();
    forward_app_events(rx, |event| {
        received.push(event.kind);
        true
    })
    .await;
    assert_eq!(received, ["item/completed", "turn/completed"]);
}

#[tokio::test]
async fn a_gap_closes_the_feed_even_when_the_connection_is_alive() {
    let (tx, rx) = broadcast::channel(1);
    tx.send(event("item/completed")).unwrap();
    tx.send(event("turn/completed")).unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        forward_app_events(rx, |event| {
            received.push(event.kind);
            true
        }),
    )
    .await
    .expect("a gap must close the feed without waiting for the socket to close");
    assert!(received.is_empty(), "do not deliver a misleading partial tail");
    assert_eq!(tx.receiver_count(), 0);

    // Re-subscribing uses the same connection after history recovery.
    let mut resumed = tx.subscribe();
    tx.send(event("turn/started")).unwrap();
    assert_eq!(resumed.recv().await.unwrap().kind, "turn/started");
}

#[tokio::test]
async fn a_dropped_dart_listener_stops_forwarding() {
    let (tx, rx) = broadcast::channel(4);
    tx.send(event("item/completed")).unwrap();
    tx.send(event("turn/completed")).unwrap();
    let mut received = Vec::new();
    forward_app_events(rx, |event| {
        received.push(event.kind);
        false
    })
    .await;
    assert_eq!(received, ["item/completed"]);
    assert_eq!(tx.receiver_count(), 0);
}

#[tokio::test]
async fn a_gap_preserves_retained_approvals_and_questions_before_closing() {
    let (tx, rx) = broadcast::channel(4);
    tx.send(event("lost-item")).unwrap();
    for (kind, id) in [
        ("item/commandExecution/requestApproval", "approval-1"),
        ("item/tool/requestUserInput", "question-1"),
    ] {
        let mut prompt = event(kind);
        prompt.request_id = Some(id.into());
        tx.send(prompt).unwrap();
    }
    tx.send(event("item/completed")).unwrap();
    tx.send(event("turn/completed")).unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        forward_app_events(rx, |event| {
            received.push((event.kind, event.request_id));
            true
        }),
    )
    .await
    .expect("close after preserving prompts");
    assert_eq!(received, vec![
        ("item/commandExecution/requestApproval".into(), Some("approval-1".into())),
        ("item/tool/requestUserInput".into(), Some("question-1".into())),
    ]);
    assert_eq!(tx.receiver_count(), 0);
}

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

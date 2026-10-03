//! [`AppEvent`] builders shared by the engines that translate a foreign
//! protocol into the app-server event shape (OpenCode and ACP, T15).

use serde_json::{json, Value};

use crate::engine::app_session::{AppEvent, ThreadItem};

/// A thread-level event.
pub(crate) fn event(kind: &str, thread: &str, raw: Value) -> AppEvent {
    AppEvent {
        kind: kind.to_string(),
        thread_id: Some(thread.to_string()),
        item_id: None,
        item_type: None,
        title: None,
        text: None,
        images: Vec::new(),
        request_id: None,
        raw: raw.to_string(),
    }
}

/// An item event carrying `item`'s id, type, title and images.
pub(crate) fn item_event(
    kind: &str,
    thread: &str,
    item: &ThreadItem,
    text: Option<String>,
) -> AppEvent {
    let raw = json!({"threadId": thread, "itemId": item.id, "item": {"id": item.id, "type": item.item_type}});
    AppEvent {
        kind: kind.to_string(),
        thread_id: Some(thread.to_string()),
        item_id: Some(item.id.clone()),
        item_type: Some(item.item_type.clone()),
        title: Some(item.title.clone()),
        text,
        images: item.images.clone(),
        request_id: None,
        raw: raw.to_string(),
    }
}

/// An item with only an id and a type.
pub(crate) fn bare_item(id: String, item_type: &str) -> ThreadItem {
    ThreadItem {
        id,
        item_type: item_type.to_string(),
        title: String::new(),
        text: String::new(),
        questions_json: None,
        images: Vec::new(),
        turn_id: String::new(),
        turn_completed_at: None,
        turn_duration_ms: None,
    }
}

/// An event not tied to a thread (hub-level elicitation, session list or hub
/// state changes).
pub(crate) fn hub_event(kind: &str, raw: Value) -> AppEvent {
    AppEvent {
        kind: kind.to_string(),
        thread_id: None,
        item_id: None,
        item_type: None,
        title: None,
        text: None,
        images: Vec::new(),
        request_id: None,
        raw: raw.to_string(),
    }
}

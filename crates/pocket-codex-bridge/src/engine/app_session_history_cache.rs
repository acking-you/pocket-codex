//! Bounded, session-local history snapshots; cursors remain server-owned.

use super::*;

const MAX_CACHED_THREADS: usize = 8;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;

type Pages = HashMap<String, ThreadPagination>;

fn pages_for(service_key: &str) -> Option<Arc<Mutex<Pages>>> {
    sessions()
        .lock()
        .ok()?
        .get(service_key)
        .map(|s| Arc::clone(&s.pagination))
}

pub(super) fn pagination_of(service_key: &str, thread_id: &str) -> Option<ThreadPagination> {
    pages_for(service_key)?.lock().ok()?.get(thread_id).cloned()
}

pub(super) fn ensure_pagination(service_key: &str, thread_id: &str) -> ThreadPagination {
    let Some(pages) = pages_for(service_key) else { return ThreadPagination::default() };
    let Ok(mut pages) = pages.lock() else { return ThreadPagination::default() };
    pages.entry(thread_id.to_string()).or_default().clone()
}

/// Commit only to the generation that issued the request. Live invalidations
/// never wait on a network request's gate.
pub(super) fn set_pagination(
    service_key: &str,
    thread_id: &str,
    mut state: ThreadPagination,
) -> bool {
    let Some(pages) = pages_for(service_key) else { return false };
    let Ok(mut pages) = pages.lock() else { return false };
    if pages
        .get(thread_id)
        .is_some_and(|p| p.generation != state.generation)
    {
        return false;
    }
    if let Some(current) = pages.get(thread_id) {
        if current.source_revision != state.source_revision {
            state.source_revision = current.source_revision;
            state.metadata = None;
            state.turn_pages = current.turn_pages.clone();
        }
    }
    pages.insert(thread_id.to_string(), state);
    trim_cache(&mut pages);
    true
}

pub(super) fn reset_pagination(service_key: &str, thread_id: &str) {
    let Some(pages) = pages_for(service_key) else { return };
    let Ok(mut pages) = pages.lock() else { return };
    // Keep the gate alive for reads already queued for this same thread.
    if let Some(state) = pages.get_mut(thread_id) {
        *state = ThreadPagination {
            request_gate: Arc::clone(&state.request_gate),
            generation: state.generation,
            source_revision: state.source_revision,
            ..Default::default()
        };
    }
}

pub(super) fn cache_history(
    service_key: &str,
    thread_id: &str,
    source_revision: u64,
    metadata: Option<Value>,
    history: &LoadedHistory,
) {
    if history
        .turns
        .last()
        .and_then(|t| t.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status == "inProgress" || status == "in_progress")
    {
        return;
    }
    let Some(mut state) = pagination_of(service_key, thread_id) else { return };
    if state.source_revision != source_revision {
        return;
    }
    if metadata
        .as_ref()
        .and_then(|thread| thread.get("updatedAt"))
        .is_none()
    {
        return;
    }
    state.metadata = metadata;
    state.cached = Some(Arc::new(history.clone()));
    state.cached_at = Some(Instant::now());
    set_pagination(service_key, thread_id, state);
}

pub(super) fn replaces_history(inbound: &Inbound) -> bool {
    matches!(
        inbound.method.as_str(),
        "thread/compacted" | "thread/reverted" | "thread/archived" | "thread/deleted"
    ) || (inbound.method == "item/completed"
        && inbound
            .params
            .as_ref()
            .and_then(|params| params.get("item"))
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("contextCompaction"))
}

pub(super) fn invalidate_history(pages: &Mutex<Pages>, inbound: &Inbound) {
    let replaces_history = replaces_history(inbound);
    if !replaces_history
        && !matches!(
            inbound.method.as_str(),
            "turn/started" | "turn/completed" | "turn/failed" | "item/completed"
        )
    {
        return;
    }
    let Some(thread) = inbound
        .params
        .as_ref()
        .and_then(|p| p.get("threadId"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let Ok(mut pages) = pages.lock() else { return };
    if let Some(state) = pages.get_mut(thread) {
        state.source_revision = state.source_revision.wrapping_add(1);
        state.metadata = None;
        if let Some(turn_id) = inbound
            .params
            .as_ref()
            .and_then(|params| {
                params
                    .get("turnId")
                    .or_else(|| params.get("turn").and_then(|turn| turn.get("id")))
            })
            .and_then(Value::as_str)
        {
            state.turn_pages.remove(turn_id);
        }
        if replaces_history {
            *state = ThreadPagination {
                request_gate: Arc::clone(&state.request_gate),
                generation: state.generation.wrapping_add(1),
                source_revision: state.source_revision,
                ..Default::default()
            };
        }
    }
}

/// Reject any in-flight read that started before a source generation change.
pub(super) fn replace_history(service: &str, thread: &str) {
    let Some(pages) = pages_for(service) else { return };
    let Ok(mut pages) = pages.lock() else { return };
    let state = pages.entry(thread.to_owned()).or_default();
    *state = ThreadPagination {
        request_gate: Arc::clone(&state.request_gate),
        generation: state.generation.wrapping_add(1),
        source_revision: state.source_revision.wrapping_add(1),
        ..Default::default()
    };
}

fn items_bytes(items: &[ThreadItem]) -> usize {
    items
        .iter()
        .map(|item| {
            std::mem::size_of::<ThreadItem>()
                + item.id.len()
                + item.turn_id.len()
                + item.text.len()
                + item.title.len()
                + item.images.iter().map(String::len).sum::<usize>()
        })
        .sum::<usize>()
}

fn snapshot_bytes(history: &LoadedHistory) -> usize {
    items_bytes(&history.items)
        + history
            .skeletons
            .iter()
            .map(|turn| {
                std::mem::size_of::<TurnSummary>()
                    + turn.turn_id.len()
                    + turn.user_text.len()
                    + turn.assistant_text.len()
            })
            .sum::<usize>()
}

fn trim_cache(pages: &mut Pages) {
    let mut snapshots: Vec<_> = pages
        .iter()
        .filter(|(_, state)| {
            state.cached.is_some() || state.turn_pages.values().any(|window| !window.evicted)
        })
        .map(|(id, state)| {
            let bytes = state
                .cached
                .as_ref()
                .map_or(0, |history| snapshot_bytes(history))
                + state
                    .turn_pages
                    .values()
                    .map(|window| items_bytes(&window.items))
                    .sum::<usize>();
            (id.clone(), state.cached_at, bytes)
        })
        .collect();
    snapshots.sort_by_key(|(_, used, _)| *used);
    let mut bytes: usize = snapshots.iter().map(|(_, _, size)| size).sum();
    let mut count = snapshots.len();
    for (id, _, size) in snapshots {
        if count <= MAX_CACHED_THREADS && bytes <= MAX_CACHE_BYTES {
            break;
        }
        if let Some(state) = pages.get_mut(&id) {
            state.cached = None;
            state.metadata = None;
            for window in state.turn_pages.values_mut() {
                window.items = Arc::default();
                window.evicted = true;
            }
        }
        bytes = bytes.saturating_sub(size);
        count -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Arc<LoadedHistory> {
        Arc::new(LoadedHistory {
            items: vec![],
            turns: vec![],
            skeletons: vec![],
            has_older: false,
        })
    }

    #[test]
    fn cache_evicts_old_snapshots_without_losing_their_cursor() {
        let mut pages = Pages::new();
        for i in 0..10 {
            pages.insert(i.to_string(), ThreadPagination {
                cached: Some(snapshot()),
                cached_at: Some(Instant::now()),
                next_item_cursor: Some(format!("cursor-{i}")),
                ..Default::default()
            });
        }
        trim_cache(&mut pages);
        assert_eq!(
            pages
                .values()
                .filter(|state| state.cached.is_some())
                .count(),
            MAX_CACHED_THREADS
        );
        assert!(pages["0"].cached.is_none());
        assert_eq!(pages["0"].next_item_cursor.as_deref(), Some("cursor-0"));
    }

    #[test]
    fn live_updates_invalidate_reuse_but_keep_the_loaded_prefix() {
        let pages = Mutex::new(Pages::from([("thread".into(), ThreadPagination {
            cached: Some(snapshot()),
            metadata: Some(json!({"updatedAt": 1})),
            next_item_cursor: Some("older".into()),
            ..Default::default()
        })]));
        invalidate_history(&pages, &Inbound {
            method: "item/completed".into(),
            params: Some(json!({"threadId": "thread"})),
            request_id: None,
        });
        let pages = pages.lock().expect("test pages");
        let state = &pages["thread"];
        assert_eq!(state.source_revision, 1);
        assert_eq!(state.generation, 0);
        assert!(state.metadata.is_none());
        assert!(state.cached.is_some());
        assert_eq!(state.next_item_cursor.as_deref(), Some("older"));
    }

    #[test]
    fn destructive_updates_discard_snapshots_and_cursors_but_preserve_the_gate() {
        for method in [
            "thread/compacted",
            "thread/reverted",
            "thread/archived",
            "thread/deleted",
            "item/completed",
        ] {
            let original = ThreadPagination {
                cached: Some(snapshot()),
                metadata: Some(json!({"updatedAt": 1})),
                next_item_cursor: Some("older".into()),
                ..Default::default()
            };
            let gate = Arc::clone(&original.request_gate);
            let pages = Mutex::new(Pages::from([("thread".into(), original)]));
            invalidate_history(&pages, &Inbound {
                method: method.into(),
                params: Some(json!({"threadId": "thread", "item": {"type": "contextCompaction"}})),
                request_id: None,
            });
            let pages = pages.lock().expect("test pages");
            let state = &pages["thread"];
            assert_eq!(state.generation, 1, "{method}");
            assert!(state.cached.is_none(), "{method}");
            assert!(state.metadata.is_none(), "{method}");
            assert!(state.next_item_cursor.is_none(), "{method}");
            assert!(Arc::ptr_eq(&gate, &state.request_gate), "{method}");
        }
    }
}

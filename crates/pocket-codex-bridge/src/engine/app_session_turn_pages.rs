//! Ascending, bounded pages for gaps reached through timeline navigation.

use super::*;

#[derive(Clone, Debug, Default)]
pub(super) struct TurnWindow {
    pub(super) items: Arc<Vec<ThreadItem>>,
    next_cursor: Option<String>,
    seen: HashSet<String>,
    pub(super) evicted: bool,
}

/// A selected turn's items and whether more follow them in that turn.
#[derive(Clone, Debug)]
pub struct TurnItemsPage {
    /// Turn owning this page.
    pub turn_id: String,
    /// Items in chronological order.
    pub items: Vec<ThreadItem>,
    /// Whether the same turn has another ascending page.
    pub has_more: bool,
}

pub(super) fn cached_turn_pages(service_key: &str, thread_id: &str) -> Vec<TurnItemsPage> {
    pagination_of(service_key, thread_id)
        .map(|state| {
            state
                .turn_pages
                .into_iter()
                .filter(|(_, page)| !page.evicted)
                .map(|(turn_id, page)| TurnItemsPage {
                    turn_id,
                    items: page.items.as_ref().clone(),
                    has_more: page.next_cursor.is_some(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read the opening page of a selected turn or explicitly advance its cursor.
/// Reopening a loaded turn returns its cached window without a network read.
/// After cache eviction, continuation returns only the new page; callers merge
/// by item ID. A fresh opening read always starts at the beginning of the turn.
pub fn thread_turn_page(
    service_key: &str,
    thread_id: &str,
    turn_id: &str,
    load_more: bool,
) -> Result<TurnItemsPage> {
    let client = client_for(service_key)?;
    let gate = ensure_pagination(service_key, thread_id).request_gate;
    let _request = gate
        .lock()
        .map_err(|_| anyhow!("history request lock poisoned"))?;
    let mut state = ensure_pagination(service_key, thread_id);
    if let Some(page) = state.turn_pages.get(turn_id) {
        if (!load_more && !page.evicted) || (load_more && page.next_cursor.is_none()) {
            return Ok(TurnItemsPage {
                turn_id: turn_id.into(),
                items: page.items.as_ref().clone(),
                has_more: page.next_cursor.is_some(),
            });
        }
    }
    if !load_more {
        state.turn_pages.remove(turn_id);
    }
    let window = state.turn_pages.entry(turn_id.into()).or_default();
    let mut params = json!({"threadId": thread_id, "turnId": turn_id, "limit": ITEM_PAGE_LIMIT, "sortDirection": "asc"});
    if let Some(cursor) = &window.next_cursor {
        params["cursor"] = json!(cursor);
    }
    let response = runtime::runtime().block_on(client.request("thread/items/list", params))?;
    let entries = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("history page missing data"))?;
    let stamp = turn_stamp(&state.turn_stamps, turn_id);
    let items: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("item")
                .and_then(|item| parse_turn_item(item, &stamp))
        })
        .collect();
    let next = response
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::to_string);
    window.next_cursor = if entries.is_empty() {
        None
    } else {
        advancing_cursor(window.next_cursor.as_deref(), next, &mut window.seen)
    };
    let mut known: HashSet<_> = window.items.iter().map(|item| item.id.clone()).collect();
    Arc::make_mut(&mut window.items).extend(
        items
            .into_iter()
            .filter(|item| known.insert(item.id.clone())),
    );
    let result = TurnItemsPage {
        turn_id: turn_id.into(),
        items: window.items.as_ref().clone(),
        has_more: window.next_cursor.is_some(),
    };
    if window.evicted {
        // A suffix alone must never be advertised as a cached opening window.
        window.items = Arc::default();
    }
    state.cached_at = Some(Instant::now());
    if !set_pagination(service_key, thread_id, state) {
        bail!("history changed while loading; reopen the thread");
    }
    Ok(result)
}

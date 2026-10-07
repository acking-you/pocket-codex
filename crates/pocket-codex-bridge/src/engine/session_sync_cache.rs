//! Reassemble retained sequential windows without a network read.
//!
//! saved view + cursor -> cached page -> cached page -> missing page
//!       |                    same generation               |
//!       +---------- oldest-first display prefix -----------+
//!
//! A fresh server tail must overlap that prefix before it becomes live. A
//! missing/evicted page keeps its cursor; it never becomes an end-of-history.

use std::collections::HashSet;

use super::{
    super::session_cache::{DiskCache, MAX_ENTRY_BYTES},
    *,
};

#[derive(Serialize, Deserialize)]
struct LiveView {
    generation: Option<String>,
    items: Vec<ThreadItem>,
}

// Server read -> full display snapshot -----+-> cached opening
// Stream      -> bounded live checkpoint --+
// Streaming never reads or rewrites the full display snapshot.
pub(super) fn write_live(
    cache: &DiskCache,
    owner: &str,
    session: &str,
    generation: Option<String>,
    items: &[ThreadItem],
    running: bool,
) -> Result<()> {
    let mut live = cache
        .read_json::<LiveView>(owner, session, "live")?
        .filter(|live| live.generation == generation)
        .unwrap_or_else(|| LiveView {
            generation,
            items: Vec::new(),
        });
    merge_items(&mut live.items, items.iter().cloned());
    if live.items.len() > 100 {
        live.items.drain(..live.items.len() - 100);
    }
    for item in &mut live.items {
        item.questions_json = None;
    }
    cache.write_json(owner, session, "live", &live, running)?;
    Ok(())
}

fn merge_items(items: &mut Vec<ThreadItem>, incoming: impl Iterator<Item = ThreadItem>) {
    let mut positions: HashMap<_, _> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.clone(), index))
        .collect();
    for item in incoming {
        if let Some(index) = positions.get(&item.id) {
            items[*index] = item;
        } else {
            positions.insert(item.id.clone(), items.len());
            items.push(item);
        }
    }
}

fn item_bytes(item: &ThreadItem) -> usize {
    std::mem::size_of::<ThreadItem>()
        + item.id.len()
        + item.item_type.len()
        + item.title.len()
        + item.text.len()
        + item.turn_id.len()
        + item.questions_json.as_ref().map_or(0, String::len)
        + item.images.iter().map(String::len).sum::<usize>()
}

fn read_page(
    cache: &DiskCache,
    owner: &str,
    session: &str,
    generation: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Option<Value>> {
    let query = query_for(
        "thread/items/list",
        &json!({
            "threadId": session, "sortDirection": "desc", "cursor": cursor, "limit": limit,
        }),
    )?;
    read_query(cache, owner, generation, &query)
}

pub(super) fn read_query(
    cache: &DiskCache,
    owner: &str,
    generation: &str,
    query: &WindowQuery,
) -> Result<Option<Value>> {
    let Some(window) = cache.read_json::<HistoryWindow>(owner, &query.session, &key(query)?)?
    else {
        return Ok(None);
    };
    if window.generation != generation
        || window.provider != "codex/app-server-v2"
        || window.validate().is_err()
    {
        return Ok(None);
    }
    Ok(Some(pocket_codex_host_svc::history_sync::codex_response(&window, query)?))
}

pub(in super::super) struct RetainedTurn {
    pub(in super::super) page: super::super::app_session::TurnItemsPage,
    pub(in super::super) cursor: Option<String>,
}

fn read_turns(
    cache: &DiskCache,
    owner: &str,
    session: &str,
    generation: &str,
    turns: &[super::super::app_session::TurnSummary],
    sequential: &[ThreadItem],
) -> Result<Vec<RetainedTurn>> {
    let mut bytes = sequential.iter().map(item_bytes).sum::<usize>();
    let complete: HashSet<_> = sequential
        .iter()
        .filter(|item| item.item_type == "userMessage")
        .map(|item| item.turn_id.as_str())
        .collect();
    let mut retained = Vec::new();
    for turn in turns {
        if complete.contains(turn.turn_id.as_str()) {
            continue;
        }
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        let mut known = HashSet::new();
        let mut found = false;
        loop {
            let query = query_for(
                "thread/items/list",
                &json!({
                    "threadId": session, "turnId": turn.turn_id,
                    "sortDirection": "asc", "limit": 100, "cursor": cursor,
                }),
            )?;
            let Some(response) = read_query(cache, owner, generation, &query)? else { break };
            let next = response["nextCursor"].as_str().map(str::to_owned);
            if next.as_ref().is_some_and(|next| seen.contains(next)) {
                break;
            }
            let mut page = super::super::app_session::parse_prefetched_items(&response);
            page.reverse(); // This source is ascending; the tail parser reverses it.
            page.retain(|item| known.insert(item.id.clone()));
            let size: usize = page.iter().map(item_bytes).sum();
            if bytes.saturating_add(size) > MAX_ENTRY_BYTES as usize {
                break;
            }
            found = true;
            bytes += size;
            items.extend(page);
            cursor = next;
            if let Some(next) = &cursor {
                seen.insert(next.clone());
            } else {
                break;
            }
        }
        if found {
            retained.push(RetainedTurn {
                page: super::super::app_session::TurnItemsPage {
                    turn_id: turn.turn_id.clone(),
                    items,
                    has_more: cursor.is_some(),
                },
                cursor,
            });
        }
        if bytes >= MAX_ENTRY_BYTES as usize {
            break;
        }
    }
    Ok(retained)
}

fn extend(
    cache: &DiskCache,
    owner: &str,
    session: &str,
    generation: &str,
    items: &mut Vec<ThreadItem>,
    cursor: &mut Option<String>,
) -> Result<()> {
    let mut known: HashSet<_> = items.iter().map(|item| item.id.clone()).collect();
    let mut bytes: usize = items.iter().map(item_bytes).sum();
    let mut seen = HashSet::new();
    let mut chunks = Vec::new();
    let mut continuation = cursor.clone();
    while let Some(current) = continuation.as_deref() {
        if !seen.insert(current.to_string()) {
            break;
        }
        let Some(page) = read_page(cache, owner, session, generation, Some(current), 100)? else {
            break;
        };
        let next = page["nextCursor"].as_str().map(str::to_owned);
        if next.as_ref().is_some_and(|next| seen.contains(next)) {
            break;
        }
        let mut chunk = super::super::app_session::parse_prefetched_items(&page);
        chunk.retain(|item| known.insert(item.id.clone()));
        let size: usize = chunk.iter().map(item_bytes).sum();
        // Preserve the existing in-memory history budget even if a session's
        // disk windows occupy hundreds of MiB. No remote prefetch is triggered.
        if bytes.saturating_add(size) > MAX_ENTRY_BYTES as usize {
            break;
        }
        bytes += size;
        chunks.push(chunk);
        continuation = next;
    }
    if !chunks.is_empty() {
        let mut prefix: Vec<_> = chunks.into_iter().rev().flatten().collect();
        prefix.append(items);
        *items = prefix;
    }
    *cursor = continuation;
    Ok(())
}

pub(super) fn read_view(
    cache: &DiskCache,
    owner: &str,
    session: &str,
) -> Result<Option<CachedView>> {
    read_view_inner(cache, owner, session, true)
}

fn read_view_inner(
    cache: &DiskCache,
    owner: &str,
    session: &str,
    include_turns: bool,
) -> Result<Option<CachedView>> {
    let saved = cache.read_json::<CachedView>(owner, session, "view")?;
    let live = cache.read_json::<LiveView>(owner, session, "live")?;
    let Some(mut view) = saved.or_else(|| {
        live.as_ref().map(|live| CachedView {
            generation: live.generation.clone(),
            history: ThreadHistory {
                has_older: true,
                ..Default::default()
            },
            continuation: None,
        })
    }) else {
        return Ok(None);
    };
    if let Some(live) = live
        .as_ref()
        .filter(|live| live.generation == view.generation)
    {
        merge_items(&mut view.history.items, live.items.iter().cloned());
    }
    let Some(generation) = view.generation.as_deref() else { return Ok(Some(view)) };
    if view.continuation.is_none() {
        // Older app versions saved a 100-item display tail without its cursor.
        // Recover its prefix from the actual retained wire chain, if it joins.
        if let Some(page) = read_page(cache, owner, session, generation, None, 20)? {
            let mut items = super::super::app_session::parse_prefetched_items(&page);
            let mut cursor = page["nextCursor"].as_str().map(str::to_owned);
            extend(cache, owner, session, generation, &mut items, &mut cursor)?;
            if stitch(&mut view.history.items, &mut items) {
                view.continuation = Some(CachedContinuation {
                    cursor,
                });
            }
        }
    } else if let Some(continuation) = &mut view.continuation {
        extend(
            cache,
            owner,
            session,
            generation,
            &mut view.history.items,
            &mut continuation.cursor,
        )?;
    }
    if let Some(continuation) = &view.continuation {
        view.history.has_older = continuation.cursor.is_some();
    }
    // Foreground prefetch updates only this small wire tail. Join it only when
    // its overlap proves that no unseen messages lie between the two windows.
    if let Some(page) = read_page(cache, owner, session, generation, None, 20)? {
        let mut tail = super::super::app_session::parse_prefetched_items(&page);
        if stitch(&mut tail, &mut view.history.items) {
            view.history.items = tail;
        }
    }
    if let Some(live) = live.filter(|live| live.generation == view.generation) {
        merge_items(&mut view.history.items, live.items.into_iter());
    }
    if include_turns {
        view.history.turn_pages = read_turns(
            cache,
            owner,
            session,
            generation,
            &view.history.turns,
            &view.history.items,
        )?
        .into_iter()
        .map(|turn| turn.page)
        .collect();
    }
    Ok(Some(view))
}

// Tail contents win; only the proven prefix before the shared item is reused.
fn stitch(tail: &mut Vec<ThreadItem>, cached: &mut Vec<ThreadItem>) -> bool {
    let Some(first) = tail.first() else { return false };
    let Some(at) = cached.iter().position(|item| item.id == first.id) else { return false };
    cached.truncate(at);
    cached.append(tail);
    *tail = std::mem::take(cached);
    true
}

pub(in super::super) fn restore_cached_prefix(
    service: &str,
    session: &str,
    items: &mut Vec<ThreadItem>,
    cursor: &mut Option<String>,
    turns: &[super::super::app_session::TurnSummary],
) -> Result<Vec<RetainedTurn>> {
    let Some(generation) = source_generation(service, session) else { return Ok(Vec::new()) };
    let owner = namespace(service)?;
    let session_gate = gate(&owner, session);
    let _request = session_gate
        .lock
        .lock()
        .map_err(|_| anyhow::anyhow!("history gate poisoned"))?;
    let cache = application_cache()?;
    if let Some(mut view) = read_view_inner(&cache, &owner, session, false)? {
        if view.generation.as_deref() == Some(&generation) {
            if let Some(continuation) = view.continuation {
                if stitch(items, &mut view.history.items) {
                    *cursor = continuation.cursor;
                }
            }
        }
    }
    // A first open can have raw cached pages even when its display snapshot
    // was evicted. Start at the fresh tail's cursor and stop at the first miss.
    extend(&cache, &owner, session, &generation, items, cursor)?;
    read_turns(&cache, &owner, session, &generation, turns, items)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn item(index: usize) -> ThreadItem {
        super::super::super::app_session::parse_prefetched_items(&json!({
            "data": [{"turnId": "turn", "item": {
                "id": index.to_string(), "type": "agentMessage", "text": format!("message {index}")
            }}]
        }))
        .remove(0)
    }

    fn page(
        cache: &DiskCache,
        cursor: Option<&str>,
        range: std::ops::Range<usize>,
        next: Option<&str>,
        generation: &str,
    ) {
        let query = query_for(
            "thread/items/list",
            &json!({
                "threadId": "session", "sortDirection": "desc", "cursor": cursor,
                "limit": if cursor.is_none() {20} else {100},
            }),
        )
        .unwrap();
        let entries: Vec<_> = range.rev().map(|index| {
            let id = format!("turn:{index}");
            (id, json!({"turnId": "turn", "item": {
                "id": index.to_string(), "type": "agentMessage", "text": format!("message {index}")
            }}))
        }).collect();
        let window = HistoryWindow {
            provider: "codex/app-server-v2".into(),
            generation: generation.into(),
            metadata: json!({"nextCursor": next}),
            order: entries.iter().map(|(id, _)| id.clone()).collect(),
            documents: entries.into_iter().collect::<BTreeMap<_, _>>(),
        };
        assert!(cache
            .write_json("owner", "session", &key(&query).unwrap(), &window, false)
            .unwrap());
    }

    fn snapshot(
        cache: &DiskCache,
        range: std::ops::Range<usize>,
        continuation: Option<CachedContinuation>,
    ) {
        cache
            .write_json(
                "owner",
                "session",
                "view",
                &CachedView {
                    generation: Some("one".into()),
                    history: ThreadHistory {
                        items: range.map(item).collect(),
                        has_older: true,
                        ..Default::default()
                    },
                    continuation,
                },
                false,
            )
            .unwrap();
    }

    #[test]
    fn restores_all_retained_pages_in_order_and_keeps_evicted_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        snapshot(
            &cache,
            200..220,
            Some(CachedContinuation {
                cursor: Some("200".into()),
            }),
        );
        page(&cache, Some("200"), 100..200, Some("100"), "one");
        let partial = read_view(&cache, "owner", "session").unwrap().unwrap();
        assert_eq!(partial.history.items.len(), 120);
        assert_eq!(partial.continuation.unwrap().cursor.as_deref(), Some("100"));
        assert!(partial.history.has_older);
        page(&cache, Some("100"), 0..100, None, "one");
        let complete = read_view(&cache, "owner", "session").unwrap().unwrap();
        assert_eq!(
            complete
                .history
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
            (0..220).map(|i| i.to_string()).collect::<Vec<_>>()
        );
        assert!(!complete.history.has_older);
    }

    #[test]
    fn old_display_snapshots_recover_the_retained_chain() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        snapshot(&cache, 120..220, None);
        page(&cache, None, 200..220, Some("200"), "one");
        page(&cache, Some("200"), 100..200, Some("100"), "one");
        page(&cache, Some("100"), 0..100, None, "one");
        let view = read_view(&cache, "owner", "session").unwrap().unwrap();
        assert_eq!(view.history.items.len(), 220);
        assert!(!view.history.has_older);
    }

    #[test]
    fn replacement_and_cyclic_pages_do_not_consume_the_missing_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        snapshot(
            &cache,
            200..220,
            Some(CachedContinuation {
                cursor: Some("200".into()),
            }),
        );
        for (generation, next) in [("two", None), ("one", Some("200"))] {
            page(&cache, Some("200"), 100..200, next, generation);
            let view = read_view(&cache, "owner", "session").unwrap().unwrap();
            assert_eq!(view.history.items.len(), 20);
            assert_eq!(view.continuation.unwrap().cursor.as_deref(), Some("200"));
        }
    }

    #[test]
    fn fresh_tail_wins_and_requires_overlap_before_reusing_prefix() {
        let mut tail = vec![item(2), item(3)];
        tail[0].text = "fresh".into();
        let mut prefix = vec![item(0), item(1), item(2)];
        assert!(stitch(&mut tail, &mut prefix));
        assert_eq!(tail.len(), 4);
        assert_eq!(tail[2].text, "fresh");
        assert!(!stitch(&mut vec![item(10)], &mut tail));
    }

    #[test]
    fn retained_turn_windows_restore_ascending_and_preserve_a_missing_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        let turns = vec![super::super::super::app_session::TurnSummary {
            turn_id: "turn".into(),
            user_text: "question".into(),
            assistant_text: String::new(),
            loaded: false,
        }];
        for (cursor, range, next) in [(None, 0..100, Some("100")), (Some("100"), 100..150, None)] {
            let query = query_for("thread/items/list", &json!({
                "threadId": "session", "turnId": "turn", "sortDirection": "asc", "limit": 100, "cursor": cursor,
            })).unwrap();
            let documents: BTreeMap<_, _> = range.clone().map(|i| (format!("turn:{i}"),
                json!({"turnId": "turn", "item": {"id": i.to_string(), "type": "agentMessage", "text": "cached"}}))).collect();
            let window = HistoryWindow {
                provider: "codex/app-server-v2".into(),
                generation: "one".into(),
                metadata: json!({"nextCursor": next}),
                order: range.map(|i| format!("turn:{i}")).collect(),
                documents,
            };
            cache
                .write_json("owner", "session", &key(&query).unwrap(), &window, false)
                .unwrap();
            let restored = read_turns(&cache, "owner", "session", "one", &turns, &[]).unwrap();
            assert_eq!(restored.len(), 1);
            let page = &restored[0].page;
            let count = if cursor.is_none() { 100 } else { 150 };
            assert_eq!(
                page.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
                (0..count).map(|i| i.to_string()).collect::<Vec<_>>()
            );
            assert_eq!(page.has_more, cursor.is_none());
            assert_eq!(restored[0].cursor.as_deref(), next);
        }
        assert!(read_turns(&cache, "owner", "session", "replaced", &turns, &[])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_retained_older_page_answers_its_exact_query_for_its_generation_only() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        page(&cache, Some("200"), 100..200, Some("100"), "one");
        let query = |cursor: &str| {
            query_for(
                "thread/items/list",
                &json!({
                    "threadId": "session", "sortDirection": "desc", "cursor": cursor, "limit": 100,
                }),
            )
            .unwrap()
        };
        let hit = read_query(&cache, "owner", "one", &query("200"))
            .unwrap()
            .unwrap();
        assert_eq!(hit["data"].as_array().unwrap().len(), 100);
        assert_eq!(hit["nextCursor"], "100");
        // Another cursor, or a source rewritten since, is a miss: the caller
        // then reads through the network.
        assert!(read_query(&cache, "owner", "one", &query("100"))
            .unwrap()
            .is_none());
        assert!(read_query(&cache, "owner", "two", &query("200"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn live_updates_are_bounded_and_do_not_rewrite_the_full_view() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().into(), 1_000_000);
        snapshot(
            &cache,
            0..500,
            Some(CachedContinuation {
                cursor: None,
            }),
        );
        // A background refresh must neither truncate the long view nor undo
        // newer stream checkpoints when the cached display is assembled.
        page(&cache, None, 480..500, Some("480"), "one");
        assert!(cache.contains("owner", "session", "view"));
        let before = cache.read("owner", "session", "view").unwrap();
        let mut updates: Vec<_> = (450..600).map(item).collect();
        updates.last_mut().unwrap().text = "streamed".into();
        write_live(&cache, "owner", "session", Some("one".into()), &updates, true).unwrap();
        assert_eq!(cache.read("owner", "session", "view").unwrap(), before);
        let live = cache
            .read_json::<LiveView>("owner", "session", "live")
            .unwrap()
            .unwrap();
        assert_eq!(live.items.len(), 100);
        let view = read_view(&cache, "owner", "session").unwrap().unwrap();
        assert_eq!(view.history.items.len(), 600);
        assert_eq!(view.history.items.last().unwrap().text, "streamed");
        write_live(&cache, "owner", "session", Some("two".into()), &[item(900)], false).unwrap();
        let view = read_view(&cache, "owner", "session").unwrap().unwrap();
        assert_eq!(view.history.items.len(), 500);
        cache.remove("owner", "session", "live").unwrap();
        assert!(cache.read("owner", "session", "live").unwrap().is_none());
    }
}

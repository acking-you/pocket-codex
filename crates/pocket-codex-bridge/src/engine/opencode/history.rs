//! History reads: the newest window, older pages and turn jumps.

use anyhow::{bail, Result};
use pocket_codex_host_svc::opencode::{Error as OcError, ModelRef};

use super::{
    conn, lock, mapping,
    ops::{block, oc, running_turn, turn_starts, PAGE},
    Window, MAX_RETAINED,
};
use crate::engine::{
    app_session::{OlderPage, ThreadHistory, ThreadItem, ThreadRuntimeConfig, TurnItemsPage},
    runtime, session_sync,
};

/// Older pages walked to reach a selected turn before asking for a retry.
const MAX_JUMP_PAGES: usize = 5;

/// Read the newest window of a session plus its turn rail.
pub fn thread_read(service_key: &str, thread_id: &str) -> Result<ThreadHistory> {
    let (client, shared, _) = conn(service_key)?;
    let (session, tail, (starts, complete), active) = runtime::runtime()
        .block_on(async {
            let session = client.session(thread_id).await?;
            let tail = client.messages(thread_id, PAGE, None, None).await?;
            let starts = turn_starts(&client, thread_id).await?;
            let active = client.active().await?;
            Ok::<_, OcError>((session, tail, starts, active))
        })
        .map_err(oc)?;
    let mut s = lock(&shared);
    s.active.clone_from(&active);
    s.directories
        .insert(thread_id.to_string(), session.location.directory.clone());
    if let Some(last) = starts.last() {
        s.translator
            .turns
            .insert(thread_id.to_string(), last.id.clone());
    }
    let running = active.contains(thread_id);
    let turn = running_turn(&s, thread_id);
    let items = mapping::map_window(&tail.messages, &starts, turn.as_deref());
    let model = session.model.clone();
    let collaboration = if session.agent.as_deref() == Some("plan") { "plan" } else { "default" };
    let config = ThreadRuntimeConfig {
        model: model.as_ref().map(ModelRef::qualified),
        model_provider: model.as_ref().map(|m| m.provider_id.clone()),
        reasoning_effort: model.as_ref().and_then(|m| m.variant.clone()),
        collaboration_mode: Some(collaboration.to_string()),
        confirmed_by_update: true,
        ..ThreadRuntimeConfig::default()
    };
    s.config.insert(thread_id.to_string(), config.clone());
    let history = ThreadHistory {
        history_epoch: None,
        turns: mapping::summaries(&starts, &items),
        first_turn_id: complete
            .then(|| starts.first().map(|t| t.id.clone()))
            .flatten(),
        has_older: tail.older_cursor.is_some(),
        items,
        running,
        active_turn_id: turn,
        branch: None,
        cwd: Some(session.location.directory.clone()),
        // OpenCode reports cumulative usage, not context occupancy.
        tokens_used: None,
        context_window: None,
        collaboration_mode: config.collaboration_mode.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        model: config.model.clone(),
        model_provider: config.model_provider.clone(),
        approval_policy: None,
        approvals_reviewer: None,
        service_tier: None,
        sandbox_mode: None,
        config_confirmed: true,
        turn_pages: Vec::new(),
        older_unavailable: false,
    };
    s.windows.insert(thread_id.to_string(), Window {
        messages: tail.messages,
        older: tail.older_cursor,
        read: true,
        starts,
    });
    drop(s);
    session_sync::save_history(service_key, thread_id, &history);
    Ok(history)
}

/// Read one older page into the thread's window; `None` when at the start.
fn read_older(service_key: &str, thread_id: &str) -> Result<Option<Vec<ThreadItem>>> {
    let (client, shared, _) = conn(service_key)?;
    let cursor = match lock(&shared).windows.get(thread_id) {
        Some(window) => window.older.clone(),
        None => bail!("open the session before paging its history"),
    };
    let Some(cursor) = cursor else { return Ok(None) };
    let page = block(client.messages(thread_id, PAGE, Some(&cursor), None))?;
    let mut s = lock(&shared);
    let Some(window) = s.windows.get_mut(thread_id) else { return Ok(None) };
    let items = mapping::map_window(&page.messages, &window.starts, None);
    window.older = page.older_cursor;
    let mut messages = page.messages;
    messages.append(&mut window.messages);
    // Keep the newest messages when the retention bound is hit.
    let excess = messages.len().saturating_sub(MAX_RETAINED);
    window.messages = messages.split_off(excess);
    Ok(Some(items))
}

/// One page further back.
pub fn thread_older_page(service_key: &str, thread_id: &str) -> Result<OlderPage> {
    let items = read_older(service_key, thread_id)?.unwrap_or_default();
    let (_, shared, _) = conn(service_key)?;
    let has_older = lock(&shared)
        .windows
        .get(thread_id)
        .is_some_and(|w| w.older.is_some());
    Ok(OlderPage {
        items,
        has_older,
        older_unavailable: false,
    })
}

/// Items of one turn, walking back a bounded number of pages to reach it.
/// A continuation (`load_more`) has nothing further: a turn is read whole.
pub fn thread_turn_page(
    service_key: &str,
    thread_id: &str,
    turn_id: &str,
    load_more: bool,
) -> Result<TurnItemsPage> {
    let empty = TurnItemsPage {
        turn_id: turn_id.to_string(),
        items: Vec::new(),
        has_more: false,
    };
    if load_more {
        return Ok(empty);
    }
    let (_, shared, _) = conn(service_key)?;
    for _ in 0..=MAX_JUMP_PAGES {
        let found = {
            let s = lock(&shared);
            let window = s.windows.get(thread_id);
            window.and_then(|w| {
                w.messages.iter().any(|m| m.id == turn_id).then(|| {
                    let items = mapping::map_window(
                        &w.messages,
                        &w.starts,
                        running_turn(&s, thread_id).as_deref(),
                    );
                    items
                        .into_iter()
                        .filter(|i| i.turn_id == turn_id)
                        .collect::<Vec<_>>()
                })
            })
        };
        if let Some(items) = found {
            return Ok(TurnItemsPage {
                items,
                ..empty
            });
        }
        if read_older(service_key, thread_id)?.is_none() {
            break;
        }
    }
    bail!("this turn is further back in the history; retry to keep loading")
}

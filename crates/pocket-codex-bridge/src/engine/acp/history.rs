//! History reads over the hub's attach and window methods (TRD §4.4.3), and
//! the ACP branch of the history prefetch (§4.4.6).

use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use pocket_codex_core::{
    acp::{
        pcx::{methods, AttachResult, WindowResult},
        HubItem,
    },
    history_sync::WindowQuery,
};
use serde_json::{json, Value};

use super::{ctx, events, mapping, pcx_code, Ctx};
use crate::engine::{
    app_events::event,
    app_session::{OlderPage, ThreadHistory, TurnItemsPage},
    runtime, session_sync,
};

/// Items attached when opening a session.
pub const TAIL: u32 = 20;
/// Page size of window reads.
const PAGE: u32 = 60;
/// Pages read for one turn jump.
const MAX_TURN_PAGES: usize = 5;
/// How long `thread_read` waits for `_pcx/session/loaded`.
const LOAD_WAIT: Duration = Duration::from_secs(300);
/// Attaches retried after the notification buffer overflowed.
const MAX_ATTACH_RETRIES: usize = 3;

/// Attach (or reload) `session`, replacing its view with the snapshot and
/// replaying the notifications buffered meanwhile (§4.4.2).
pub(super) async fn attach(
    ctx: &Arc<Ctx>,
    session: &str,
    cwd: Option<&str>,
    tail: u32,
    reload: bool,
) -> Result<AttachResult> {
    for _ in 0..MAX_ATTACH_RETRIES {
        let existed = {
            let mut s = ctx.shared();
            let existed = s.sessions.contains_key(session);
            let view = s.sessions.entry(session.to_string()).or_default();
            view.syncing = true;
            view.buffered.clear();
            view.overflowed = false;
            existed
        };
        let (method, params) = if reload {
            (methods::SESSION_RELOAD, json!({"sessionId": session}))
        } else {
            let mut params = json!({"sessionId": session, "tail": tail});
            if let Some(cwd) = cwd {
                params["cwd"] = json!(cwd);
            }
            (methods::SESSION_ATTACH, params)
        };
        let result = ctx.call(method, params).await.and_then(|v| {
            serde_json::from_value::<AttachResult>(v).context("decoding the attach result")
        });
        let mut s = ctx.shared();
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(e) => {
                if existed {
                    if let Some(view) = s.sessions.get_mut(session) {
                        view.syncing = false;
                        view.buffered.clear();
                    }
                } else {
                    s.sessions.remove(session);
                }
                return Err(e);
            },
        };
        let Some(view) = s.sessions.get_mut(session) else { bail!("the session view was dropped") };
        view.reset_from(&snapshot);
        let mut buffered = std::mem::take(&mut view.buffered);
        let overflowed = std::mem::take(&mut view.overflowed);
        view.syncing = false;
        buffered.sort_by_key(|(seq, _, _)| *seq);
        let mut out = Vec::new();
        for (seq, method, params) in buffered {
            let Some(view) = s.sessions.get_mut(session) else { break };
            if seq <= view.seq {
                continue;
            }
            view.seq = seq;
            out.extend(events::apply(&mut s, session, &method, &params));
        }
        drop(s);
        ctx.emit(out);
        if !overflowed {
            return Ok(snapshot);
        }
    }
    bail!("the session changes too fast to attach; try again")
}

/// Wait for `_pcx/session/loaded` (bounded); `Err` carries `loadFailed`.
async fn wait_loaded(ctx: &Arc<Ctx>, session: &str) -> Result<()> {
    let rx = {
        let mut s = ctx.shared();
        let Some(view) = s.sessions.get(session) else { return Ok(()) };
        if let Some(error) = view.load_failed.clone() {
            return Err(anyhow!(error));
        }
        if !view.loading {
            return Ok(());
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        s.loads.entry(session.to_string()).or_default().push(tx);
        rx
    };
    match tokio::time::timeout(LOAD_WAIT, rx).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(anyhow!(error)),
        Ok(Err(_)) => bail!("the ACP hub connection closed while the session was loading"),
        Err(_) => bail!("[acp.timeout] the session is still loading; try again later"),
    }
}

/// Attach and wait until the hub finished loading.
async fn attach_loaded(ctx: &Arc<Ctx>, session: &str, tail: u32) -> Result<AttachResult> {
    let snapshot = attach(ctx, session, None, tail, false).await?;
    if !snapshot.loading {
        return Ok(snapshot);
    }
    wait_loaded(ctx, session).await?;
    attach(ctx, session, None, tail, false).await
}

/// `ThreadHistory` of an attach snapshot (§4.4.3 `app_thread_read`).
fn history_of(snapshot: &AttachResult) -> ThreadHistory {
    let config = mapping::runtime_config(&snapshot.config_options);
    ThreadHistory {
        history_epoch: Some(snapshot.generation.clone()),
        items: mapping::thread_items(&snapshot.items, &snapshot.turns),
        running: snapshot.running,
        active_turn_id: snapshot.active_turn.map(mapping::turn_id),
        cwd: Some(snapshot.cwd.clone()).filter(|c| !c.is_empty()),
        tokens_used: snapshot.usage.as_ref().map(|u| u.used as i64),
        context_window: snapshot.usage.as_ref().map(|u| u.size as i64),
        collaboration_mode: config.collaboration_mode.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        model: config.model.clone(),
        config_confirmed: true,
        has_older: snapshot.has_older,
        turns: mapping::summaries(&snapshot.turns, &snapshot.items),
        first_turn_id: (snapshot.dropped_turns == 0)
            .then(|| snapshot.turns.first().map(|t| mapping::turn_id(t.turn)))
            .flatten(),
        turn_pages: Vec::new(),
        older_unavailable: snapshot.older_unavailable,
        ..ThreadHistory::default()
    }
}

/// Read the newest window of a session plus its turn rail.
pub fn thread_read(service_key: &str, thread_id: &str) -> Result<ThreadHistory> {
    let ctx = ctx(service_key)?;
    let snapshot = runtime::runtime().block_on(attach_loaded(&ctx, thread_id, TAIL))?;
    let history = history_of(&snapshot);
    session_sync::save_history(service_key, thread_id, &history);
    Ok(history)
}

/// Force the hub to re-materialize a session (`session/load` again).
pub fn thread_reload(service_key: &str, thread_id: &str) -> Result<()> {
    let ctx = ctx(service_key)?;
    let before = ctx
        .shared()
        .sessions
        .get(thread_id)
        .map(|v| v.generation.clone());
    let snapshot = runtime::runtime().block_on(attach(&ctx, thread_id, None, TAIL, true))?;
    if before.is_some_and(|g| g != snapshot.generation) {
        ctx.emit(vec![event("acp/session/generation", thread_id, json!({"threadId": thread_id}))]);
    }
    Ok(())
}

async fn window(ctx: &Arc<Ctx>, session: &str, params: Value) -> Result<WindowResult> {
    match ctx.call(methods::SESSION_WINDOW, params).await {
        Ok(value) => serde_json::from_value(value).context("decoding the window result"),
        Err(e) => {
            if pcx_code(&e).as_deref() == Some("acp.generation_changed") {
                ctx.emit(vec![event(
                    "acp/session/generation",
                    session,
                    json!({"threadId": session}),
                )]);
            }
            Err(e)
        },
    }
}

fn generation_of(ctx: &Ctx, session: &str) -> Result<String> {
    ctx.shared()
        .sessions
        .get(session)
        .map(|v| v.generation.clone())
        .ok_or_else(|| anyhow!("open the session first"))
}

/// One page further back.
pub fn thread_older_page(service_key: &str, thread_id: &str) -> Result<OlderPage> {
    let ctx = ctx(service_key)?;
    let (generation, before, has_older) = {
        let s = ctx.shared();
        let view = s
            .sessions
            .get(thread_id)
            .ok_or_else(|| anyhow!("open the session first"))?;
        (view.generation.clone(), view.items.first().map(|i| i.id.clone()), view.has_older)
    };
    let Some(before) = before.filter(|_| has_older) else {
        let unavailable = ctx
            .shared()
            .sessions
            .get(thread_id)
            .is_some_and(|v| v.older_unavailable);
        return Ok(OlderPage {
            items: Vec::new(),
            has_older: false,
            older_unavailable: unavailable,
        });
    };
    let params =
        json!({"sessionId": thread_id, "generation": generation, "before": before, "limit": PAGE});
    let page = runtime::runtime().block_on(window(&ctx, thread_id, params))?;
    let mut s = ctx.shared();
    let Some(view) = s.sessions.get_mut(thread_id) else { bail!("the session view was dropped") };
    view.prepend(page.items.clone());
    view.has_older = page.has_older;
    view.older_unavailable = page.older_unavailable;
    Ok(OlderPage {
        items: mapping::thread_items(&page.items, &view.turns),
        has_older: page.has_older,
        older_unavailable: page.older_unavailable,
    })
}

/// Read or continue one turn (§4.4.3 `app_thread_turn_page`).
pub fn thread_turn_page(
    service_key: &str,
    thread_id: &str,
    turn_id: &str,
    load_more: bool,
    delta_only: bool,
) -> Result<TurnItemsPage> {
    let ctx = ctx(service_key)?;
    let turn = mapping::parse_turn(turn_id).ok_or_else(|| anyhow!("unknown turn {turn_id}"))?;
    let generation = generation_of(&ctx, thread_id)?;
    let after = load_more
        .then(|| {
            ctx.shared()
                .sessions
                .get(thread_id)
                .and_then(|v| v.turn_reads.get(&turn))
                .and_then(|read| read.last().map(|i| i.id.clone()))
        })
        .flatten();
    let mut params =
        json!({"sessionId": thread_id, "generation": generation, "turn": turn, "limit": PAGE});
    if let Some(after) = &after {
        params["after"] = json!(after);
    }
    let page = runtime::runtime().block_on(window(&ctx, thread_id, params))?;
    let mut s = ctx.shared();
    let Some(view) = s.sessions.get_mut(thread_id) else { bail!("the session view was dropped") };
    let read = view.turn_reads.entry(turn).or_default();
    if after.is_none() {
        read.clear();
    }
    read.extend(page.items.iter().cloned());
    let returned: Vec<HubItem> = if delta_only { page.items.clone() } else { read.clone() };
    Ok(TurnItemsPage {
        turn_id: turn_id.to_string(),
        items: mapping::thread_items(&returned, &view.turns),
        has_more: page.has_more,
    })
}

/// Every item of one turn (up to [`MAX_TURN_PAGES`] pages).
pub fn thread_turn_items(
    service_key: &str,
    thread_id: &str,
    turn_id: &str,
) -> Result<Vec<crate::engine::app_session::ThreadItem>> {
    let mut page = thread_turn_page(service_key, thread_id, turn_id, false, false)?;
    for _ in 1..MAX_TURN_PAGES {
        if !page.has_more {
            break;
        }
        page = thread_turn_page(service_key, thread_id, turn_id, true, false)?;
    }
    Ok(page.items)
}

/// The ACP branch of `session_sync::prefetch` (§4.4.6): a 20-item tail from
/// `/history/v1` plus the running flag from the metadata collection.
pub fn prefetch_history(service: &str, session: &str) -> Result<()> {
    let query = |collection: &str| WindowQuery {
        session: session.to_string(),
        collection: collection.into(),
        group: None,
        cursor: None,
        limit: TAIL,
        projection: None,
    };
    let tail = session_sync::sync_window(service, &query("items"), true)?;
    let mut items: Vec<HubItem> = Vec::with_capacity(tail.order.len());
    for id in tail.order.iter().rev() {
        if let Some(document) = tail.documents.get(id) {
            items
                .push(serde_json::from_value(document.clone()).context("decoding a history item")?);
        }
    }
    let metadata = session_sync::sync_window(service, &query("metadata"), true)?.metadata;
    let history = ThreadHistory {
        history_epoch: Some(tail.generation.clone()),
        items: mapping::thread_items(&items, &[]),
        running: metadata["running"].as_bool().unwrap_or(false),
        cwd: metadata["cwd"].as_str().map(str::to_string),
        has_older: tail.metadata["hasOlder"].as_bool().unwrap_or(false),
        older_unavailable: tail.metadata["olderUnavailable"].as_bool().unwrap_or(false),
        ..ThreadHistory::default()
    };
    session_sync::save_history(service, session, &history);
    Ok(())
}

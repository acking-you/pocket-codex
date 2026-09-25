//! Controller history synchronization, independent of app-server connections.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, ensure, Context, Result};
use pocket_codex_codex::client::AppClient;
use pocket_codex_core::history_sync::{
    self as wire, HistoryWindow, SyncRequest, SyncResponse, WindowQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    app_session::{ThreadHistory, ThreadItem},
    meta, runtime,
    session_cache::{application_cache, namespace},
};

#[derive(Serialize, Deserialize)]
struct CachedView {
    generation: Option<String>,
    history: ThreadHistory,
}

type Generations = Mutex<HashMap<String, String>>;
fn generations() -> &'static Generations {
    static GENERATIONS: OnceLock<Generations> = OnceLock::new();
    GENERATIONS.get_or_init(Default::default)
}

fn generation_key(owner: &str, session: &str) -> String {
    format!("{owner}:{session}")
}

type Capabilities = Mutex<HashMap<String, (bool, Instant)>>;
fn capabilities() -> &'static Capabilities {
    static STATE: OnceLock<Capabilities> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

#[derive(Default)]
struct SessionGate {
    lock: Mutex<()>,
    revision: AtomicU64,
}

fn gate(owner: &str, session: &str) -> Arc<SessionGate> {
    type Gates = Mutex<HashMap<String, Weak<SessionGate>>>;
    static GATES: OnceLock<Gates> = OnceLock::new();
    let mut gates = GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let key = format!("{owner}:{session}");
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return gate;
    }
    gates.retain(|_, gate| gate.strong_count() != 0);
    let gate = Arc::new(SessionGate::default());
    gates.insert(key, Arc::downgrade(&gate));
    gate
}

/// A queued live snapshot may commit only while its source is still current.
pub(super) struct LiveCheckpoint {
    owner: String,
    gate: Arc<SessionGate>,
    revision: u64,
    reset: bool,
}

/// Capture an in-memory invalidation token without doing IO on the event loop.
pub(super) fn live_checkpoint(owner: &str, session: &str, reset: bool) -> LiveCheckpoint {
    let gate = gate(owner, session);
    if reset {
        gate.revision.fetch_add(1, Ordering::SeqCst);
    }
    let revision = gate.revision.load(Ordering::SeqCst);
    LiveCheckpoint {
        owner: owner.into(),
        gate,
        revision,
        reset,
    }
}

/// Probe the explicit capability endpoint. Only an absent endpoint is legacy;
/// timeouts, authorization failures and unsupported versions stay visible.
pub fn prepare(service: &str) -> Result<bool> {
    let owner = namespace(service)?;
    if let Some((enabled, checked)) = capabilities()
        .lock()
        .ok()
        .and_then(|s| s.get(&owner).copied())
    {
        if checked.elapsed() < Duration::from_secs(60) {
            return Ok(enabled);
        }
    }
    let url = meta::endpoint(service, &["history", "v1", "capabilities"])?;
    let supported = runtime::runtime().block_on(async {
        let response = meta::client()
            .get(url)
            .send()
            .await
            .context("history capability negotiation")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let body: Value = response.error_for_status()?.json().await?;
        ensure!(body["version"] == wire::VERSION, "unsupported history synchronization version");
        ensure!(body["provider"] == "codex/app-server-v2", "unsupported history document schema");
        Ok::<_, anyhow::Error>(true)
    })?;
    if let Ok(mut state) = capabilities().lock() {
        if state.len() >= 128 {
            state.clear();
        }
        state.insert(owner, (supported, Instant::now()));
    }
    Ok(supported)
}

pub(super) fn enabled(service: &str) -> bool {
    namespace(service)
        .ok()
        .and_then(|owner| {
            capabilities()
                .lock()
                .ok()
                .and_then(|s| s.get(&owner).map(|s| s.0))
        })
        .unwrap_or(false)
}

/// Latest validated source generation, separate from an RPC connection.
pub(super) fn source_generation(service: &str, session: &str) -> Option<String> {
    let owner = namespace(service).ok()?;
    generations()
        .lock()
        .ok()?
        .get(&generation_key(&owner, session))
        .cloned()
}

/// Read via negotiated meta synchronization, preserving legacy RPC callers.
pub fn request(
    service: &str,
    client: &Arc<AppClient>,
    method: &str,
    params: Value,
) -> Result<Value> {
    if !enabled(service) {
        return runtime::runtime().block_on(client.request(method, params));
    }
    let query = query_for(method, &params)?;
    let window = sync(service, &query, false)?;
    pocket_codex_host_svc::history_sync::codex_response(&window, &query)
}

fn query_for(method: &str, params: &Value) -> Result<WindowQuery> {
    let collection = match method {
        "thread/read" => "metadata",
        "thread/items/list" => "items",
        "thread/turns/list" => "groups",
        _ => bail!("unsupported synchronized read"),
    };
    Ok(WindowQuery {
        session: params["threadId"]
            .as_str()
            .context("missing history session")?
            .into(),
        collection: collection.into(),
        group: params["turnId"].as_str().map(str::to_owned),
        cursor: params["cursor"].as_str().map(str::to_owned),
        limit: params["limit"].as_u64().unwrap_or(1).min(100) as u32,
        projection: if collection == "items" {
            params["sortDirection"].as_str().map(str::to_owned)
        } else {
            params["itemsView"].as_str().map(str::to_owned)
        },
    })
}

fn key(query: &WindowQuery) -> Result<String> {
    Ok(format!("window:{}", wire::digest_bytes(&serde_json::to_vec(query)?)))
}

fn sync(service: &str, query: &WindowQuery, running: bool) -> Result<HistoryWindow> {
    let owner = namespace(service)?;
    let gate = gate(&owner, &query.session);
    let _request = gate
        .lock
        .lock()
        .map_err(|_| anyhow::anyhow!("history sync gate poisoned"))?;
    let cache = application_cache()?;
    let key = key(query)?;
    let base: Option<HistoryWindow> = cache.read_json(&owner, &query.session, &key)?;
    let known = base.as_ref().map(HistoryWindow::manifest).transpose()?;
    let url = meta::endpoint(service, &["history", "v1", "window"])?;
    let response: SyncResponse = runtime::runtime().block_on(async {
        let response = meta::client()
            .post(url)
            .json(&SyncRequest {
                version: wire::VERSION,
                query: query.clone(),
                known,
            })
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::GONE {
            bail!("history window invalidated; reopen the session to load a fresh window");
        }
        let mut response = response.error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 64 * 1024 * 1024,
                "history response exceeds memory limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).context("decoding history delta")
    })?;
    let updated = wire::apply(base.as_ref(), &response)?;
    let generation_changed = {
        let mut state = generations()
            .lock()
            .map_err(|_| anyhow::anyhow!("history generations poisoned"))?;
        if state.len() >= 128 {
            state.clear();
        }
        state
            .insert(generation_key(&owner, &query.session), updated.generation.clone())
            .is_some_and(|old| old != updated.generation)
    };
    if response.reset || generation_changed {
        gate.revision.fetch_add(1, Ordering::SeqCst);
        cache.invalidate_session(&owner, &query.session)?;
        super::app_session::reset_synced_history(service, &query.session);
    }
    if response.reset
        || generation_changed
        || !response.changes.is_empty()
        || response.metadata.is_some()
        || base.is_none()
    {
        if let Err(error) = cache.write_json(&owner, &query.session, &key, &updated, running) {
            tracing::warn!(%error, "could not persist history window");
        }
    }
    Ok(updated)
}

/// Load a display-only snapshot without transport setup or a live connection.
pub fn cached_history(service: &str, session: &str) -> Result<Option<ThreadHistory>> {
    let owner = namespace(service)?;
    let Some(view) = application_cache()?.read_json::<CachedView>(&owner, session, "view")? else {
        return Ok(None);
    };
    if let Some(generation) = &view.generation {
        if let Ok(mut state) = generations().lock() {
            state
                .entry(generation_key(&owner, session))
                .or_insert_with(|| generation.clone());
        }
    }
    let mut history = view.history;
    history.history_epoch = view.generation;
    // Cached state must never revive actionable approvals or claim live control.
    history.running = false;
    history.config_confirmed = false;
    history.turn_pages.clear();
    for item in &mut history.items {
        item.questions_json = None;
    }
    Ok(Some(history))
}

/// Save a bounded display snapshot separately from authoritative wire windows.
pub fn save_history(service: &str, session: &str, history: &ThreadHistory) {
    let save = || -> Result<()> {
        let owner = namespace(service)?;
        let gate = gate(&owner, session);
        let _request = gate
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("history gate poisoned"))?;
        save_history_locked(service, &owner, session, history)
    };
    if let Err(error) = save() {
        tracing::debug!(%error, "history display cache unavailable");
    }
}

fn save_history_locked(
    service: &str,
    owner: &str,
    session: &str,
    history: &ThreadHistory,
) -> Result<()> {
    let current = source_generation(service, session);
    ensure!(
        history.history_epoch.is_none() || current.is_none() || history.history_epoch == current,
        "display history belongs to a superseded source generation"
    );
    let mut snapshot = history.clone();
    snapshot.turn_pages.clear();
    if snapshot.items.len() > 100 {
        snapshot.items.drain(..snapshot.items.len() - 100);
        snapshot.has_older = true;
    }
    for item in &mut snapshot.items {
        item.questions_json = None;
    }
    let generation = history.history_epoch.clone().or(current);
    application_cache()?.write_json(
        owner,
        session,
        "view",
        &CachedView {
            generation,
            history: snapshot,
        },
        history.running,
    )?;
    Ok(())
}

/// Coalesced live checkpoint: received text remains available after app death,
/// and fingerprints of retained tool/message text can serve as the next base.
pub(super) fn checkpoint_live(
    service: &str,
    session: &str,
    token: LiveCheckpoint,
    items: Vec<ThreadItem>,
    running: bool,
) {
    let update = || -> Result<()> {
        let owner = namespace(service)?;
        let _request = token
            .gate
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("history gate poisoned"))?;
        if owner != token.owner || token.gate.revision.load(Ordering::SeqCst) != token.revision {
            return Ok(());
        }
        let cache = application_cache()?;
        if token.reset {
            cache.invalidate_session(&owner, session)?;
            return Ok(());
        }
        let mut history = cache
            .read_json::<CachedView>(&owner, session, "view")?
            .map(|view| view.history)
            .unwrap_or_default();
        for item in &items {
            if let Some(old) = history.items.iter_mut().find(|old| old.id == item.id) {
                *old = item.clone();
            } else {
                history.items.push(item.clone());
            }
        }
        history.running = running;
        save_history_locked(service, &owner, session, &history)?;
        let query = query_for(
            "thread/items/list",
            &json!({"threadId": session, "limit": 20, "sortDirection": "desc"}),
        )?;
        let key = key(&query)?;
        if let Some(mut window) = cache.read_json::<HistoryWindow>(&owner, session, &key)? {
            for item in items {
                let field = match item.item_type.as_str() {
                    "agentMessage" => "text",
                    // Tool display text may be trimmed or carry an exit-code
                    // annotation. Only raw wire windows can fingerprint it.
                    _ => continue,
                };
                let id = format!("{}:{}", item.turn_id, item.id);
                if let Some(document) = window.documents.get_mut(&id) {
                    document["item"][field] = json!(item.text);
                } else if !item.turn_id.is_empty() && !item.id.is_empty() {
                    let mut document = json!({"turnId": item.turn_id, "item": {"id": item.id, "type": item.item_type}});
                    document["item"][field] = json!(item.text);
                    window.documents.insert(id.clone(), document);
                    window.order.insert(0, id);
                }
            }
            window.order.truncate(20);
            window.documents.retain(|id, _| window.order.contains(id));
            cache.write_json(&owner, session, &key, &window, running)?;
        }
        Ok(())
    };
    if let Err(error) = update() {
        tracing::debug!(%error, "live history checkpoint unavailable");
    }
}

/// Prefetch a bounded tail without connecting, resuming, or taking over a
/// thread. The caller controls foreground lifecycle, concurrency and
/// scheduling.
pub fn prefetch(service: &str, session: &str) -> Result<()> {
    if super::session_cache::is_focused(service, session) {
        return Ok(());
    }
    if !prepare(service)? {
        return Ok(());
    }
    let mut history = ThreadHistory::default();
    let metadata_query = query_for("thread/read", &json!({"threadId": session}))?;
    let metadata = pocket_codex_host_svc::history_sync::codex_response(
        &sync(service, &metadata_query, true)?,
        &metadata_query,
    )?;
    let thread = &metadata["thread"];
    history.cwd = thread["cwd"].as_str().map(str::to_owned);
    history.model = thread["model"].as_str().map(str::to_owned);
    history.model_provider = thread["modelProvider"].as_str().map(str::to_owned);
    let query = query_for(
        "thread/items/list",
        &json!({"threadId": session, "limit": 20, "sortDirection": "desc"}),
    )?;
    let tail = sync(service, &query, true)?;
    history.history_epoch = Some(tail.generation.clone());
    let response = pocket_codex_host_svc::history_sync::codex_response(&tail, &query)?;
    history.items = super::app_session::parse_prefetched_items(&response);
    history.has_older = response["nextCursor"].as_str().is_some();
    history.running = true;
    save_history(service, session, &history);
    Ok(())
}

/// Cache a fetched preview using the same global byte budget as history.
pub fn preview(service: &str, session: &str, path: &str) -> Result<Option<Vec<u8>>> {
    application_cache()?.read(&namespace(service)?, session, &format!("image:{path}"))
}

/// Persist only requested previews; downloaded user files use another path.
pub fn save_preview(service: &str, session: &str, path: &str, bytes: &[u8]) {
    let result = (|| {
        application_cache()?.write(
            &namespace(service)?,
            session,
            &format!("image:{path}"),
            bytes,
            false,
        )
    })();
    if let Err(error) = result {
        tracing::debug!(%error, "preview cache unavailable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_replacement_invalidates_queued_checkpoints_only_for_that_session() {
        let old = live_checkpoint("checkpoint-test", "one", false);
        let other = live_checkpoint("checkpoint-test", "two", false);
        let replacement = live_checkpoint("checkpoint-test", "one", true);
        assert!(Arc::ptr_eq(&old.gate, &replacement.gate));
        assert_ne!(old.revision, old.gate.revision.load(Ordering::SeqCst));
        assert_eq!(other.revision, other.gate.revision.load(Ordering::SeqCst));
        let new = live_checkpoint("checkpoint-test", "one", false);
        assert_eq!(new.revision, replacement.revision);
        // A version change learned by synchronization also rejects queued work.
        new.gate.revision.fetch_add(1, Ordering::SeqCst);
        assert_ne!(new.revision, new.gate.revision.load(Ordering::SeqCst));
    }
}

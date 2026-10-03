//! `/history/v1` adapter over the hub transcript (TRD §4.2.10, D9).
//!
//! ```text
//!   collection  cursor            order
//!   metadata    —                 (no documents; metadata only)
//!   items       b:<itemId> desc   newest first (projection omitted / "desc")
//!               a:<itemId> asc    oldest first (projection "asc")
//!   groups      t:<turn>          newest turn first
//! ```

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use pocket_codex_core::{
    acp::{HubItem, Transcript},
    history_sync::{HistoryWindow, WindowQuery},
};
use serde_json::{json, Value};

use super::{
    hub::{lock, AcpHub},
    session::Phase,
};
use crate::{file_links::SessionDirResolver, history_sync::SessionHistorySource};

/// History provider id.
pub const PROVIDER: &str = "acp/hub-v1";
/// How long a history read waits for a session to load.
const LOAD_WAIT: Duration = Duration::from_secs(25);

/// [`SessionHistorySource`] backed by an [`AcpHub`].
pub struct AcpHistorySource {
    hub: Arc<AcpHub>,
}

impl AcpHistorySource {
    /// Adapter for `hub`.
    pub fn new(hub: Arc<AcpHub>) -> Self {
        Self {
            hub,
        }
    }
}

#[async_trait]
impl SessionHistorySource for AcpHistorySource {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    async fn read_window(&self, query: &WindowQuery) -> Result<HistoryWindow> {
        self.hub.history_window(query).await
    }
}

/// Session working directories for `/fs/thread-file`.
pub struct AcpSessionDirs(pub Arc<AcpHub>);

#[async_trait]
impl SessionDirResolver for AcpSessionDirs {
    async fn session_dir(&self, session: &str) -> Result<Option<String>> {
        Ok(self.0.session_cwd(session))
    }
}

impl AcpHub {
    /// One `/history/v1` window; loads the session (without subscribing)
    /// when no transcript is materialized yet.
    pub async fn history_window(self: &Arc<Self>, query: &WindowQuery) -> Result<HistoryWindow> {
        query.validate()?;
        let needs_load = {
            let st = lock(&self.state);
            st.sessions
                .get(&query.session)
                .is_none_or(|s| s.transcript.is_none())
        };
        if needs_load {
            let loaded =
                tokio::time::timeout(LOAD_WAIT, self.load_for_history(&query.session)).await;
            match loaded {
                Err(_) => bail!("history is still loading"),
                Ok(Err(e)) if e.code() == "acp.session_not_loadable" => {
                    bail!("session cannot be loaded by this agent")
                },
                Ok(Err(e)) => bail!("{e}"),
                Ok(Ok(())) => {},
            }
        }
        let st = lock(&self.state);
        let session = st.sessions.get(&query.session).context("unknown session")?;
        if matches!(session.phase, Phase::Loading) && session.transcript.is_none() {
            bail!("history is still loading");
        }
        let transcript = session
            .transcript
            .as_ref()
            .context("history is still loading")?;
        let generation = transcript.generation().to_string();
        let window = match query.collection.as_str() {
            "metadata" => HistoryWindow {
                provider: PROVIDER.into(),
                generation,
                metadata: json!({
                    "sessionId": session.id,
                    "cwd": session.cwd,
                    "title": session.title,
                    "updatedAt": session.updated_at,
                    "running": session.running(),
                    "turns": transcript.turns().len(),
                    "olderUnavailable": transcript.dropped_turns() > 0,
                }),
                order: Vec::new(),
                documents: BTreeMap::new(),
            },
            "items" => items_window(transcript, query, generation)?,
            "groups" => groups_window(transcript, query, generation)?,
            _ => bail!("unsupported history collection"),
        };
        window.validate()?;
        Ok(window)
    }

    async fn load_for_history(self: &Arc<Self>, session: &str) -> Result<(), super::AcpError> {
        super::ops::ensure_loaded_quietly(self, session).await
    }
}

fn items_window(t: &Transcript, query: &WindowQuery, generation: String) -> Result<HistoryWindow> {
    let all = t.items();
    let range = match &query.group {
        Some(group) => {
            let turn: u32 = group
                .strip_prefix('t')
                .and_then(|n| n.parse().ok())
                .context("invalid history group")?;
            let lo = all.partition_point(|i| i.turn < turn);
            let hi = all.partition_point(|i| i.turn <= turn);
            lo..hi
        },
        None => 0..all.len(),
    };
    let scope = &all[range.clone()];
    let ascending = query.projection.as_deref() == Some("asc");
    ensure!(
        matches!(query.projection.as_deref(), None | Some("asc") | Some("desc")),
        "unsupported item projection"
    );
    let limit = query.limit as usize;
    let position = |id: &str| scope.iter().position(|i| i.id == id);
    let (selected, more, first): (Vec<&HubItem>, bool, usize) = if ascending {
        let start = match &query.cursor {
            Some(cursor) => {
                let id = cursor
                    .strip_prefix("a:")
                    .context("invalid history cursor")?;
                position(id).context("invalid history cursor: item not found")? + 1
            },
            None => 0,
        };
        let end = (start + limit).min(scope.len());
        (scope[start..end].iter().collect(), end < scope.len(), start)
    } else {
        let end = match &query.cursor {
            Some(cursor) => {
                let id = cursor
                    .strip_prefix("b:")
                    .context("invalid history cursor")?;
                position(id).context("invalid history cursor: item not found")?
            },
            None => scope.len(),
        };
        let start = end.saturating_sub(limit);
        (scope[start..end].iter().rev().collect(), start > 0, start)
    };
    let has_older = first > 0 || range.start > 0;
    let next = match (more, selected.last()) {
        (true, Some(last)) => Some(format!("{}:{}", if ascending { "a" } else { "b" }, last.id)),
        _ => None,
    };
    let mut documents = BTreeMap::new();
    let mut order = Vec::new();
    for item in selected {
        order.push(item.id.clone());
        documents.insert(item.id.clone(), serde_json::to_value(item)?);
    }
    let mut metadata = json!({
        "hasOlder": has_older,
        "olderUnavailable": t.dropped_turns() > 0 && !has_older,
    });
    if let Some(next) = next {
        metadata["nextCursor"] = Value::String(next);
    }
    Ok(HistoryWindow {
        provider: PROVIDER.into(),
        generation,
        metadata,
        order,
        documents,
    })
}

fn groups_window(t: &Transcript, query: &WindowQuery, generation: String) -> Result<HistoryWindow> {
    ensure!(
        matches!(query.projection.as_deref(), None | Some("desc") | Some("summary")),
        "unsupported group projection"
    );
    let turns = t.turns();
    let end = match &query.cursor {
        Some(cursor) => {
            let turn: u32 = cursor
                .strip_prefix("t:")
                .and_then(|n| n.parse().ok())
                .context("invalid history cursor")?;
            turns
                .iter()
                .position(|i| i.turn == turn)
                .context("invalid history cursor: turn not found")?
        },
        None => turns.len(),
    };
    let start = end.saturating_sub(query.limit as usize);
    let mut documents = BTreeMap::new();
    let mut order = Vec::new();
    for info in turns[start..end].iter().rev() {
        let key = format!("t{}", info.turn);
        order.push(key.clone());
        documents.insert(key, serde_json::to_value(info)?);
    }
    let mut metadata = json!({});
    if start > 0 {
        metadata["nextCursor"] = Value::String(format!("t:{}", turns[start].turn));
    }
    Ok(HistoryWindow {
        provider: PROVIDER.into(),
        generation,
        metadata,
        order,
        documents,
    })
}

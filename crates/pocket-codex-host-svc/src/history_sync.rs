//! Read-only history adapters and the provider-independent synchronization API.

use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use pocket_codex_codex::client::AppClient;
use pocket_codex_core::history_sync::{
    self as wire, HistoryWindow, SyncRequest, SyncResponse, WindowQuery,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

/// A bounded read-only history source. Adapters own collection projection,
/// stable document ids, source generations and opaque cursors. They must never
/// resume a session, take ownership, answer an approval or start model work.
#[async_trait]
pub trait SessionHistorySource: Send + Sync {
    /// Schema identifier advertised during capability negotiation.
    fn provider(&self) -> &'static str;
    /// Read one authoritative window without modifying the source session.
    async fn read_window(&self, query: &WindowQuery) -> Result<HistoryWindow>;
}

/// Build the versioned synchronization routes for any history source.
pub fn router(source: Arc<dyn SessionHistorySource>) -> Router {
    Router::new()
        .route("/history/v1/capabilities", get(capabilities))
        .route("/history/v1/window", post(sync_window).layer(DefaultBodyLimit::max(512 * 1024)))
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(source)
}

async fn capabilities(State(source): State<Arc<dyn SessionHistorySource>>) -> Json<Value> {
    Json(json!({"version": wire::VERSION, "provider": source.provider(),
        "maxDocuments": wire::MAX_DOCUMENTS, "textAppend": true}))
}

async fn sync_window(
    State(source): State<Arc<dyn SessionHistorySource>>,
    Json(request): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, (StatusCode, String)> {
    request
        .validate()
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let window = tokio::time::timeout(Duration::from_secs(30), source.read_window(&request.query))
        .await
        .map_err(|_| (StatusCode::GATEWAY_TIMEOUT, "history source timed out".into()))?
        .map_err(|e| {
            let message = format!("{e:#}");
            let invalid_cursor = request.query.cursor.is_some()
                && message.to_lowercase().contains("cursor")
                && (message.to_lowercase().contains("invalid")
                    || message.to_lowercase().contains("not found"));
            (if invalid_cursor { StatusCode::GONE } else { StatusCode::BAD_GATEWAY }, message)
        })?;
    wire::reconcile(&window, request.known.as_ref())
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))
}

/// Codex adapter using a separate, read-only connection to the external
/// runtime. This connection never attaches through thread/resume or takes
/// ownership.
pub struct CodexHistorySource {
    address: SocketAddr,
    client: Mutex<Option<Arc<AppClient>>>,
    directories: Option<(PathBuf, PathBuf)>,
}

impl CodexHistorySource {
    /// Create a lazy adapter; construction does not connect to the runtime.
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            client: Mutex::new(None),
            directories: None,
        }
    }

    /// Use an explicit Codex home and disposable index directory, including
    /// isolated tests, without changing process-global environment settings.
    pub fn with_history_directory(
        address: SocketAddr,
        codex_home: PathBuf,
        index_directory: PathBuf,
    ) -> Self {
        Self {
            address,
            client: Mutex::new(None),
            directories: Some((codex_home, index_directory)),
        }
    }

    async fn generation(&self, session: &str) -> Result<String> {
        let directories = self.directories.clone();
        let session = session.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Some((home, index)) = directories {
                let path =
                    pocket_codex_codex::rollout::rollout_path_in(&home.join("sessions"), &session)?
                        .context("history session no longer exists")?;
                super::history_sync_revision::scan(&path, &index)
            } else {
                super::history_sync_revision::generation(&session)
            }
        })
        .await
        .context("history revision task")?
    }

    async fn read_codex(&self, query: &WindowQuery, method: &str, params: Value) -> Result<Value> {
        if query.collection == "metadata" {
            return self.request(method, params).await;
        }
        let metadata = self
            .request("thread/read", json!({"threadId": query.session, "includeTurns": false}))
            .await?;
        if metadata["thread"]["historyMode"] == "paginated" {
            return self.request(method, params).await;
        }
        // Codex's legacy histories explicitly reject items/list. Hydrate them
        // on the host, then expose only the requested bounded window remotely.
        let history = self
            .request("thread/read", json!({"threadId": query.session, "includeTurns": true}))
            .await?;
        legacy_page(query, history)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let mut connection = self.client.lock().await;
        if !connection.as_ref().is_some_and(|client| client.is_alive()) {
            let (client, receiver) = AppClient::connect(&format!("ws://{}", self.address)).await?;
            // Read-only RPCs do not subscribe to session events. Dropping the
            // receiver also prevents unrelated notifications from accumulating.
            drop(receiver);
            client.initialize("pocket-codex-history", true).await?;
            *connection = Some(Arc::new(client));
        }
        let client = connection
            .as_ref()
            .context("history connection unavailable")?
            .clone();
        drop(connection);
        client.request(method, params).await
    }
}

#[async_trait]
impl SessionHistorySource for CodexHistorySource {
    fn provider(&self) -> &'static str {
        "codex/app-server-v2"
    }

    async fn read_window(&self, query: &WindowQuery) -> Result<HistoryWindow> {
        query.validate()?;
        let mut params = json!({"threadId": query.session});
        let method = match query.collection.as_str() {
            "metadata" => {
                params["includeTurns"] = json!(false);
                "thread/read"
            },
            "items" | "groups" => {
                params["limit"] = json!(query.limit);
                params["sortDirection"] = json!(if query.collection == "items"
                    && query.projection.as_deref() == Some("asc")
                {
                    "asc"
                } else {
                    "desc"
                });
                if let Some(cursor) = &query.cursor {
                    params["cursor"] = json!(cursor);
                }
                if query.collection == "items" {
                    if let Some(group) = &query.group {
                        params["turnId"] = json!(group);
                    }
                    "thread/items/list"
                } else {
                    let view = query.projection.as_deref().unwrap_or("summary");
                    ensure!(
                        matches!(view, "summary" | "notLoaded"),
                        "unsupported group projection"
                    );
                    params["itemsView"] = json!(view);
                    "thread/turns/list"
                }
            },
            _ => bail!("unsupported history collection"),
        };
        let before = self.generation(&query.session).await?;
        let raw = self.read_codex(query, method, params).await?;
        let generation = self.generation(&query.session).await?;
        ensure!(before == generation, "history changed while reading; retry the window");
        from_codex_response(query, raw, generation)
    }
}

fn legacy_page(query: &WindowQuery, history: Value) -> Result<Value> {
    let turns = history["thread"]["turns"]
        .as_array()
        .context("missing legacy history turns")?;
    let mut entries = Vec::new();
    for turn in turns {
        let turn_id = turn["id"].as_str().context("missing legacy turn id")?;
        if query.group.as_deref().is_some_and(|id| id != turn_id) {
            continue;
        }
        if query.collection == "groups" {
            let mut summary = turn.clone();
            let items = turn["items"].as_array().cloned().unwrap_or_default();
            summary["items"] = if query.projection.as_deref() == Some("notLoaded") {
                json!([])
            } else {
                let first = items.iter().find(|item| item["type"] == "userMessage");
                let last = items
                    .iter()
                    .rev()
                    .find(|item| item["type"] == "agentMessage");
                Value::Array(first.into_iter().chain(last).cloned().collect())
            };
            entries.push((turn_id.to_owned(), summary));
        } else {
            for item in turn["items"].as_array().into_iter().flatten() {
                let id = item["id"].as_str().context("missing legacy item id")?;
                let key = serde_json::to_string(&(turn_id, id))?;
                entries.push((key, json!({"turnId": turn_id, "item": item})));
            }
        }
    }
    if query.projection.as_deref() != Some("asc") {
        entries.reverse();
    }
    let offset = if let Some(cursor) = &query.cursor {
        entries
            .iter()
            .position(|(key, _)| key == cursor)
            .context("invalid history cursor")?
            + 1
    } else {
        0
    };
    let end = (offset + query.limit as usize).min(entries.len());
    let next =
        if end < entries.len() && end > offset { Some(entries[end - 1].0.clone()) } else { None };
    Ok(
        json!({"data": entries[offset..end].iter().map(|(_, value)| value).collect::<Vec<_>>(), "nextCursor": next}),
    )
}

/// Restore the adapter-specific response after generic synchronization.
pub fn codex_response(window: &HistoryWindow, query: &WindowQuery) -> Result<Value> {
    ensure!(window.provider == "codex/app-server-v2", "unsupported history provider");
    if query.collection == "metadata" {
        return window
            .documents
            .get("metadata")
            .cloned()
            .context("missing thread metadata");
    }
    let mut response = window.metadata.clone();
    response["data"] = Value::Array(
        window
            .order
            .iter()
            .map(|id| {
                window
                    .documents
                    .get(id)
                    .cloned()
                    .context("missing window item")
            })
            .collect::<Result<_>>()?,
    );
    Ok(response)
}

fn from_codex_response(
    query: &WindowQuery,
    mut raw: Value,
    generation: String,
) -> Result<HistoryWindow> {
    let mut documents = BTreeMap::new();
    let mut order = Vec::new();
    if query.collection == "metadata" {
        order.push("metadata".into());
        documents.insert("metadata".into(), raw);
        raw = json!({});
    } else {
        let entries = raw
            .as_object_mut()
            .and_then(|v| v.remove("data"))
            .context("missing history page data")?;
        for entry in entries.as_array().context("invalid history page data")? {
            let id = entry
                .get("item")
                .unwrap_or(entry)
                .get("id")
                .and_then(Value::as_str)
                .context("history document has no stable identity")?;
            let key = if query.collection == "items" {
                format!(
                    "{}:{id}",
                    entry
                        .get("turnId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                )
            } else {
                id.to_owned()
            };
            ensure!(
                documents.insert(key.clone(), entry.clone()).is_none(),
                "duplicate history identity"
            );
            order.push(key);
        }
    }
    let window = HistoryWindow {
        provider: "codex/app-server-v2".into(),
        generation,
        metadata: raw,
        order,
        documents,
    };
    window.validate()?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OtherProvider;
    #[async_trait]
    impl SessionHistorySource for OtherProvider {
        fn provider(&self) -> &'static str {
            "example/independent"
        }
        async fn read_window(&self, query: &WindowQuery) -> Result<HistoryWindow> {
            Ok(HistoryWindow {
                provider: self.provider().into(),
                generation: "one".into(),
                metadata: json!({"session": query.session}),
                order: vec!["record".into()],
                documents: BTreeMap::from([("record".into(), json!({"message": "hello"}))]),
            })
        }
    }

    #[tokio::test]
    async fn independent_provider_round_trips_over_real_http() -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server =
            tokio::spawn(
                async move { axum::serve(listener, router(Arc::new(OtherProvider))).await },
            );
        let query = WindowQuery {
            session: "session".into(),
            collection: "messages".into(),
            group: None,
            cursor: None,
            limit: 20,
            projection: None,
        };
        let url = format!("http://{address}/history/v1/window");
        let client = reqwest::Client::new();
        let first: SyncResponse = client
            .post(&url)
            .json(&SyncRequest {
                version: wire::VERSION,
                query: query.clone(),
                known: None,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let local = wire::apply(None, &first)?;
        let unchanged: SyncResponse = client
            .post(url)
            .json(&SyncRequest {
                version: wire::VERSION,
                query,
                known: Some(local.manifest()?),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(unchanged.changes.is_empty());
        assert_eq!(wire::apply(Some(&local), &unchanged)?, local);
        server.abort();
        Ok(())
    }
}

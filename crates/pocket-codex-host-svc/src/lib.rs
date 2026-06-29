//! Host-side **meta service** for Pocket-Codex.
//!
//! A small axum HTTP server, run on the machine that hosts a `codex`
//! app-server, published through the account broker as a third tunnel
//! (`pcx:<device>:meta:<name>`) alongside the host's `app:` and `api:` tunnels.
//! It lets a **remote** client list the host's local `CODEX_HOME` sessions,
//! read transcripts, force-resume a session, and persist per-thread config —
//! the things that previously only worked when the Flutter app ran on the host.
//! See `DESIGN.md` for the rationale and the auth/trust model.
//!
//! Two entry points share one router: [`run`] binds a fresh listener (a CLI
//! worker), and [`serve`] adopts a pre-bound [`TcpListener`] (the in-app host,
//! which binds `127.0.0.1:0` first so it can learn the port it must register).

#![forbid(unsafe_code)]

pub mod resume;
pub mod sessions;
pub mod store;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::net::TcpListener;

use crate::store::{ConfigStore, ThreadConfig};

/// Shared handler state.
struct AppState {
    app_ws_addr: SocketAddr,
    store: Arc<ConfigStore>,
}

/// Bind `listen` and serve the meta service until the process is signalled,
/// opening a fresh thread-config store at `db_path` (the CLI worker path, where
/// there is a single host).
pub async fn run(listen: String, app_ws_addr: SocketAddr, db_path: PathBuf) -> Result<()> {
    let addr: SocketAddr = listen
        .parse()
        .with_context(|| format!("parsing meta service listen address `{listen}`"))?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding meta service on {addr}"))?;
    let store = Arc::new(
        ConfigStore::open(db_path)
            .await
            .context("opening thread-config store")?,
    );
    serve(listener, app_ws_addr, store).await
}

/// Serve the meta service on an already-bound `listener` until the task is
/// dropped. `app_ws_addr` is the colocated app-server (resume target); `store`
/// is shared so multiple colocated hosts on one machine — which share a single
/// `CODEX_HOME` and therefore one per-thread config map — write through one
/// serialized store rather than racing separate files.
pub async fn serve(
    listener: TcpListener,
    app_ws_addr: SocketAddr,
    store: Arc<ConfigStore>,
) -> Result<()> {
    let state = Arc::new(AppState {
        app_ws_addr,
        store,
    });
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}/liveness", get(session_liveness))
        .route("/sessions/{id}/transcript", get(session_transcript))
        .route("/sessions/{id}/resume", post(session_resume))
        .route("/threads/{id}/config", get(get_config).put(put_config))
        .with_state(state);
    axum::serve(listener, app)
        .await
        .context("running meta service")
}

/// An error rendered as `500` with the full anyhow chain in the body. The meta
/// tunnel is reached only by the authenticated account owner, so surfacing the
/// detail aids debugging without leaking to third parties.
struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let msg = format!("{:#}", self.0);
        // Map the two well-known client-input conditions to their proper status
        // (a missing session → 404, a turn running elsewhere → 409) so the
        // contract is correct; everything else is a genuine 500. Matched on the
        // message because the underlying ops return `anyhow` — these substrings
        // are fixed strings in `sessions`/`resume` (keep them in sync).
        let status = if msg.contains("no rollout found") {
            StatusCode::NOT_FOUND
        } else if msg.contains("running in another client") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, msg).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn healthz() -> StatusCode {
    StatusCode::OK
}

/// `{ "sessions": [...] }` — the local session inventory.
#[derive(Serialize)]
struct SessionsResponse {
    sessions: Vec<sessions::LocalSession>,
}

async fn list_sessions() -> Result<Json<SessionsResponse>, ApiError> {
    let sessions = tokio::task::spawn_blocking(sessions::list)
        .await
        .context("session-scan task panicked")??;
    Ok(Json(SessionsResponse {
        sessions,
    }))
}

async fn session_liveness(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<sessions::SessionLiveness>, ApiError> {
    let addr = state.app_ws_addr;
    // protected_pids + the liveness inspect both hit the process table.
    let view = tokio::task::spawn_blocking(move || {
        let protected = resume::protected_pids(addr);
        sessions::liveness(&id, &protected)
    })
    .await
    .context("liveness task panicked")??;
    Ok(Json(view))
}

/// `{ "items": [...] }` — a read-only transcript.
#[derive(Serialize)]
struct TranscriptResponse {
    items: Vec<sessions::TranscriptItem>,
}

async fn session_transcript(Path(id): Path<String>) -> Result<Json<TranscriptResponse>, ApiError> {
    let items = tokio::task::spawn_blocking(move || sessions::transcript(&id))
        .await
        .context("transcript task panicked")??;
    Ok(Json(TranscriptResponse {
        items,
    }))
}

async fn session_resume(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<resume::ForceResumeOutcome>, ApiError> {
    let outcome = resume::force_resume(state.app_ws_addr, &id).await?;
    Ok(Json(outcome))
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<ThreadConfig> {
    Json(state.store.get(&id).await)
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<ThreadConfig>,
) -> Result<Json<ThreadConfig>, ApiError> {
    state.store.put(&id, config).await?;
    // Echo what is actually stored (re-read), not the request body, so the
    // response reflects the persisted state.
    Ok(Json(state.store.get(&id).await))
}

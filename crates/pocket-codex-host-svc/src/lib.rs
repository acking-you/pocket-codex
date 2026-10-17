//! Host-side **meta service** for Pocket-Codex.
//!
//! A small axum HTTP server, run on the machine that hosts a `codex`
//! app-server, published on the relay as a third service (`meta:<name>`)
//! alongside the host's `app:` and `api:` ones.
//! It lets a **remote** client list the host's local `CODEX_HOME` sessions,
//! read transcripts, force-resume a session, and persist per-thread config —
//! the things that previously only worked when the Flutter app ran on the host.
//! See `DESIGN.md` for the rationale and the auth/trust model.
//!
//! Two entry points share one router: [`run`] binds a fresh listener (a CLI
//! worker), and [`serve`] adopts a pre-bound [`TcpListener`] (the in-app host,
//! which binds `127.0.0.1:0` first so it can learn the port it must register).

#![forbid(unsafe_code)]

pub mod acp;
pub mod file_links;
pub mod fs;
pub mod history_sync;
mod history_sync_revision;
pub mod native_history;
pub mod opencode;
pub mod process;
pub mod projects;
pub mod resume;
pub mod sessions;
pub mod store;

mod codex_routes;
mod file_routes;

use std::{convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    codex_routes::{
        list_sessions, read_thread_image, session_follow, session_liveness, session_resume,
        session_transcript,
    },
    file_routes::{list_dir, list_files_in, read_file, upload_file, write_file, UPLOAD_BODY_LIMIT},
    projects::{EnsuredProject, ProjectError, ProjectsUpdate, ProjectsView},
    store::{ConfigStore, HostStore, ThreadConfig},
};

/// Shared handler state.
struct AppState {
    app_ws_addr: SocketAddr,
    store: Arc<ConfigStore>,
    host: Arc<HostStore>,
    /// Upload destination; `None` keeps the Codex default under `CODEX_HOME`.
    uploads_dir: Option<PathBuf>,
    /// Session working directories for `/fs/thread-file`; `None` reads them
    /// from Codex rollouts.
    session_dirs: Option<Arc<dyn file_links::SessionDirResolver>>,
}

/// Bind `listen` and serve the meta service until the process is signalled,
/// opening a fresh thread-config store at `db_path` (the CLI worker path, where
/// there is a single host).
pub async fn run(
    listen: String,
    app_ws_addr: SocketAddr,
    db_path: PathBuf,
    host_config_path: PathBuf,
) -> Result<()> {
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
    let host = Arc::new(
        HostStore::open(host_config_path)
            .await
            .context("opening host-config store")?,
    );
    serve(listener, app_ws_addr, store, host).await
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
    host: Arc<HostStore>,
) -> Result<()> {
    let state = Arc::new(AppState {
        app_ws_addr,
        store,
        host,
        uploads_dir: None,
        session_dirs: None,
    });
    let app = generic_routes()
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}/liveness", get(session_liveness))
        .route("/sessions/{id}/transcript", get(session_transcript))
        .route("/sessions/{id}/follow", get(session_follow))
        .route("/sessions/{id}/resume", post(session_resume))
        .route("/threads/{id}/config", get(get_config).put(put_config))
        .route("/fs/thread-file", get(file_links::read))
        // Inline image previews: not root-confined, but authorised by the
        // thread's own transcript — see `read_thread_image`.
        .route("/fs/thread-image", get(read_thread_image))
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(state)
        .merge(history_sync::router(Arc::new(
            history_sync::CodexHistorySource::new(app_ws_addr),
        )));
    axum::serve(listener, app)
        .await
        .context("running meta service")
}

/// Routes that only touch the host filesystem and host config — no Codex
/// rollout or app-server — shared by every provider's meta service.
fn generic_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/host/device", get(|| async {
            Json(pocket_codex_core::device::DeviceMetadata::current())
        }))
        // Project-folder browser: the configured roots + default, and a
        // root-confined directory listing to drill the host's project tree.
        .route("/projects", get(get_projects).put(put_projects))
        .route("/projects/default/ensure", post(ensure_default_project))
        .route("/fs/list", get(list_dir))
        // File-transfer panel: list the files in a chosen dir, download a
        // file's bytes, upload a local file into a chosen dir — root-confined.
        .route("/fs/files", get(list_files_in))
        .route("/fs/read", get(read_file))
        .route("/host/local-probe", get(file_links::local_probe))
        .route(
            "/fs/write",
            post(write_file).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        // Attachment uploads carry whole files; raise the 2 MB default body cap
        // on this route only.
        .route(
            "/uploads/{name}",
            post(upload_file).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
}

/// Serve the provider-neutral meta service (host files, uploads, project
/// folders) on `listener` — the meta service of an ACP host, which has no
/// Codex rollouts or app-server. Uploads land in `uploads_dir`; conversation
/// links (`/fs/thread-file`) resolve against the session directories reported
/// by `session_dirs`.
pub async fn serve_generic(
    listener: TcpListener,
    store: Arc<ConfigStore>,
    host: Arc<HostStore>,
    uploads_dir: PathBuf,
    session_dirs: Arc<dyn file_links::SessionDirResolver>,
) -> Result<()> {
    let state = Arc::new(AppState {
        // Never read by the generic routes.
        app_ws_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        store,
        host,
        uploads_dir: Some(uploads_dir),
        session_dirs: Some(session_dirs),
    });
    let app = generic_routes()
        .route("/fs/thread-file", get(file_links::read))
        .layer(tower_http::compression::CompressionLayer::new())
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
        if let Some(error) = self.0.downcast_ref::<ProjectError>() {
            let (status, code, current) = match error {
                ProjectError::Conflict {
                    current,
                } => (StatusCode::CONFLICT, "revision_conflict", Some(current.as_ref())),
                ProjectError::Missing(_) => {
                    (StatusCode::NOT_FOUND, "default_project_missing", None)
                },
                ProjectError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_project", None),
            };
            return (status, Json(serde_json::json!({ "code": code, "message": error.to_string(), "current": current }))).into_response();
        }
        let msg = format!("{:#}", self.0);
        // Map the two well-known client-input conditions to their proper status
        // (a missing session → 404, a turn running elsewhere → 409) so the
        // contract is correct; everything else is a genuine 500. Matched on the
        // message because the underlying ops return `anyhow` — these substrings
        // are fixed strings in `sessions`/`resume` (keep them in sync).
        let status =
            if msg.contains("no rollout found") || msg.starts_with(file_links::SESSION_NOT_FOUND) {
                StatusCode::NOT_FOUND
            } else if msg.contains("running in another client") {
                StatusCode::CONFLICT
            } else if msg.contains("outside the configured project roots") {
                StatusCode::FORBIDDEN
            } else if msg.contains("already exists") {
                StatusCode::CONFLICT
            } else if msg.contains("is not a file") {
                StatusCode::BAD_REQUEST
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

/// `GET /projects` — the host's configured project roots + default project.
async fn get_projects(State(state): State<Arc<AppState>>) -> Result<Json<ProjectsView>, ApiError> {
    Ok(Json(state.host.view().await?))
}

/// `PUT /projects` — replace the project roots + default. The desktop host
/// edits this over its own loopback meta tunnel; the result is shared with
/// every device (a phone reads it to seed a new session's folder browser).
async fn put_projects(
    State(state): State<Arc<AppState>>,
    Json(update): Json<ProjectsUpdate>,
) -> Result<Json<ProjectsView>, ApiError> {
    Ok(Json(state.host.update(update).await?))
}

async fn ensure_default_project(
    State(state): State<Arc<AppState>>,
    Json(_): Json<serde_json::Value>,
) -> Result<Json<EnsuredProject>, ApiError> {
    Ok(Json(state.host.ensure_default().await?))
}

/// Original-session access shared by native engines and ACP.
pub mod session_access;

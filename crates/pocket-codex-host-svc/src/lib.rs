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

pub mod fs;
pub mod resume;
pub mod sessions;
pub mod store;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::store::{ConfigStore, HostConfig, HostStore, ThreadConfig};

/// Shared handler state.
struct AppState {
    app_ws_addr: SocketAddr,
    store: Arc<ConfigStore>,
    host: Arc<HostStore>,
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
    });
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}/liveness", get(session_liveness))
        .route("/sessions/{id}/transcript", get(session_transcript))
        .route("/sessions/{id}/resume", post(session_resume))
        .route("/threads/{id}/config", get(get_config).put(put_config))
        // Project-folder browser: the configured roots + default, and a
        // root-confined directory listing to drill the host's project tree.
        .route("/projects", get(get_projects).put(put_projects))
        .route("/fs/list", get(list_dir))
        // Attachment uploads carry whole files; raise the 2 MB default body cap
        // on this route only.
        .route(
            "/uploads/{name}",
            post(upload_file).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
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
        } else if msg.contains("outside the configured project roots") {
            StatusCode::FORBIDDEN
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

/// `GET /projects` — the host's configured project roots + default project.
async fn get_projects(State(state): State<Arc<AppState>>) -> Json<HostConfig> {
    Json(state.host.get().await)
}

/// `PUT /projects` — replace the project roots + default. The desktop host
/// edits this over its own loopback meta tunnel; the result is shared with
/// every device (a phone reads it to seed a new session's folder browser).
async fn put_projects(
    State(state): State<Arc<AppState>>,
    Json(config): Json<HostConfig>,
) -> Result<Json<HostConfig>, ApiError> {
    state.host.put(config).await?;
    Ok(Json(state.host.get().await))
}

/// Query for `GET /fs/list`.
#[derive(Deserialize)]
struct ListDirQuery {
    /// Absolute host path to list. Must be a configured root or inside one.
    path: String,
}

/// `{ "path": ..., "entries": [...] }` — one directory's browsable children.
#[derive(Serialize)]
struct ListDirResponse {
    path: String,
    entries: Vec<fs::DirEntry>,
}

/// `GET /fs/list?path=<abs>` — the sub-directories of `path`, for the remote
/// project-folder browser. Confined to the configured roots: a path that is
/// not a root or inside one is refused (`403`) so the browser can never
/// free-roam the host filesystem.
async fn list_dir(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListDirQuery>,
) -> Result<Json<ListDirResponse>, ApiError> {
    let roots = state.host.get().await.project_roots;
    let requested = std::path::PathBuf::from(&q.path);
    if !fs::within_roots(&requested, &roots) {
        return Err(ApiError(anyhow!("path is outside the configured project roots")));
    }
    let entries = tokio::task::spawn_blocking(move || fs::list_subdirs(&requested))
        .await
        .context("directory-listing task panicked")??;
    Ok(Json(ListDirResponse {
        path: q.path,
        entries,
    }))
}

/// Per-file cap for `/uploads/{name}` bodies. Generous for documents while
/// keeping a runaway request bounded (the tunnel itself has no practical cap).
const UPLOAD_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// `{ "path": ..., "size": ... }` — where an uploaded attachment landed.
#[derive(Serialize)]
struct UploadResponse {
    /// Absolute host filesystem path of the stored file.
    path: String,
    /// Stored size in bytes.
    size: u64,
}

/// Store an uploaded attachment under `$CODEX_HOME/pocket-codex-uploads/` and
/// return its absolute host path. The controller then references that path in
/// the turn text, and the agent reads it with its own tools — codex's native
/// host-file workflow (its input protocol has no document slot). Only the
/// authenticated account owner can reach this tunnel; the filename is
/// sanitized to a single path component regardless.
async fn upload_file(
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<UploadResponse>, ApiError> {
    let home = pocket_codex_codex::rollout::codex_home().context("resolving CODEX_HOME")?;
    let dir = home.join("pocket-codex-uploads");
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating uploads dir {}", dir.display()))?;
    let path = save_upload(&dir, &name, &body).await?;
    Ok(Json(UploadResponse {
        path: path.display().to_string(),
        size: body.len() as u64,
    }))
}

/// Write `bytes` into a fresh per-upload subdirectory of `dir` under the
/// sanitized client-provided `name`; returns the final path. A subdirectory
/// (millisecond timestamp, counter-bumped on collision) rather than a name
/// prefix keeps the file's BASENAME exactly what the user attached — the path
/// is quoted into the prompt and rendered as a chip, so `report.pdf` must not
/// become `1735689…-report.pdf`.
async fn save_upload(
    dir: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, anyhow::Error> {
    let safe = sanitize_file_name(name)?;
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    // CLAIM the subdirectory atomically with `create_dir` (which errors on an
    // existing dir) — a check-then-create would let two same-millisecond
    // uploads share one subdir and silently overwrite same-named files.
    let mut n = 0u32;
    let sub = loop {
        let cand =
            if n == 0 { dir.join(format!("{millis}")) } else { dir.join(format!("{millis}-{n}")) };
        match tokio::fs::create_dir(&cand).await {
            Ok(()) => break cand,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => {
                return Err(e).with_context(|| format!("creating upload dir {}", cand.display()));
            },
        }
    };
    let path = sub.join(safe);
    tokio::fs::write(&path, bytes)
        .await
        .with_context(|| format!("writing upload {}", path.display()))?;
    Ok(path)
}

/// Reduce a client-supplied filename to one safe path component: strips
/// directory separators and traversal, drops characters Windows forbids,
/// trims TRAILING dots/spaces (the Windows-compat problem — leading dots are
/// meaningful dotfile names and survive), sidesteps Windows reserved device
/// names, and caps the length. Errors only when nothing usable remains.
fn sanitize_file_name(name: &str) -> Result<String, anyhow::Error> {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .filter(|c| !c.is_control())
        .collect();
    let trimmed = cleaned.trim_start_matches(' ').trim_end_matches([' ', '.']);
    if trimmed.is_empty() {
        return Err(anyhow!("filename `{name}` has no usable characters"));
    }
    let capped: String = trimmed.chars().take(120).collect();
    // `CON`/`NUL`/`COM1`… (bare or with any extension) open DOS devices instead
    // of files on Windows hosts — the write would vanish into the device.
    // Prefix them so the bytes land on disk.
    let stem = capped.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem[3..].chars().all(|c| c.is_ascii_digit() && c != '0'));
    if reserved {
        return Ok(format!("_{capped}"));
    }
    Ok(capped)
}

#[cfg(test)]
mod upload_tests {
    use super::*;

    #[test]
    fn sanitize_strips_traversal_and_separators() {
        let clean = |n: &str| sanitize_file_name(n).expect("sanitizable name");
        assert_eq!(clean("report.pdf"), "report.pdf");
        // Traversal never escapes the uploads dir: separators are removed and
        // what remains is one path component (leading dots are harmless there).
        assert_eq!(clean("../../etc/passwd"), "....etcpasswd");
        assert_eq!(clean(r"..\..\boot.ini"), "....boot.ini");
        assert_eq!(clean("a:b*c?d\"e<f>g|h.txt"), "abcdefgh.txt");
        // CJK names survive.
        assert_eq!(clean("报告.md"), "报告.md");
        // Dotfiles keep their leading dot (only TRAILING dots/spaces are the
        // Windows-compat problem) — '.env' must not become 'env'.
        assert_eq!(clean(".env"), ".env");
        assert_eq!(clean("notes.txt.  "), "notes.txt");
        // Windows reserved device names are defused with a prefix.
        assert_eq!(clean("nul.txt"), "_nul.txt");
        assert_eq!(clean("COM1"), "_COM1");
        assert_eq!(clean("common.txt"), "common.txt"); // not COM<digit>
                                                       // Nothing usable → error.
        assert!(sanitize_file_name("../..").is_err());
        assert!(sanitize_file_name("").is_err());
    }

    #[tokio::test]
    async fn save_upload_writes_and_never_overwrites() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = save_upload(dir.path(), "notes.txt", b"one")
            .await
            .expect("first");
        let b = save_upload(dir.path(), "notes.txt", b"two")
            .await
            .expect("second");
        assert_ne!(a, b, "same name must not overwrite");
        assert_eq!(std::fs::read(&a).expect("read a"), b"one");
        assert_eq!(std::fs::read(&b).expect("read b"), b"two");
        assert!(a.starts_with(dir.path()) && b.starts_with(dir.path()));
        // The basename stays exactly what was attached — it is quoted into the
        // prompt and rendered as a chip.
        assert_eq!(a.file_name().and_then(|n| n.to_str()), Some("notes.txt"));
        assert_eq!(b.file_name().and_then(|n| n.to_str()), Some("notes.txt"));
    }
}

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

pub mod fs;
pub mod resume;
pub mod sessions;
pub mod store;

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
        .route("/sessions/{id}/follow", get(session_follow))
        .route("/sessions/{id}/resume", post(session_resume))
        .route("/threads/{id}/config", get(get_config).put(put_config))
        // Project-folder browser: the configured roots + default, and a
        // root-confined directory listing to drill the host's project tree.
        .route("/projects", get(get_projects).put(put_projects))
        .route("/fs/list", get(list_dir))
        // File-transfer panel: list the files in a chosen dir, download a
        // file's bytes, upload a local file into a chosen dir — root-confined.
        .route("/fs/files", get(list_files_in))
        .route("/fs/read", get(read_file))
        // Inline image previews: not root-confined, but authorised by the
        // thread's own transcript — see `read_thread_image`.
        .route("/fs/thread-image", get(read_thread_image))
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

/// Follow a rollout over one long-lived response. The filesystem is sampled
/// close to its append cadence on the host, but only changed snapshots cross
/// the relay; liveness is checked separately so a completed writer is noticed
/// even when releasing the file does not append another record.
async fn session_follow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let protected = resume::protected_pids(state.app_ws_addr);
    let initial_id = id.clone();
    let initial_protected = protected.clone();
    let initial = tokio::task::spawn_blocking(move || {
        sessions::follow_update(&initial_id, &initial_protected)
    })
    .await
    .context("session-follow seed task panicked")??;
    let rollout_path = sessions::rollout_path(&id)?;
    let initial_metadata = tokio::fs::metadata(&rollout_path).await?;
    let mut revision = (initial_metadata.len(), initial_metadata.modified().ok());

    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        let mut previous = initial;
        let mut last_sent = match serde_json::to_string(&previous) {
            Ok(encoded) => encoded,
            Err(error) => {
                tracing::warn!(%error, "encoding session follow update failed");
                return;
            },
        };
        if send_follow_data(&tx, last_sent.clone()).await.is_err() {
            return;
        }

        let mut interval = tokio::time::interval(Duration::from_millis(300));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first interval tick is immediate; the seed above already covers
        // it, so consume that tick before entering the change loop.
        interval.tick().await;
        let mut tick = 0_u8;
        loop {
            interval.tick().await;
            tick = tick.wrapping_add(1);
            let metadata = match tokio::fs::metadata(&rollout_path).await {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::debug!(%error, thread_id = %id, "session follow rollout disappeared");
                    break;
                },
            };
            let next_revision = (metadata.len(), metadata.modified().ok());
            let transcript_changed = next_revision != revision;
            let check_liveness = tick % 3 == 0;
            if !transcript_changed && !check_liveness {
                continue;
            }
            let next_id = id.clone();
            let next_protected = protected.clone();
            let next = match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let items = transcript_changed
                    .then(|| sessions::transcript(&next_id))
                    .transpose()?;
                let liveness = check_liveness
                    .then(|| sessions::liveness(&next_id, &next_protected))
                    .transpose()?;
                Ok((items, liveness))
            })
            .await
            {
                Ok(Ok(update)) => update,
                Ok(Err(error)) => {
                    tracing::debug!(%error, thread_id = %id, "session follow stopped");
                    break;
                },
                Err(error) => {
                    tracing::warn!(%error, thread_id = %id, "session follow task panicked");
                    break;
                },
            };
            revision = next_revision;
            if let Some(items) = next.0 {
                previous.items = items;
            }
            if let Some(liveness) = next.1 {
                previous.liveness = liveness;
            }
            let encoded = match serde_json::to_string(&previous) {
                Ok(encoded) => encoded,
                Err(error) => {
                    tracing::warn!(%error, "encoding session follow update failed");
                    break;
                },
            };
            if encoded == last_sent {
                continue;
            }
            last_sent.clone_from(&encoded);
            if send_follow_data(&tx, encoded).await.is_err() {
                break;
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("session-follow"),
    ))
}

async fn send_follow_data(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    data: String,
) -> Result<(), ()> {
    tx.send(Ok(Event::default().event("snapshot").data(data)))
        .await
        .map_err(|_| ())
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

/// `{ "path": ..., "files": [...] }` — one directory's files (not sub-dirs).
#[derive(Serialize)]
struct ListFilesResponse {
    path: String,
    files: Vec<fs::FileEntry>,
}

/// `GET /fs/files?path=<abs>` — the files in `path` (root-confined), for the
/// file-transfer panel. `403` for a path outside the configured roots.
async fn list_files_in(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListDirQuery>,
) -> Result<Json<ListFilesResponse>, ApiError> {
    let roots = state.host.get().await.project_roots;
    let requested = std::path::PathBuf::from(&q.path);
    if !fs::within_roots(&requested, &roots) {
        return Err(ApiError(anyhow!("path is outside the configured project roots")));
    }
    let files = tokio::task::spawn_blocking(move || fs::list_files(&requested))
        .await
        .context("file-listing task panicked")??;
    Ok(Json(ListFilesResponse {
        path: q.path,
        files,
    }))
}

/// `GET /fs/read?path=<file>` — a file's raw bytes (root-confined), to download
/// a host file to the controller. `403` outside roots, `400` if not a file.
/// Reads the whole file into memory: project files are the target, so a range
/// protocol would be premature; the meta tunnel is the only reachable caller.
async fn read_file(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListDirQuery>,
) -> Result<Vec<u8>, ApiError> {
    let roots = state.host.get().await.project_roots;
    let requested = std::path::PathBuf::from(&q.path);
    if !fs::within_roots(&requested, &roots) {
        return Err(ApiError(anyhow!("path is outside the configured project roots")));
    }
    // within_roots already rejects non-existent paths; reject a directory too
    // so the error is honest rather than an opaque read failure.
    if !requested.is_file() {
        return Err(ApiError(anyhow!("path is not a file")));
    }
    let bytes = tokio::fs::read(&requested)
        .await
        .with_context(|| format!("reading {}", requested.display()))?;
    Ok(bytes)
}

/// Query for `GET /fs/thread-image`.
#[derive(Deserialize)]
struct ThreadImageQuery {
    /// Thread whose transcript authorises the read.
    thread: String,
    /// Absolute host path of the image, exactly as the transcript carries it.
    path: String,
}

/// Image suffixes this endpoint will serve. Anything else is a document, and a
/// document is not what a remote controller needs to *look* at.
const IMAGE_SUFFIXES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Largest image this endpoint will inline. Matches the controller's own
/// ceiling, so the tunnel never carries bytes the UI would throw away.
const MAX_INLINE_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// `GET /fs/thread-image?thread=<id>&path=<file>` — an image's bytes, for a
/// remote controller to render inline.
///
/// Deliberately NOT root-confined, because the images worth showing are
/// exactly the ones that are not project files: a pasted screenshot lands in
/// the OS temp directory. The authorisation is narrower instead — the path
/// must already appear in a USER message of the named thread's transcript.
/// A remote controller therefore sees only what this conversation itself put
/// in front of the model, and never gains a general file read:
///
///  * only `userMessage` items count, so a path the model merely *wrote* in a
///    reply is not readable — otherwise a prompt-injected reply could name
///    `~/.ssh/id_rsa` and have the controller fetch it;
///  * only image suffixes are served;
///  * `403` for anything unreferenced, `400` for a non-file.
async fn read_thread_image(Query(q): Query<ThreadImageQuery>) -> Result<Vec<u8>, ApiError> {
    let suffix = std::path::Path::new(&q.path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !suffix.is_some_and(|s| IMAGE_SUFFIXES.contains(&s.as_str())) {
        return Err(ApiError(anyhow!("path is not an image")));
    }
    let thread = q.thread.clone();
    let items = tokio::task::spawn_blocking(move || sessions::transcript(&thread))
        .await
        .context("transcript task panicked")??;
    if !items.iter().any(|i| references_path(i, &q.path)) {
        return Err(ApiError(anyhow!("path is outside the configured project roots")));
    }
    let requested = std::path::PathBuf::from(&q.path);
    if !requested.is_file() {
        return Err(ApiError(anyhow!("path is not a file")));
    }
    // Cap before reading: the controller discards anything it can't draw
    // anyway, so a mis-named huge file would otherwise be pulled into host
    // memory and pushed through the tunnel for nothing.
    let size = tokio::fs::metadata(&requested)
        .await
        .with_context(|| format!("stat {}", requested.display()))?
        .len();
    if size > MAX_INLINE_IMAGE_BYTES {
        return Err(ApiError(anyhow!("path is not a file we will inline")));
    }
    let bytes = tokio::fs::read(&requested)
        .await
        .with_context(|| format!("reading {}", requested.display()))?;
    Ok(bytes)
}

/// Whether `item` is a user message that named `path` — as an attachment or in
/// its text (where a client's IDE-context block lists mentioned files).
/// Separators are normalised because a client may write either style on
/// Windows; nothing else is loosened, since the match IS the authorisation.
fn references_path(item: &sessions::TranscriptItem, path: &str) -> bool {
    if item.item_type != "userMessage" {
        return false;
    }
    let want = path.replace('\\', "/");
    item.images.iter().any(|i| i.replace('\\', "/") == want)
        || mentions_whole_path(&item.text.replace('\\', "/"), &want)
}

/// Whether `text` names exactly `want` — not merely a path it is a prefix of.
/// A bare `contains` would authorise reading `/tmp/a.png` because the message
/// mentioned `/tmp/a.png.bak`, and this match IS the authorisation.
fn mentions_whole_path(text: &str, want: &str) -> bool {
    text.match_indices(want).any(|(at, _)| {
        text[at + want.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '/')))
    })
}

/// Query for `POST /fs/write`.
#[derive(Deserialize)]
struct WriteFileQuery {
    /// Absolute host directory to write into (root-confined).
    dir: String,
    /// Client filename; sanitized to a single path component.
    name: String,
}

/// `POST /fs/write?dir=<abs>&name=<file>` (body = bytes) — upload a local file
/// into a chosen host directory (root-confined). Never overwrites: an existing
/// same-named file yields `409`. `403` for a directory outside the roots.
async fn write_file(
    State(state): State<Arc<AppState>>,
    Query(q): Query<WriteFileQuery>,
    body: axum::body::Bytes,
) -> Result<Json<UploadResponse>, ApiError> {
    let roots = state.host.get().await.project_roots;
    let dir = std::path::PathBuf::from(&q.dir);
    if !fs::within_roots(&dir, &roots) {
        return Err(ApiError(anyhow!("path is outside the configured project roots")));
    }
    let safe = sanitize_file_name(&q.name)?;
    let path = dir.join(&safe);
    // create_new is atomic: an existing same-named file is never silently
    // overwritten — it errors with AlreadyExists, which maps to 409.
    use tokio::io::AsyncWriteExt;
    match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
    {
        Ok(mut f) => f
            .write_all(&body)
            .await
            .with_context(|| format!("writing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ApiError(anyhow!("a file named `{safe}` already exists here")));
        },
        Err(e) => {
            return Err(ApiError(
                anyhow::Error::from(e).context(format!("creating {}", path.display())),
            ));
        },
    }
    Ok(Json(UploadResponse {
        path: path.display().to_string(),
        size: body.len() as u64,
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

    fn item(item_type: &str, text: &str, images: &[&str]) -> sessions::TranscriptItem {
        sessions::TranscriptItem {
            id: "1".into(),
            item_type: item_type.into(),
            title: String::new(),
            text: text.into(),
            images: images.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn thread_image_authorises_only_paths_the_user_sent() {
        // Attached to a user message, or named in its text (a client's
        // IDE-context block) — both are things the user put in front of the
        // model, so both are readable.
        assert!(references_path(&item("userMessage", "", &["/tmp/shot.png"]), "/tmp/shot.png"));
        assert!(references_path(
            &item("userMessage", "## shot.png: /tmp/shot.png", &[]),
            "/tmp/shot.png"
        ));
        // Windows clients write either separator for the same file.
        assert!(references_path(
            &item("userMessage", "", &[r"C:\Temp\shot.png"]),
            "C:/Temp/shot.png"
        ));
    }

    #[test]
    fn thread_image_refuses_paths_the_user_did_not_send() {
        // The critical case: a path the MODEL wrote is not authorisation. A
        // prompt-injected reply naming a private file must not become a read.
        assert!(!references_path(
            &item("agentMessage", "see /home/u/.ssh/id_rsa.png", &[]),
            "/home/u/.ssh/id_rsa.png"
        ));
        assert!(!references_path(
            &item("reasoning", "", &["/home/u/.ssh/id_rsa.png"]),
            "/home/u/.ssh/id_rsa.png"
        ));
        // An unrelated user message authorises nothing.
        assert!(!references_path(&item("userMessage", "look at /tmp/a.png", &[]), "/tmp/b.png"));
        // Nor does one whose path merely STARTS with the requested one — a bare
        // substring match would hand over /tmp/a.png on the strength of a
        // message about /tmp/a.png.bak.
        assert!(!references_path(&item("userMessage", "see /tmp/a.png.bak", &[]), "/tmp/a.png"));
        // The real mention still matches at end-of-text and before punctuation.
        assert!(references_path(&item("userMessage", "see /tmp/a.png", &[]), "/tmp/a.png"));
        assert!(references_path(
            &item("userMessage", "## a.png: /tmp/a.png\n\nwhy?", &[]),
            "/tmp/a.png"
        ));
    }
}

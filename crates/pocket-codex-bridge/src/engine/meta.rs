//! Client side of the host meta service.
//!
//! Reaches a host's `meta:<name>` tunnel — locally over loopback when this app
//! is itself the host, or through the account broker when the host is remote —
//! and calls its HTTP endpoints: remote session inventory, transcript,
//! force-resume, and per-thread config persistence (requirements #5 and #2).
//!
//! Callers pass the app-server `service_key` they are already viewing
//! (`pcx:device:app:name`); the matching `meta` key is derived here, so the UI
//! never has to know the meta tunnel exists.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_account_proto::BoundedRetry;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    fs::{DirEntry, FileEntry},
    resume::ForceResumeOutcome,
    sessions::{LocalSession, SessionLiveness, TranscriptItem},
    store::{HostConfig, ThreadConfig},
};
use reqwest::{Client, Method, Url};
use serde::{de::DeserializeOwned, Deserialize};
use tokio::sync::broadcast;

use crate::engine::{runtime, serve};

/// Per-request timeout: a session scan on a busy host plus a relay hop.
const META_TIMEOUT: Duration = Duration::from_secs(30);

/// One retry in progress, so the UI can say "retrying 2 / 10" instead of
/// looking frozen for the duration of the backoff.
#[derive(Clone, Copy, Debug)]
pub struct RetryProgress {
    /// Attempts made so far (1-based).
    pub attempt: u32,
    /// Total attempt budget.
    pub max_attempts: u32,
}

/// Retry-progress fan-out. Small backlog: a subscriber that misses a tick has
/// already been superseded by a newer one, and the terminal state is the
/// request's own Ok/Err — never a dropped notification.
static RETRIES: OnceCell<broadcast::Sender<RetryProgress>> = OnceCell::new();

fn retries() -> &'static broadcast::Sender<RetryProgress> {
    RETRIES.get_or_init(|| broadcast::channel(16).0)
}

/// Live retry notifications, for the UI's "retrying…" indicator.
pub fn subscribe_retries() -> broadcast::Receiver<RetryProgress> {
    retries().subscribe()
}

fn emit_retry(attempt: u32, max_attempts: u32) {
    // A send with no subscribers is not an error — nobody is watching yet.
    let _ = retries().send(RetryProgress {
        attempt,
        max_attempts,
    });
}

/// The meta client. Built with `.no_proxy()` for the same reason
/// `serve::probe_client` is: every meta request targets LOOPBACK — either this
/// app's own host service, or the local end of a subscribed tunnel — but
/// reqwest honours the process/system proxy by default, and a proxy whose
/// exception list doesn't cover 127.0.0.1 swallows the request. The symptom is
/// indistinguishable from a dead host ("connection closed before message
/// completed"), so it would retry the full budget and still fail.
fn client() -> &'static Client {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(META_TIMEOUT)
            .no_proxy()
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// The `meta` service key for any pocket-codex service key (same device +
/// name).
fn meta_key_of(service_key: &str) -> Result<String> {
    let id = ServiceId::parse_key(service_key)
        .ok_or_else(|| anyhow!("not a pocket-codex service key: {service_key}"))?;
    Ok(ServiceId::new(id.device, ServiceKind::Meta, id.name).key())
}

/// Resolve a service key to a reachable meta base [`Url`]. A meta tunnel hosted
/// by THIS app is served on loopback directly (no relay hop); any other is
/// reached by subscribing to its broker tunnel (account mode).
fn base_url(service_key: &str) -> Result<Url> {
    // Loopback short-circuit only when THIS process actually hosts the viewed
    // app-server — match its app key, not just the derived meta key, so a remote
    // host that happens to share this device id + instance name can't misroute
    // to our local loopback meta service.
    let base = if let Some(addr) = serve::serve_status()
        .into_iter()
        .find(|s| s.app_service_key == service_key)
        .map(|s| s.meta_listen_addr)
    {
        format!("http://{addr}")
    } else {
        let meta_key = meta_key_of(service_key)?;
        let dir = runtime::support_dir()?;
        let sub = runtime::subscribe_account(meta_key, 0, &dir)
            .context("subscribing to the host meta tunnel")?;
        format!("http://{}", sub.local_addr)
    };
    Url::parse(&base).with_context(|| format!("parsing meta base url `{base}`"))
}

/// Build an endpoint URL under the meta base, percent-encoding each segment.
fn endpoint(service_key: &str, segments: &[&str]) -> Result<Url> {
    let mut url = base_url(service_key)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("meta base url cannot be a base"))?
        .extend(segments);
    Ok(url)
}

async fn ensure_ok(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(anyhow!("meta service returned {status}: {body}"))
}

/// Whether a failed send is worth another attempt.
///
/// The meta client holds a pooled keep-alive connection to a service that runs
/// under a supervisor (see `serve::meta_svc_supervisor`), so a restart leaves
/// the pool holding a dead socket: the next request reaches "connection closed
/// before message completed" — sent, never answered. That is a transport
/// hiccup, not an answer, and re-issuing it on a fresh connection succeeds.
///
/// Deliberately narrow. A request that got a real HTTP response (any status)
/// has been answered and is NOT retried here — the host said something, and
/// repeating the question won't change it.
fn is_transient(err: &reqwest::Error) -> bool {
    // `is_request` covers a connection dropped mid-flight; timeouts and connect
    // failures are the other two shapes a restarting service produces.
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Send an IDEMPOTENT request, retrying transient transport failures with the
/// shared bounded-retry policy.
///
/// Only for requests that are safe to repeat: every GET, and the two
/// full-replace PUTs. NOT for `POST /uploads/{name}` (allocates a fresh
/// directory per call, so a retry duplicates the file), `POST /fs/write` (409s
/// on collision, so a retry after a lost response reports a spurious conflict)
/// or `POST /sessions/{id}/resume` (evicts processes). Those stay single-shot
/// by design.
///
/// The URL is rebuilt per attempt by the caller's closure rather than captured,
/// so a retry re-resolves the base — a remote host whose tunnel was
/// re-subscribed lands on the new local port instead of the stale one.
async fn send_idempotent(
    build: impl Fn() -> Result<reqwest::Request>,
) -> Result<reqwest::Response> {
    let mut retry = BoundedRetry::new();
    loop {
        let request = build()?;
        let url = request.url().clone();
        match client().execute(request).await {
            Ok(resp) => return Ok(resp),
            Err(e) if is_transient(&e) => {
                let Some(delay) = retry.fail() else {
                    return Err(anyhow!(e)).with_context(|| {
                        format!("meta request to {url} failed after {} attempts", retry.attempts())
                    });
                };
                // Logged, not silent: a user watching a slow panel should be
                // able to see WHY it is slow in the log viewer, and the attempt
                // counter is what the UI mirrors as "retrying 2/10".
                tracing::warn!(
                    error = %e,
                    %url,
                    attempt = retry.attempts(),
                    max = retry.max_attempts(),
                    "meta request failed transiently; retrying"
                );
                emit_retry(retry.attempts(), retry.max_attempts());
                tokio::time::sleep(delay).await;
            },
            Err(e) => return Err(anyhow!(e)).context("meta request"),
        }
    }
}

async fn get_json<T: DeserializeOwned>(url: Url) -> Result<T> {
    let resp = send_idempotent(|| {
        client()
            .get(url.clone())
            .build()
            .map_err(|e| anyhow!(e).context("building meta GET"))
    })
    .await
    .context("meta GET")?;
    ensure_ok(resp)
        .await?
        .json()
        .await
        .context("decoding meta response")
}

#[derive(Deserialize)]
struct SessionsResponse {
    sessions: Vec<LocalSession>,
}

#[derive(Deserialize)]
struct TranscriptResponse {
    items: Vec<TranscriptItem>,
}

/// List the remote host's local sessions over its meta tunnel.
pub fn sessions(service_key: &str) -> Result<Vec<LocalSession>> {
    let url = endpoint(service_key, &["sessions"])?;
    let resp: SessionsResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp.sessions)
}

/// Inspect one remote session's liveness + would-be takeover targets.
pub fn session_liveness(service_key: &str, thread_id: &str) -> Result<SessionLiveness> {
    let url = endpoint(service_key, &["sessions", thread_id, "liveness"])?;
    runtime::runtime().block_on(get_json(url))
}

/// Read a remote session's transcript for read-only viewing.
pub fn transcript(service_key: &str, thread_id: &str) -> Result<Vec<TranscriptItem>> {
    let url = endpoint(service_key, &["sessions", thread_id, "transcript"])?;
    let resp: TranscriptResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp.items)
}

/// A stored attachment upload, echoed by the host. (The response also carries
/// a `size` field; the caller already knows the byte count it sent, so only
/// the path is modelled.)
#[derive(Deserialize)]
pub struct UploadedFile {
    /// Absolute HOST filesystem path where the file landed.
    pub path: String,
}

/// Big documents over a slow relay hop can outlive [`META_TIMEOUT`]; uploads
/// get their own generous per-request bound instead.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// Upload a document/file attachment to the host behind `service_key`,
/// returning where it landed on the HOST filesystem. The turn text then
/// references that path so the agent reads the file with its own tools —
/// codex's native host-file workflow (its input protocol has no document
/// slot; only images travel inline).
pub fn upload_file(service_key: &str, file_name: &str, bytes: Vec<u8>) -> Result<UploadedFile> {
    let url = endpoint(service_key, &["uploads", file_name])?;
    runtime::runtime().block_on(async move {
        let resp = client()
            .post(url)
            .timeout(UPLOAD_TIMEOUT)
            .body(bytes)
            .send()
            .await
            .context("meta POST upload")?;
        ensure_ok(resp)
            .await?
            .json()
            .await
            .context("decoding upload response")
    })
}

/// Force-resume a remote session into its host's colocated app-server.
pub fn force_resume(service_key: &str, thread_id: &str) -> Result<ForceResumeOutcome> {
    let url = endpoint(service_key, &["sessions", thread_id, "resume"])?;
    runtime::runtime().block_on(async move {
        let resp = client()
            .post(url)
            .send()
            .await
            .context("meta POST resume")?;
        ensure_ok(resp)
            .await?
            .json()
            .await
            .context("decoding resume response")
    })
}

/// Read the host's project-folder config (configured roots + default project)
/// over its meta tunnel — what a new session's folder browser starts from.
pub fn project_config(service_key: &str) -> Result<HostConfig> {
    let url = endpoint(service_key, &["projects"])?;
    runtime::runtime().block_on(get_json(url))
}

/// Replace the host's project-folder config; returns the stored value. The
/// desktop host edits this over its own loopback meta tunnel.
pub fn set_project_config(service_key: &str, config: HostConfig) -> Result<HostConfig> {
    let url = endpoint(service_key, &["projects"])?;
    runtime::runtime().block_on(async move {
        // A full replace, so repeating it is safe: the second write stores the
        // same value the first would have.
        let resp = send_idempotent(|| {
            client()
                .request(Method::PUT, url.clone())
                .json(&config)
                .build()
                .map_err(|e| anyhow!(e).context("building meta PUT projects"))
        })
        .await
        .context("meta PUT projects")?;
        ensure_ok(resp)
            .await?
            .json()
            .await
            .context("decoding projects response")
    })
}

/// `{ "path": ..., "entries": [...] }` — one directory's browsable children.
#[derive(Deserialize)]
struct ListDirResponse {
    entries: Vec<DirEntry>,
}

/// List the sub-directories of `path` on the host, for the remote
/// project-folder browser. The host confines this to its configured roots, so
/// a path outside them errors (surfaced as a `403` in the returned message).
pub fn list_dir(service_key: &str, path: &str) -> Result<Vec<DirEntry>> {
    let mut url = endpoint(service_key, &["fs", "list"])?;
    url.query_pairs_mut().append_pair("path", path);
    let resp: ListDirResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp.entries)
}

/// `{ "path": ..., "files": [...] }` — one directory's files.
#[derive(Deserialize)]
struct ListFilesResponse {
    files: Vec<FileEntry>,
}

/// List the files in `path` on the host (root-confined), for the file-transfer
/// panel. A path outside the configured roots errors (403 in the message).
pub fn list_files(service_key: &str, path: &str) -> Result<Vec<FileEntry>> {
    let mut url = endpoint(service_key, &["fs", "files"])?;
    url.query_pairs_mut().append_pair("path", path);
    let resp: ListFilesResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp.files)
}

/// Download a host file's raw bytes (root-confined) to the controller. Reuses
/// the generous upload timeout since a large file over a relay hop can outlast
/// [`META_TIMEOUT`].
pub fn read_file(service_key: &str, path: &str) -> Result<Vec<u8>> {
    let mut url = endpoint(service_key, &["fs", "read"])?;
    url.query_pairs_mut().append_pair("path", path);
    runtime::runtime().block_on(async move {
        let resp = client()
            .get(url)
            .timeout(UPLOAD_TIMEOUT)
            .send()
            .await
            .context("meta GET read")?;
        let resp = ensure_ok(resp).await?;
        Ok(resp.bytes().await.context("reading file bytes")?.to_vec())
    })
}

/// Read an image the thread's transcript already references, so it can be
/// shown inline. Unlike [`read_file`] this is not root-confined — the host
/// authorises it against the transcript instead (see the host service's
/// `/fs/thread-image`), which is what lets a pasted screenshot in the OS temp
/// directory render on a remote controller.
pub fn read_thread_image(service_key: &str, thread_id: &str, path: &str) -> Result<Vec<u8>> {
    let mut url = endpoint(service_key, &["fs", "thread-image"])?;
    url.query_pairs_mut()
        .append_pair("thread", thread_id)
        .append_pair("path", path);
    runtime::runtime().block_on(async move {
        let resp = client()
            .get(url)
            .timeout(UPLOAD_TIMEOUT)
            .send()
            .await
            .context("meta GET thread-image")?;
        let resp = ensure_ok(resp).await?;
        Ok(resp.bytes().await.context("reading image bytes")?.to_vec())
    })
}

/// Upload local `bytes` as `file_name` into host directory `dir`
/// (root-confined); returns where it landed. Never overwrites (a collision
/// surfaces the host's 409 in the returned message).
pub fn write_file(
    service_key: &str,
    dir: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<UploadedFile> {
    let mut url = endpoint(service_key, &["fs", "write"])?;
    url.query_pairs_mut()
        .append_pair("dir", dir)
        .append_pair("name", file_name);
    runtime::runtime().block_on(async move {
        let resp = client()
            .post(url)
            .timeout(UPLOAD_TIMEOUT)
            .body(bytes)
            .send()
            .await
            .context("meta POST write")?;
        ensure_ok(resp)
            .await?
            .json()
            .await
            .context("decoding write response")
    })
}

/// Read a remote thread's persisted config (all-unset when none stored).
pub fn config_get(service_key: &str, thread_id: &str) -> Result<ThreadConfig> {
    let url = endpoint(service_key, &["threads", thread_id, "config"])?;
    runtime::runtime().block_on(get_json(url))
}

/// Persist a remote thread's config; returns the stored value.
pub fn config_put(
    service_key: &str,
    thread_id: &str,
    config: ThreadConfig,
) -> Result<ThreadConfig> {
    let url = endpoint(service_key, &["threads", thread_id, "config"])?;
    runtime::runtime().block_on(async move {
        // Full replace, so a retry is safe (same reasoning as the projects PUT).
        let resp = send_idempotent(|| {
            client()
                .request(Method::PUT, url.clone())
                .json(&config)
                .build()
                .map_err(|e| anyhow!(e).context("building meta PUT config"))
        })
        .await
        .context("meta PUT config")?;
        ensure_ok(resp)
            .await?
            .json()
            .await
            .context("decoding config response")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_key_derives_from_any_pocket_codex_key() {
        // app / api / meta all map to the same-device, same-name meta key.
        assert_eq!(meta_key_of("pcx:dev:app:work").unwrap(), "pcx:dev:meta:work");
        assert_eq!(meta_key_of("pcx:dev:api:x").unwrap(), "pcx:dev:meta:x");
        assert_eq!(meta_key_of("pcx:dev:meta:y").unwrap(), "pcx:dev:meta:y");
        // A non-pocket-codex key is rejected rather than silently mis-derived.
        assert!(meta_key_of("not-a-key").is_err());
    }

    /// A connection dropped mid-request is exactly what a supervised service
    /// restart looks like from the client's pool: the request went out and no
    /// response came back. Retrying on a fresh connection is what recovers it —
    /// the failure this whole path exists for.
    #[tokio::test]
    async fn idempotent_request_survives_a_dropped_connection() {
        use std::{
            io::{Read, Write},
            sync::{
                atomic::{AtomicU32, Ordering},
                Arc,
            },
        };

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen = Arc::new(AtomicU32::new(0));
        let seen_srv = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let seen = seen_srv.clone();
                // One thread per connection: a peer that never finishes its
                // request must not stall the listener for the retry that follows.
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let n = seen.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        // First attempt: take the request, then hang up without
                        // answering — "connection closed before message
                        // completed", exactly what a restarting service does.
                        return;
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}",
                    );
                    let _ = stream.flush();
                });
            }
        });

        let url: Url = format!("http://{addr}/sessions").parse().expect("url");
        let resp = send_idempotent(|| client().get(url.clone()).build().map_err(|e| anyhow!(e)))
            .await
            .expect("the retry should recover the dropped first attempt");
        assert!(resp.status().is_success());
        assert_eq!(seen.load(Ordering::SeqCst), 2, "one failure, then one success");
    }

    /// The budget is finite: a service that never answers must eventually
    /// surface an error rather than retrying behind a spinner forever.
    #[tokio::test]
    async fn a_persistently_dead_service_gives_up_and_reports_attempts() {
        use std::io::Read;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                drop(stream); // never answers
            }
        });

        let url: Url = format!("http://{addr}/sessions").parse().expect("url");
        // Two attempts rather than the default ten, so the test doesn't sit
        // through the real backoff curve.
        let mut retry = BoundedRetry::with_bounds(2, Duration::from_millis(1));
        let mut attempts = 0;
        let err = loop {
            attempts += 1;
            match client().get(url.clone()).send().await {
                Ok(_) => panic!("the server must not answer"),
                Err(e) if is_transient(&e) => {
                    if retry.fail().is_none() {
                        break e;
                    }
                },
                Err(e) => break e,
            }
        };
        assert!(is_transient(&err));
        assert_eq!(attempts, 2);
        assert_eq!(retry.attempts(), 2);
    }
}

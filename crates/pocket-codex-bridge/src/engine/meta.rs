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

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_account_proto::BoundedRetry;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    fs::{DirEntry, FileEntry},
    resume::ForceResumeOutcome,
    sessions::{LocalSession, SessionFollowUpdate, SessionLiveness, TranscriptItem},
    store::{HostConfig, ThreadConfig},
};
use reqwest::{Client, Method, Url};
use serde::{de::DeserializeOwned, Deserialize};
use tokio::sync::broadcast;

use crate::engine::{runtime, serve};

/// Per-request timeout: a session scan on a busy host plus a relay hop.
const META_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound on a retried request END TO END, attempts and backoff included.
///
/// The attempt cap alone is not a time bound: against a tunnel that accepts the
/// connection but never answers, each attempt can burn the whole
/// [`META_TIMEOUT`], so ten of them would turn a 30-second failure into five
/// minutes of spinner. Retries exist to paper over a service restart — which
/// resolves in milliseconds — not to outlast a network blackhole, so the budget
/// gives up while a user might still be waiting.
const META_TOTAL_DEADLINE: Duration = Duration::from_secs(45);

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

/// A client without an overall response timeout for the long-lived session
/// follow endpoint. Connection establishment is still bounded at the call
/// site; only the healthy response body is allowed to remain open.
fn stream_client() -> &'static Client {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_init(|| {
        Client::builder()
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
    // `is_request` covers a connection dropped before the response; timeouts and
    // connect failures are the other two shapes a restarting service produces.
    //
    // `is_body` / `is_decode` are the same drop happening LATER — headers
    // arrived, then the transfer was cut — which is what a restart looks like on
    // a big transcript. Both are checked because reqwest classifies a truncated
    // body under `is_decode` here (verified against this workspace's version,
    // not assumed from the names).
    //
    // `is_decode` cannot mean "bad JSON" on this path: the retried unit only
    // calls `bytes()`, which never parses — deserialization happens after the
    // retry returns, so a malformed-but-complete body is reported by serde and
    // is never retried. Re-sending wouldn't fix that anyway.
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body() || err.is_decode()
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
///
/// `run` owns the WHOLE exchange, body included. A response resolves as soon as
/// its headers arrive, so reading the body outside the retry would let a
/// connection dropped mid-transfer — the exact failure this exists for, and the
/// likeliest one on a big transcript — escape the budget entirely.
///
/// Bounded by [`META_TOTAL_DEADLINE`] as well as by attempts: against a
/// blackholed tunnel every attempt can burn the full per-request timeout, and
/// ten of those would turn a 30-second failure into a five-minute one.
async fn retry_idempotent<T, F, Fut>(url_for_log: Url, run: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, reqwest::Error>>,
{
    let mut retry = BoundedRetry::new();
    let started = Instant::now();
    loop {
        match run().await {
            Ok(v) => return Ok(v),
            Err(e) if is_transient(&e) => {
                let elapsed = started.elapsed();
                let give_up = retry.fail();
                // Out of attempts, or out of time — whichever comes first. The
                // deadline check uses the time ALREADY spent, so a caller never
                // waits past it just to be told it failed.
                if give_up.is_none() || elapsed >= META_TOTAL_DEADLINE {
                    return Err(anyhow!(e)).with_context(|| {
                        format!(
                            "meta request to {url_for_log} failed after {} attempts ({:.1}s)",
                            retry.attempts(),
                            elapsed.as_secs_f32(),
                        )
                    });
                }
                let delay = give_up.unwrap_or_default();
                // Report the attempt ABOUT TO RUN, not the one that just
                // failed: the UI says "retrying n/10" while it waits, so n has
                // to name the upcoming try or the count reads one behind and
                // never reaches the maximum.
                let next = (retry.attempts() + 1).min(retry.max_attempts());
                tracing::warn!(
                    error = %e,
                    url = %url_for_log,
                    attempt = next,
                    max = retry.max_attempts(),
                    "meta request failed transiently; retrying"
                );
                emit_retry(next, retry.max_attempts());
                tokio::time::sleep(delay).await;
            },
            Err(e) => return Err(anyhow!(e)).context("meta request"),
        }
    }
}

async fn get_json<T: DeserializeOwned>(url: Url) -> Result<T> {
    // Send AND drain inside one retried unit: a body cut short mid-transfer is
    // the same dropped connection as a failed send, and must spend the same
    // budget rather than escaping it.
    //
    // A non-2xx response is NOT retried (the host answered), but its status and
    // body still have to reach the caller — so the outcome is carried out of the
    // retry rather than turned into a reqwest status error, whose message would
    // drop the body that usually says what the host objected to.
    let (status, bytes) = retry_idempotent(url.clone(), || async {
        let resp = client().get(url.clone()).send().await?;
        let status = resp.status();
        Ok((status, resp.bytes().await?))
    })
    .await
    .context("meta GET")?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes);
        return Err(anyhow!("meta service returned {status}: {body}"));
    }
    serde_json::from_slice(&bytes).context("decoding meta response")
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

/// Follow a remote session until the response closes or `on_update` declines
/// another snapshot. The host emits Server-Sent Events only when either the
/// rollout or its ownership changes, so one relay connection replaces client
/// polling while retaining the full-snapshot recovery semantics.
pub async fn follow_session<F>(service_key: &str, thread_id: &str, mut on_update: F) -> Result<()>
where
    F: FnMut(SessionFollowUpdate) -> bool,
{
    let url = endpoint(service_key, &["sessions", thread_id, "follow"])?;
    let response = tokio::time::timeout(META_TIMEOUT, stream_client().get(url.clone()).send())
        .await
        .with_context(|| format!("meta session follow connection to {url} timed out"))?
        .context("opening meta session follow")?;
    let mut response = ensure_ok(response).await?;
    let mut pending = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("reading meta session follow")?
    {
        pending.extend_from_slice(&chunk);
        while let Some(event) = take_sse_event(&mut pending) {
            let Some(update) = decode_follow_event(&event)? else {
                continue;
            };
            if !on_update(update) {
                return Ok(());
            }
        }
    }
    Err(anyhow!("meta session follow ended unexpectedly"))
}

fn take_sse_event(pending: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf = pending.windows(2).position(|window| window == b"\n\n");
    let crlf = pending.windows(4).position(|window| window == b"\r\n\r\n");
    let (at, delimiter_len) = match (lf, crlf) {
        (Some(lf), Some(crlf)) if lf <= crlf => (lf, 2),
        (Some(_), Some(crlf)) => (crlf, 4),
        (Some(lf), None) => (lf, 2),
        (None, Some(crlf)) => (crlf, 4),
        (None, None) => return None,
    };
    let event = pending[..at].to_vec();
    pending.drain(..at + delimiter_len);
    Some(event)
}

fn decode_follow_event(event: &[u8]) -> Result<Option<SessionFollowUpdate>> {
    let text = std::str::from_utf8(event).context("decoding meta session follow event")?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| line.trim_start().trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&data).context("decoding meta session follow snapshot")?))
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
        put_json(url, &config).await.context("meta PUT projects")
    })
}

/// A full-replace PUT plus its JSON response, as ONE retried unit (same
/// send-and-drain reasoning as [`get_json`]).
async fn put_json<B: serde::Serialize, T: DeserializeOwned>(url: Url, body: &B) -> Result<T> {
    let (status, bytes) = retry_idempotent(url.clone(), || async {
        let resp = client()
            .request(Method::PUT, url.clone())
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        Ok((status, resp.bytes().await?))
    })
    .await?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(anyhow!("meta service returned {status}: {text}"));
    }
    serde_json::from_slice(&bytes).context("decoding meta response")
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
        put_json(url, &config).await.context("meta PUT config")
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

    #[test]
    fn session_follow_parser_handles_chunked_events_and_keepalives() {
        let mut pending = b": session-follow\n\ndata: {\"liveness\":{\"thread_id\":\"t1\",\"turn_state\":\"incomplete\",\"held_open\":true,\"safety\":\"ownedRunning\",\"allows_resume\":false,\"requires_takeover\":false,\"holders\":[]},\"items\":[{\"id\":\"i1\",\"item_type\":\"agentMessage\",\"title\":\"\",\"text\":\"hel".to_vec();

        let keepalive = take_sse_event(&mut pending).expect("keepalive frame");
        assert!(decode_follow_event(&keepalive).expect("decode").is_none());
        assert!(take_sse_event(&mut pending).is_none());

        pending.extend_from_slice(b"lo\",\"images\":[]}]}\n\n");
        let event = take_sse_event(&mut pending).expect("completed data frame");
        let update = decode_follow_event(&event)
            .expect("valid snapshot")
            .expect("data event");
        assert_eq!(update.liveness.thread_id, "t1");
        assert_eq!(update.items[0].text, "hello");
        assert!(pending.is_empty());
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
        let (status, _body) = retry_idempotent(url.clone(), || async {
            let resp = client().get(url.clone()).send().await?;
            let status = resp.status();
            Ok((status, resp.bytes().await?))
        })
        .await
        .expect("the retry should recover the dropped first attempt");
        assert!(status.is_success());
        assert_eq!(seen.load(Ordering::SeqCst), 2, "one failure, then one success");
    }

    /// A response whose headers arrive but whose BODY is cut off must spend the
    /// retry budget too — reading the body outside the retried unit let this
    /// escape entirely, and it is the likeliest shape on a large transcript.
    #[tokio::test]
    async fn a_truncated_body_is_retried_like_a_dropped_send() {
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
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let n = seen.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        // Promise 200 bytes, hang up after 5: headers say 200 OK,
                        // so only draining the body reveals the failure.
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 200\r\n\r\n{\"ok\"");
                        let _ = stream.flush();
                        return;
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\
                          connection: close\r\n\r\n{\"ok\":true}",
                    );
                    let _ = stream.flush();
                });
            }
        });

        #[derive(Deserialize)]
        struct Body {
            ok: bool,
        }
        let url: Url = format!("http://{addr}/sessions").parse().expect("url");
        let out: Body = get_json(url)
            .await
            .expect("the truncated body must be retried");
        assert!(out.ok);
        assert_eq!(seen.load(Ordering::SeqCst), 2, "truncated, then complete");
    }

    /// The progress the UI mirrors must name the attempt ABOUT TO RUN: a count
    /// that lags by one never reaches the maximum, so "10/10" would be
    /// unreachable and every number would understate what is happening.
    #[tokio::test]
    async fn retry_progress_names_the_upcoming_attempt() {
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

        let mut rx = subscribe_retries();
        let url: Url = format!("http://{addr}/sessions").parse().expect("url");
        let out: Result<serde_json::Value> = get_json(url).await;
        assert!(out.is_err());

        let first = rx.try_recv().expect("a first retry tick");
        // The first attempt failed, so the NEXT one is #2 — not #1.
        assert_eq!(first.attempt, 2);
        assert_eq!(first.max_attempts, 10);
        let mut last = first;
        while let Ok(p) = rx.try_recv() {
            last = p;
        }
        // And the count reaches the cap rather than stopping at 9.
        assert_eq!(last.attempt, last.max_attempts);
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

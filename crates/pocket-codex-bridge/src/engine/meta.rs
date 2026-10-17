//! Client side of the host meta service.
//!
//! Reaches a host's `meta:<name>` service — locally over loopback when this app
//! is itself the host, or by subscribing to it on the relay when the host is
//! remote — and calls its HTTP endpoints: remote session inventory, transcript,
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

use crate::engine::{
    acp, runtime, serve,
    session_engine::{protocol_of, Protocol},
    transport,
};

#[path = "meta_files.rs"]
mod files;

#[cfg(test)]
use self::files::{download_file, download_response, upload_context_at};
pub use self::files::{
    file_download, file_preview, host_is_local, list_dir, list_files, read_file, read_thread_image,
    upload_context, upload_file, upload_file_scoped, write_file,
};
#[allow(
    unused_imports,
    reason = "the result types of `file_preview` and the uploads keep their `engine::meta` paths \
              from before `meta_files.rs` was split out; callers only read their fields"
)]
pub use self::files::{FilePreview, UploadedFile};

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
pub(super) fn client() -> &'static Client {
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
    Ok(ServiceId::new(
        id.device,
        if id.kind == ServiceKind::Host { ServiceKind::Host } else { ServiceKind::Meta },
        id.name,
    )
    .key())
}

/// A fresh relay inventory can confirm a legacy Codex host has no meta
/// capability. A network failure is not proof of an older host; never silently
/// substitute a different working directory after that failure.
pub fn default_projects_available(service_key: &str) -> Result<bool> {
    if serve::local_endpoints(service_key).is_some() {
        return Ok(true);
    }
    let id = ServiceId::parse_key(service_key).context("invalid service key")?;
    if id.kind != ServiceKind::App {
        return Ok(true);
    }
    let owned = transport::resolve_owned_blocking()?;
    if !transport::Context::of(&owned.transport).admits(service_key) {
        anyhow::bail!("this service belongs to another account or relay");
    }
    let services = runtime::runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), super::discovery::discover(&owned.transport))
            .await
            .context("checking host project capability timed out")?
    })?;
    if !transport::is_current(owned.epoch) {
        anyhow::bail!("connection changed while checking host project capability");
    }
    let available = project_capability(&id, &services, |manager| {
        let device = super::management_client::device(&manager.key())?;
        Ok(device
            .hosts
            .iter()
            .any(|host| super::host_names::same(&host.name, &id.name)))
    })?;
    if !transport::is_current(owned.epoch) {
        anyhow::bail!("connection changed while checking host project capability");
    }
    Ok(available)
}

fn project_capability(
    id: &ServiceId,
    services: &[ServiceId],
    mut managed: impl FnMut(&ServiceId) -> Result<bool>,
) -> Result<bool> {
    if services.iter().any(|service| {
        service.device == id.device && service.kind == ServiceKind::Meta && service.name == id.name
    }) {
        return Ok(true);
    }
    // A CLI host can share this device ID without belonging to the app's
    // manager. Only a confirmed app-owned host must have its meta recovered.
    for manager in services
        .iter()
        .filter(|s| s.device == id.device && s.kind == ServiceKind::Host)
    {
        if managed(manager)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
#[test]
fn only_confirmed_legacy_hosts_skip_default_project_lookup() {
    let app = ServiceId::new("desktop", ServiceKind::App, "old-cli");
    let unexpected = |_: &ServiceId| -> Result<bool> { panic!("unnecessary manager lookup") };
    assert!(!project_capability(&app, std::slice::from_ref(&app), unexpected).expect("legacy"));
    assert!(project_capability(
        &app,
        &[ServiceId::new("desktop", ServiceKind::Meta, "old-cli")],
        unexpected
    )
    .expect("meta"));
    let managers = [ServiceId::new("desktop", ServiceKind::Host, "pocket")];
    assert!(!project_capability(&app, &managers, |_| Ok(false)).expect("independent CLI"));
    assert!(project_capability(&app, &managers, |_| Ok(true)).expect("app-owned host"));
    assert!(project_capability(&app, &managers, |_| anyhow::bail!("offline")).is_err());
    assert!(!project_capability(
        &app,
        &[ServiceId::new("other", ServiceKind::Meta, "old-cli")],
        unexpected
    )
    .expect("other device"));
}

/// Registry-key prefix of the meta tunnels; each is scoped to the transport
/// context it was opened in.
const META_TUNNEL: &str = "meta\u{1f}";

/// Resolve a service key to a reachable meta base [`Url`]. A meta tunnel hosted
/// by THIS app is served on loopback directly (no relay hop); any other is
/// reached by subscribing to it on the relay.
///
/// A relay tunnel belongs to the transport context (account and relay, or
/// relay and key) it was opened in: the same logical key under another
/// context never reuses it, it is dropped when the context changes, and one
/// opened while the context changed is closed again instead of kept. A key
/// namespaced to another account is refused.
fn base_url(service_key: &str) -> Result<Url> {
    base_url_for(service_key, false)
}

fn base_url_for(service_key: &str, direct: bool) -> Result<Url> {
    // Loopback short-circuit only when THIS process actually hosts the viewed
    // app-server — match its app key, not just the derived meta key, so a remote
    // host that happens to share this device id + instance name can't misroute
    // to our local loopback meta service.
    let base = if let Some((_, addr)) = serve::local_endpoints(service_key) {
        format!("http://{addr}")
    } else {
        let meta_key = if direct { service_key.to_string() } else { meta_key_of(service_key)? };
        let owned = transport::resolve_owned_blocking()?;
        let context = transport::Context::of(&owned.transport);
        if !context.admits(service_key) {
            anyhow::bail!("this service belongs to another account or relay");
        }
        let registry = format!("{META_TUNNEL}{}\u{1f}{meta_key}", context.id());
        // Registered only while the context it was resolved in is current;
        // a change drops it (`context_changed`) after the epoch moved.
        let sub = runtime::subscribe_service_as(registry, &meta_key, 0, &owned.transport, || {
            transport::is_current(owned.epoch)
        })
        .context("subscribing to the host meta tunnel")?;
        format!("http://{}", sub.local_addr)
    };
    Url::parse(&base).with_context(|| format!("parsing meta base url `{base}`"))
}

/// The transport context changed: close every meta tunnel of the old one.
pub(super) fn context_changed() {
    runtime::unsubscribe_prefix(META_TUNNEL);
}

/// Build an endpoint URL under the meta base, percent-encoding each segment.
pub(super) fn endpoint(service_key: &str, segments: &[&str]) -> Result<Url> {
    let mut url = base_url(service_key)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("meta base url cannot be a base"))?
        .extend(segments);
    Ok(url)
}

/// A standalone API proxy has no companion meta service, so read its metadata
/// from the API listener itself. All other engines use their meta/host route.
pub(super) fn device_endpoint(service_key: &str) -> Result<Url> {
    let id = ServiceId::parse_key(service_key).context("invalid service key")?;
    let mut url = base_url_for(service_key, id.kind == ServiceKind::Api)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("host URL cannot be a base"))?
        .extend(["host", "device"]);
    Ok(url)
}

async fn ensure_ok(resp: reqwest::Response) -> Result<reqwest::Response> {
    ensure_ok_with_timeout(resp, META_TIMEOUT).await
}

async fn ensure_ok_with_timeout(
    mut resp: reqwest::Response,
    timeout: Duration,
) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    // The streaming client has no overall timeout. Error responses must still
    // finish within a bounded time and cannot consume unbounded memory.
    const ERROR_LIMIT: usize = 64 * 1024;
    let mut body = Vec::new();
    let read = tokio::time::timeout(timeout, async {
        while let Some(chunk) = resp.chunk().await? {
            let take = chunk.len().min(ERROR_LIMIT - body.len());
            body.extend_from_slice(&chunk[..take]);
            if body.len() == ERROR_LIMIT {
                break;
            }
        }
        Ok::<_, reqwest::Error>(())
    })
    .await;
    let detail = match read {
        Err(_) => " (error body timed out)",
        Ok(Err(_)) => " (error body incomplete)",
        Ok(Ok(())) if body.len() == ERROR_LIMIT => " (error body truncated)",
        Ok(Ok(())) => "",
    };
    Err(anyhow!("meta service returned {status}: {}{detail}", String::from_utf8_lossy(&body)))
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
pub fn sessions(service_key: &str, running_only: bool) -> Result<Vec<LocalSession>> {
    let mut url = endpoint(service_key, &["sessions"])?;
    if running_only {
        url.query_pairs_mut().append_pair("running_only", "true");
    }
    let resp: SessionsResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp
        .sessions
        .into_iter()
        .filter(|session| !running_only || session.safety == "ownedRunning")
        .collect())
}

/// Inspect one remote session's liveness + would-be takeover targets.
pub fn session_liveness(service_key: &str, thread_id: &str) -> Result<SessionLiveness> {
    if protocol_of(service_key) == Protocol::OpenCode {
        return super::opencode::liveness(service_key, thread_id);
    }
    if protocol_of(service_key) == Protocol::Acp {
        return acp::native_liveness(service_key, thread_id);
    }
    let url = endpoint(service_key, &["sessions", thread_id, "liveness"])?;
    runtime::runtime().block_on(get_json(url))
}

/// Read a remote session's transcript for read-only viewing.
pub fn transcript(service_key: &str, thread_id: &str) -> Result<Vec<TranscriptItem>> {
    if matches!(protocol_of(service_key), Protocol::Acp | Protocol::OpenCode) {
        return Err(anyhow!("use the paginated native history of the selected ACP host"));
    }
    let url = endpoint(service_key, &["sessions", thread_id, "transcript"])?;
    let resp: TranscriptResponse = runtime::runtime().block_on(get_json(url))?;
    Ok(resp.items)
}

/// Follow a remote session until the response closes or `on_update` declines
/// another snapshot. The host emits Server-Sent Events only when either the
/// rollout or its ownership changes, so one relay connection replaces client
/// polling. New hosts emit revisions for bounded app-server reads; old hosts
/// that ignore the query retain their full-snapshot response semantics.
pub async fn follow_session<F>(service_key: &str, thread_id: &str, mut on_update: F) -> Result<()>
where
    F: FnMut(SessionFollowUpdate) -> bool,
{
    if protocol_of(service_key) == Protocol::OpenCode {
        return super::opencode::follow_session(service_key, thread_id, on_update).await;
    }
    if protocol_of(service_key) == Protocol::Acp {
        return acp::follow_native_session(service_key, thread_id, on_update).await;
    }
    // Transport resolution and tunnel readiness use the synchronous FRB path.
    // Keep their block_on calls off the async worker that consumes the SSE body.
    let key = service_key.to_owned();
    let thread = thread_id.to_owned();
    let mut url =
        tokio::task::spawn_blocking(move || endpoint(&key, &["sessions", &thread, "follow"]))
            .await
            .context("resolving session follow endpoint")??;
    url.query_pairs_mut().append_pair("metadata_only", "true");
    let response = tokio::time::timeout(META_TIMEOUT, stream_client().get(url.clone()).send())
        .await
        .with_context(|| format!("meta session follow connection to {url} timed out"))?
        .context("opening meta session follow")?;
    let mut response = ensure_ok(response).await?;
    let mut pending = Vec::new();
    let mut version = None;
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
            let next_version =
                (update.history_revision.clone(), update.liveness.turn_state.clone());
            // Liveness is sampled separately from the rollout revision. A
            // terminal state must invalidate an earlier tail at the same revision.
            if update.history_revision.is_some() && version.as_ref() != Some(&next_version) {
                super::app_session::external_history_changed(service_key, thread_id);
            }
            version = Some(next_version);
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

/// Force-resume a remote session into its host's colocated app-server.
pub fn force_resume(service_key: &str, thread_id: &str) -> Result<ForceResumeOutcome> {
    if matches!(protocol_of(service_key), Protocol::Acp | Protocol::OpenCode) {
        return Err(anyhow!(
            "this provider has no verified safe takeover operation; continue viewing read-only"
        ));
    }
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
#[path = "meta_tests.rs"]
mod tests;

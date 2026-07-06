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
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    fs::{DirEntry, FileEntry},
    resume::ForceResumeOutcome,
    sessions::{LocalSession, SessionLiveness, TranscriptItem},
    store::{HostConfig, ThreadConfig},
};
use reqwest::{Client, Method, Url};
use serde::{de::DeserializeOwned, Deserialize};

use crate::engine::{runtime, serve};

/// Per-request timeout: a session scan on a busy host plus a relay hop.
const META_TIMEOUT: Duration = Duration::from_secs(30);

fn client() -> &'static Client {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(META_TIMEOUT)
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

async fn get_json<T: DeserializeOwned>(url: Url) -> Result<T> {
    let resp = client().get(url).send().await.context("meta GET")?;
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
        let resp = client()
            .request(Method::PUT, url)
            .json(&config)
            .send()
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
        let resp = client()
            .request(Method::PUT, url)
            .json(&config)
            .send()
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
}

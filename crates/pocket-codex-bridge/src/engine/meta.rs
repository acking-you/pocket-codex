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
    resume::ForceResumeOutcome,
    sessions::{LocalSession, SessionLiveness, TranscriptItem},
    store::ThreadConfig,
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

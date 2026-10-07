//! Dictation: speech in, text out, for the composer's microphone.
//!
//! The recording goes to the host's API proxy (`api:<name>`), which forwards
//! it to ChatGPT's `/transcribe` endpoint signed with the host's Codex login —
//! the same endpoint the Codex desktop app's composer microphone uses. This
//! app never holds that credential; it only reaches the proxy, on loopback
//! when it is the host and over the relay otherwise.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use pocket_codex_core::service::{ServiceId, ServiceKind};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::engine::{runtime, serve, transport};

/// One recording plus the upload and the backend's transcription.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// The `api` service key for any pocket-codex service key (same device and
/// name).
fn api_key_of(service_key: &str) -> Result<String> {
    let id = ServiceId::parse_key(service_key)
        .ok_or_else(|| anyhow!("not a pocket-codex service key: {service_key}"))?;
    Ok(ServiceId::new(id.device, ServiceKind::Api, id.name).key())
}

fn proxy_base(service_key: &str) -> Result<String> {
    if let Some(addr) = serve::local_api_endpoint(service_key) {
        return Ok(format!("http://{addr}"));
    }
    let sub = runtime::subscribe_service(
        api_key_of(service_key)?,
        0,
        &transport::resolve_blocking()?,
    )
    .context("subscribing to the host API tunnel")?;
    Ok(format!("http://{}", sub.local_addr))
}

/// Loopback-only, like the meta client: a system proxy must not intercept a
/// request bound for 127.0.0.1.
fn client() -> &'static Client {
    static CLIENT: once_cell::sync::OnceCell<Client> = once_cell::sync::OnceCell::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(TRANSCRIBE_TIMEOUT)
            .no_proxy()
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

#[derive(Deserialize)]
struct Transcription {
    #[serde(default)]
    text: String,
}

/// Transcribe one recording on the host behind `service_key`.
///
/// `mime` is the audio's type (e.g. `audio/mp4`), `file_name` carries the
/// extension the backend reads the format from, and `language` is an optional
/// BCP-47 hint (none lets the backend detect it).
pub fn transcribe(
    service_key: &str,
    audio: Vec<u8>,
    mime: &str,
    file_name: &str,
    language: Option<&str>,
) -> Result<String> {
    if audio.is_empty() {
        bail!("the recording is empty");
    }
    let mut url = Url::parse(&proxy_base(service_key)?)
        .context("parsing the API proxy address")?
        .join("/v1/transcribe")?;
    url.query_pairs_mut().append_pair("filename", file_name);
    if let Some(language) = language.filter(|l| !l.trim().is_empty()) {
        url.query_pairs_mut().append_pair("language", language.trim());
    }
    let mime = mime.to_string();
    runtime::runtime().block_on(async move {
        let response = client()
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, mime)
            .body(audio)
            .send()
            .await
            .context("sending the recording to the host")?;
        let status = response.status();
        let body = response.text().await.context("reading the transcription")?;
        if !status.is_success() {
            // A host without the route predates dictation; say so plainly.
            if status == reqwest::StatusCode::FORBIDDEN {
                bail!("this host does not support dictation yet; update Pocket-Codex on it");
            }
            let detail: String = body.chars().take(300).collect();
            bail!("transcription failed ({status}): {detail}");
        }
        let parsed: Transcription =
            serde_json::from_str(&body).context("the transcription response was not JSON")?;
        Ok(parsed.text.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_follows_the_viewed_service() {
        assert_eq!(api_key_of("pcx:dev:app:work").unwrap(), "pcx:dev:api:work");
        assert!(api_key_of("nonsense").is_err());
    }
}

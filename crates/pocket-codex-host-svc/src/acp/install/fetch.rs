//! HTTPS downloads with an allowlist and SRI verification (TRD §4.3.4).

use std::{path::Path, time::Duration};

use base64::Engine as _;
use sha2::{Digest, Sha256, Sha512};
use tokio::io::AsyncWriteExt;
use url::Url;

use super::super::error::AcpError;

/// Hosts downloads may come from (including redirects).
pub const ALLOWED_HOSTS: &[&str] = &[
    "nodejs.org",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "registry.npmjs.org",
    "cdn.agentclientprotocol.com",
];
/// Download size limit.
pub const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Where downloads may come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchPolicy {
    /// Allowed host names.
    pub allowed_hosts: Vec<String>,
    /// Size limit.
    pub max_bytes: u64,
    /// Tests only: allow `http://127.0.0.1:<port>`.
    pub allow_loopback_http: bool,
}

impl FetchPolicy {
    /// The production policy (HTTPS, [`ALLOWED_HOSTS`], 1 GiB).
    pub fn production() -> Self {
        Self {
            allowed_hosts: ALLOWED_HOSTS.iter().map(|h| h.to_string()).collect(),
            max_bytes: MAX_DOWNLOAD_BYTES,
            allow_loopback_http: false,
        }
    }

    /// Whether `url` may be fetched.
    pub fn allows(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else { return false };
        match url.scheme() {
            "https" => self.allowed_hosts.iter().any(|h| h == host),
            "http" => self.allow_loopback_http && matches!(host, "127.0.0.1" | "localhost"),
            _ => false,
        }
    }
}

enum Hasher {
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hasher {
    fn for_integrity(integrity: &str) -> Result<(Self, &str), AcpError> {
        if let Some(expected) = integrity.strip_prefix("sha256-") {
            Ok((Self::Sha256(Sha256::new()), expected))
        } else if let Some(expected) = integrity.strip_prefix("sha512-") {
            Ok((Self::Sha512(Sha512::new()), expected))
        } else {
            Err(AcpError::IntegrityMismatch(format!("unsupported integrity `{integrity}`")))
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha256(h) => h.update(bytes),
            Self::Sha512(h) => h.update(bytes),
        }
    }

    fn finish(self) -> String {
        let digest = match self {
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        };
        base64::engine::general_purpose::STANDARD.encode(digest)
    }
}

/// SRI string of `bytes`: `sha512-…` for `"sha512"`, otherwise `sha256-…`.
pub fn sri(algorithm: &str, bytes: &[u8]) -> String {
    let b64 = |digest: &[u8]| base64::engine::general_purpose::STANDARD.encode(digest);
    match algorithm {
        "sha512" => format!("sha512-{}", b64(&Sha512::digest(bytes))),
        _ => format!("sha256-{}", b64(&Sha256::digest(bytes))),
    }
}

/// Download `url` to `dest`, verifying `integrity` while streaming.
pub async fn download(
    url: &str,
    integrity: &str,
    dest: &Path,
    policy: &FetchPolicy,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<(), AcpError> {
    let parsed = Url::parse(url).map_err(|e| AcpError::DownloadFailed(format!("{url}: {e}")))?;
    if !policy.allows(&parsed) {
        return Err(AcpError::DownloadFailed(format!("{url} is not an allowed download source")));
    }
    let (mut hasher, expected) = Hasher::for_integrity(integrity)?;
    let redirect_policy = policy.clone();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if redirect_policy.allows(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirected to a host that is not allowed")
            }
        }))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| AcpError::DownloadFailed(e.to_string()))?;
    let mut response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| AcpError::DownloadFailed(format!("{url}: {e:#}")))?;
    if !response.status().is_success() {
        return Err(AcpError::DownloadFailed(format!("{url}: HTTP {}", response.status())));
    }
    let total = response.content_length();
    if total.is_some_and(|t| t > policy.max_bytes) {
        return Err(AcpError::DownloadFailed(format!("{url} is larger than the limit")));
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(dest).await?;
    let mut bytes = 0u64;
    let result: Result<(), AcpError> = async {
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AcpError::DownloadFailed(format!("{url}: {e}")))?
        {
            bytes += chunk.len() as u64;
            if bytes > policy.max_bytes {
                return Err(AcpError::DownloadFailed(format!("{url} is larger than the limit")));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            progress(bytes, total);
        }
        file.flush().await?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(e);
    }
    let actual = hasher.finish();
    if actual != expected {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(AcpError::IntegrityMismatch(format!(
            "{url}: expected {integrity}, got {}-{actual}",
            integrity.split('-').next().unwrap_or_default()
        )));
    }
    Ok(())
}

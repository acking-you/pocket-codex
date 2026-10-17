//! Bounded controller requests. Mutations are never transparently retried:
//! a lost response means the controller must refresh the actual host state.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use once_cell::sync::OnceCell;
use reqwest::{Client, Method};
use serde::{de::DeserializeOwned, Serialize};

use super::{management, meta, runtime, transport};

const LIMIT: usize = 2 * 1024 * 1024;

fn client() -> Result<&'static Client> {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_try_init(|| {
        Client::builder()
            .no_proxy()
            // A management body can contain write-only credentials. Never
            // resend it to a redirect target or replay a failed mutation.
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(120))
            .build()
            .context("initializing device management client")
    })
}

/// A bounded, server-classified failure; response bytes are never exposed.
#[derive(Debug)]
pub struct RemoteError {
    /// HTTP status, for legacy-route capability checks.
    pub status: u16,
    /// Stable safe failure category.
    pub code: String,
    /// Bounded display explanation.
    pub message: String,
}
impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.code, self.status, self.message)
    }
}
impl std::error::Error for RemoteError {}

pub(crate) fn request<T: DeserializeOwned, B: Serialize + ?Sized>(
    key: &str,
    method: Method,
    segments: &[&str],
    body: Option<&B>,
) -> Result<T> {
    let epoch = transport::epoch();
    let url = meta::endpoint(key, segments)?;
    if !transport::is_current(epoch) {
        bail!("connection changed; refresh the host");
    }
    runtime::runtime().block_on(request_at(url.into(), method, body, epoch))
}

async fn request_at<T: DeserializeOwned, B: Serialize + ?Sized>(
    url: String,
    method: Method,
    body: Option<&B>,
    epoch: u64,
) -> Result<T> {
    let mut request = client()?.request(method.clone(), url);
    if let Some(body) = body {
        request = request.json(body);
    } else if method == Method::POST {
        request = request.json(&serde_json::json!({}));
    }
    let mut response = request
        .send()
        .await
        .context("host request failed; refresh its state before retrying")?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("host response interrupted; refresh its state")?
    {
        if bytes.len().saturating_add(chunk.len()) > LIMIT {
            bail!("host response exceeded the size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    if !transport::is_current(epoch) {
        bail!("connection changed; discard the old host response");
    }
    if !status.is_success() {
        #[derive(serde::Deserialize)]
        struct Error {
            code: Option<String>,
            message: Option<String>,
        }
        let error = serde_json::from_slice::<Error>(&bytes).ok();
        let message = error
            .as_ref()
            .and_then(|e| e.message.as_deref())
            .unwrap_or("Host operation failed; refresh its state.");
        let code = error
            .as_ref()
            .and_then(|e| e.code.as_deref())
            .unwrap_or("host_error");
        return Err(RemoteError {
            status: status.as_u16(),
            code: code.chars().filter(|c| !c.is_control()).take(80).collect(),
            message: message
                .chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .take(600)
                .collect(),
        }
        .into());
    }
    serde_json::from_slice(&bytes).context("decoding host response")
}

fn check_key(key: &str) -> Result<()> {
    let id = pocket_codex_core::service::ServiceId::parse_key(key)
        .context("invalid device management key")?;
    if id.kind != pocket_codex_core::service::ServiceKind::Host {
        bail!("this is not a device management service");
    }
    Ok(())
}

/// Read publisher metadata without inferring capabilities from its label.
pub fn device_metadata(key: &str) -> Result<Option<pocket_codex_core::device::DeviceMetadata>> {
    let epoch = transport::epoch();
    let url = meta::device_endpoint(key)?;
    if !transport::is_current(epoch) {
        bail!("connection changed; refresh the host");
    }
    metadata_result(runtime::runtime().block_on(request_at::<_, ()>(
        url.into(),
        Method::GET,
        None,
        epoch,
    )))
}

fn metadata_result(
    response: Result<pocket_codex_core::device::DeviceMetadata>,
) -> Result<Option<pocket_codex_core::device::DeviceMetadata>> {
    match response {
        Ok(value) if value.protocol == 1 => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(error)
            if error
                .downcast_ref::<RemoteError>()
                .is_some_and(|error| matches!(error.status, 404 | 405)) =>
        {
            Ok(None)
        },
        Err(error) => Err(error),
    }
}

/// Read the selected computer's management state.
pub fn device(key: &str) -> Result<management::DeviceView> {
    check_key(key)?;
    let view: management::DeviceView =
        request(key, Method::GET, &["manage", "v1", "device"], None::<&()>)?;
    if view.protocol != 1 {
        bail!("update Pocket Codex to manage this computer");
    }
    Ok(view)
}

/// Read one managed host.
pub fn host(key: &str, name: &str) -> Result<management::HostView> {
    check_key(key)?;
    request(key, Method::GET, &["manage", "v1", "hosts", name], None::<&()>)
}

/// Save the reviewed configuration, with conflict protection.
pub fn save(key: &str, name: &str, body: &management::SaveRequest) -> Result<management::HostView> {
    check_key(key)?;
    request(key, Method::PUT, &["manage", "v1", "hosts", name], Some(body))
}

/// Create a host using a host-side runtime choice.
pub fn start(key: &str, body: &management::StartRequest) -> Result<management::HostView> {
    check_key(key)?;
    request(key, Method::POST, &["manage", "v1", "hosts"], Some(body))
}

/// Apply the saved configuration to the exact reviewed incarnation.
pub fn apply(
    key: &str,
    name: &str,
    body: &management::ApplyRequest,
) -> Result<super::host_apply::ApplyReport> {
    check_key(key)?;
    request(key, Method::POST, &["manage", "v1", "hosts", name, "apply"], Some(body))
}

/// Stop only the reviewed incarnation; does not withdraw device management.
pub fn stop(key: &str, name: &str, body: &management::StopRequest) -> Result<()> {
    check_key(key)?;
    request(key, Method::POST, &["manage", "v1", "hosts", name, "stop"], Some(body))
}

/// Detect installed runtimes on the selected computer, without installing.
pub fn runtimes(key: &str) -> Result<Vec<super::management_runtimes::Availability>> {
    check_key(key)?;
    request(key, Method::GET, &["manage", "v1", "runtimes"], None::<&()>)
}

/// Start preparation only on an explicit action; observing progress is
/// read-only.
pub fn claude_prepare(key: &str, start: bool) -> Result<super::management_runtimes::Preparation> {
    check_key(key)?;
    request(
        key,
        if start { Method::POST } else { Method::GET },
        &["manage", "v1", "claude", "prepare"],
        None::<&()>,
    )
}

/// Start the exact saved configuration without exposing its private fields.
pub fn start_saved(key: &str, name: &str, revision: u64) -> Result<management::HostView> {
    check_key(key)?;
    request(
        key,
        Method::POST,
        &["manage", "v1", "hosts", name, "start"],
        Some(&management::StartSavedRequest {
            expected_revision: revision,
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use axum::{
        http::{header, StatusCode},
        routing::{get, post},
        Router,
    };
    use serde_json::{json, Value};

    use super::*;

    async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (url, task)
    }

    #[tokio::test]
    async fn missing_metadata_is_distinct_from_outages_and_invalid_responses() {
        let (url, task) = serve(
            Router::new()
                .route("/method", get(|| async { StatusCode::METHOD_NOT_ALLOWED }))
                .route("/offline", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
                .route("/invalid", get(|| async { "not JSON" }))
                .route(
                    "/future",
                    get(|| async {
                        let mut value = pocket_codex_core::device::DeviceMetadata::current();
                        value.protocol = 2;
                        axum::Json(value)
                    }),
                ),
        )
        .await;
        for path in ["missing", "method", "future", "offline", "invalid"] {
            let result = metadata_result(
                request_at::<_, ()>(format!("{url}/{path}"), Method::GET, None, transport::epoch())
                    .await,
            );
            if matches!(path, "offline" | "invalid") {
                assert!(result.is_err(), "{path} is not an older host");
            } else {
                assert_eq!(result.expect("unsupported metadata"), None, "{path}");
            }
        }
        task.abort();
    }

    #[tokio::test]
    async fn mutations_do_not_follow_redirects_or_retry_failures() {
        let received = Arc::new(AtomicUsize::new(0));
        let sink_count = received.clone();
        let (sink, sink_task) = serve(Router::new().route(
            "/",
            post(move || {
                let count = sink_count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    "null"
                }
            }),
        ))
        .await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let failed_count = attempts.clone();
        let (url, task) =
            serve(
                Router::new()
                    .route(
                        "/redirect",
                        post(move || {
                            let location = sink.clone();
                            async move {
                                (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, location)])
                            }
                        }),
                    )
                    .route(
                        "/fail",
                        post(move || {
                            let count = failed_count.clone();
                            async move {
                                count.fetch_add(1, Ordering::SeqCst);
                                (StatusCode::SERVICE_UNAVAILABLE, "untrusted secret diagnostic")
                            }
                        }),
                    ),
            )
            .await;
        let body = json!({"apiKey": "must-never-be-forwarded"});
        for path in ["redirect", "fail"] {
            let error = request_at::<Value, _>(
                format!("{url}/{path}"),
                Method::POST,
                Some(&body),
                transport::epoch(),
            )
            .await
            .expect_err("failure");
            assert!(error.downcast_ref::<RemoteError>().is_some());
            assert!(!error.to_string().contains("secret"));
        }
        assert_eq!(received.load(Ordering::SeqCst), 0);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        task.abort();
        sink_task.abort();
    }

    #[tokio::test]
    async fn oversized_and_old_context_responses_cannot_be_used() {
        let (url, task) = serve(
            Router::new()
                .route("/large", get(|| async { "x".repeat(LIMIT + 1) }))
                .route("/normal", get(|| async { "null" })),
        )
        .await;
        let large =
            request_at::<Value, ()>(format!("{url}/large"), Method::GET, None, transport::epoch())
                .await
                .expect_err("bounded");
        assert!(large.to_string().contains("size limit"));
        let old = request_at::<Value, ()>(
            format!("{url}/normal"),
            Method::GET,
            None,
            transport::epoch().wrapping_sub(1),
        )
        .await
        .expect_err("stale response");
        assert!(old.to_string().contains("connection changed"));
        task.abort();
    }
}

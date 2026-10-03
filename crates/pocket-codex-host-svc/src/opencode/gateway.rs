//! Loopback reverse proxy that publishes an attached OpenCode server.
//!
//! The relay reaches the host through this gateway only. It forwards an
//! explicit allowlist of session-control routes, injects the server's Basic
//! credentials (which never leave this process), streams responses (including
//! the SSE event stream) unbuffered, and answers everything else with 404.
//! Credential, integration, PTY, shell, config, filesystem and experimental
//! routes are never reachable through it.

use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use tokio::net::TcpListener;
use url::Url;

use super::{BasicCredentials, Error, Result};

/// Largest forwarded request body.
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Routes forwarded to the server: (method, OpenAPI-style path template).
pub const ALLOWED_ROUTES: &[(&str, &str)] = &[
    ("GET", "/openapi.json"),
    ("GET", "/api/info"),
    ("GET", "/api/event"),
    ("GET", "/api/session"),
    ("POST", "/api/session"),
    ("GET", "/api/session/active"),
    ("GET", "/api/session/{id}"),
    ("PATCH", "/api/session/{id}"),
    ("GET", "/api/session/{id}/message"),
    ("GET", "/api/session/{id}/message/{id}"),
    ("GET", "/api/session/{id}/diff"),
    ("POST", "/api/session/{id}/prompt"),
    ("POST", "/api/session/{id}/interrupt"),
    ("POST", "/api/session/{id}/compact"),
    ("POST", "/api/session/{id}/model"),
    ("POST", "/api/session/{id}/agent"),
    ("GET", "/api/session/{id}/permission"),
    ("POST", "/api/session/{id}/permission/{id}/reply"),
    ("GET", "/api/session/{id}/form"),
    ("POST", "/api/session/{id}/form/{id}/reply"),
    ("DELETE", "/api/session/{id}/form/{id}"),
    ("GET", "/api/project"),
    ("GET", "/api/model"),
    ("GET", "/api/agent"),
    ("GET", "/api/vcs/diff"),
];

/// Where the gateway forwards to. Replaceable at runtime, e.g. after OpenCode
/// restarted its background service on a new port or password.
#[derive(Clone)]
pub struct Upstream {
    origin: Url,
    credentials: Option<BasicCredentials>,
}

impl std::fmt::Debug for Upstream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Upstream([redacted])")
    }
}

impl Upstream {
    /// A loopback upstream origin with optional credentials.
    pub fn new(origin: &str, credentials: Option<BasicCredentials>) -> Result<Self> {
        Ok(Self {
            origin: super::validate_origin(origin, true)?,
            credentials,
        })
    }
}

/// Shared gateway state; clone to keep a handle for [`Gateway::set_upstream`].
#[derive(Clone)]
pub struct Gateway {
    upstream: Arc<RwLock<Upstream>>,
    http: reqwest::Client,
}

impl Gateway {
    /// Create a gateway for `upstream`.
    pub fn new(upstream: Upstream) -> Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            upstream: Arc::new(RwLock::new(upstream)),
            http,
        })
    }

    /// Point subsequent requests at a new upstream.
    pub fn set_upstream(&self, upstream: Upstream) {
        match self.upstream.write() {
            Ok(mut guard) => *guard = upstream,
            Err(poisoned) => *poisoned.into_inner() = upstream,
        }
    }

    fn current(&self) -> Upstream {
        match self.upstream.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The axum router serving this gateway.
    pub fn router(&self) -> Router {
        Router::new().fallback(forward).with_state(self.clone())
    }
}

/// Serve `gateway` on an already-bound loopback `listener` until dropped.
pub async fn serve(listener: TcpListener, gateway: Gateway) -> anyhow::Result<()> {
    let local: SocketAddr = listener.local_addr()?;
    if !local.ip().is_loopback() {
        anyhow::bail!("the OpenCode gateway only listens on loopback");
    }
    axum::serve(listener, gateway.router()).await?;
    Ok(())
}

/// Whether `method path` is forwarded.
pub fn allowed(method: &Method, path: &str) -> bool {
    ALLOWED_ROUTES
        .iter()
        .any(|(m, template)| *m == method.as_str() && matches_template(template, path))
}

fn matches_template(template: &str, path: &str) -> bool {
    let mut want = template.split('/');
    let mut have = path.split('/');
    loop {
        match (want.next(), have.next()) {
            (None, None) => return true,
            (Some("{id}"), Some(segment)) => {
                if super::validate_id(segment).is_err() {
                    return false;
                }
            },
            (Some(a), Some(b)) if a == b => {},
            _ => return false,
        }
    }
}

async fn forward(
    State(gateway): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = uri.path();
    if !allowed(&method, path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let body = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let upstream = gateway.current();
    let Ok(mut url) = upstream.origin.join(path.trim_start_matches('/')) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    url.set_query(uri.query());
    let Ok(reqwest_method) = reqwest::Method::from_bytes(method.as_str().as_bytes()) else {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    };
    let streaming = path == "/api/event";
    let mut request = gateway.http.request(reqwest_method, url);
    if !streaming {
        request = request.timeout(Duration::from_secs(120));
    }
    for name in [header::CONTENT_TYPE, header::ACCEPT] {
        if let Some(value) = headers.get(&name).and_then(|v| v.to_str().ok()) {
            request = request.header(name.as_str(), value);
        }
    }
    if let Some(credentials) = &upstream.credentials {
        request = credentials.apply(request);
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    // Never relay the server's own authentication challenge to the controller.
    let status = if status == StatusCode::UNAUTHORIZED { StatusCode::BAD_GATEWAY } else { status };
    let mut builder = Response::builder().status(status);
    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
        if let Some(value) = response
            .headers()
            .get(name.as_str())
            .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok())
        {
            builder = builder.header(name, value);
        }
    }
    if streaming {
        builder = builder.header("x-accel-buffering", "no");
    }
    builder
        .body(Body::from_stream(response.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_matches_exact_templates_only() {
        assert!(allowed(&Method::GET, "/api/session"));
        assert!(allowed(&Method::POST, "/api/session/ses_1/prompt"));
        assert!(allowed(&Method::POST, "/api/session/ses_1/permission/per_2/reply"));
        assert!(allowed(&Method::DELETE, "/api/session/ses_1/form/frm_3"));
        assert!(!allowed(&Method::DELETE, "/api/session/ses_1"));
        assert!(!allowed(&Method::GET, "/api/credential/x"));
        assert!(!allowed(&Method::GET, "/api/pty"));
        assert!(!allowed(&Method::POST, "/api/session/ses_1/shell"));
        assert!(!allowed(&Method::GET, "/api/session/../credential"));
        assert!(!allowed(&Method::GET, "/api/session/ses_1/prompt/extra"));
        assert!(!allowed(&Method::GET, "/api/fs/read/etc/passwd"));
    }
}

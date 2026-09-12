//! Local Responses API proxy.
//!
//! ```text
//!                       (Codex client / Flutter app)
//!                                 │
//!                          POST /v1/responses
//!                          GET  /v1/responses (WS upgrade)
//!                                 │
//!                                 ▼
//!     ┌────────────────── axum Router ──────────────────┐
//!     │  forward_http  ◀── HTTP POST                    │
//!     │  forward_ws    ◀── WebSocket upgrade            │
//!     │       │             │                           │
//!     │       │   forwarded_headers (drops hop-by-hop)  │
//!     │       │   merge_auth_headers (Bearer + account) │
//!     │       ▼             ▼                           │
//!     └─────────┬───────────┬───────────────────────────┘
//!               │           │
//!               │           └──── tokio_tungstenite ──┐
//!               │                                     │
//!               └─── reqwest ────────────────────────┐│
//!                                                    ▼▼
//!                              https://chatgpt.com/backend-api/codex
//!                                       /responses (HTTP + WSS)
//! ```
//!
//! Auth headers are loaded once per server. The lookup order is:
//! 1. `CODEX_ACCESS_TOKEN` env var (used as a Bearer token).
//! 2. `~/.codex/auth.json` (written by `codex login`), parsed for the ChatGPT
//!    access token, account id, and FedRAMP claim from the embedded id_token.
//!
//! Two entry points share the same router: [`run`] binds a fresh listener
//! (used by the `pocket-codex api serve` worker subprocess) and [`serve`]
//! adopts a pre-bound [`TcpListener`] (used by the in-app host, which binds
//! `127.0.0.1:0` first so it can learn the port it must register).

#![forbid(unsafe_code)]

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{bail, Context, Result};
use axum::{
    body::Body,
    extract::{
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use base64::Engine as _;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http::header::{HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_socks::tcp::Socks5Stream;
use tokio_tungstenite::{
    client_async_tls_with_config, connect_async,
    tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage},
    MaybeTlsStream, WebSocketStream,
};
use url::Url;

const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

#[derive(Clone)]
struct ProxyState {
    client: Client,
    auth_headers: HeaderMap,
    http_upstream_url: String,
    ws_upstream_url: String,
    proxy: Option<UpstreamProxy>,
}

/// Bind `listen` and run the Responses API proxy until the process is
/// signalled. Used by the `pocket-codex api serve` worker.
pub async fn run(listen: String, proxy: Option<String>) -> Result<()> {
    let listen: SocketAddr = listen
        .parse()
        .with_context(|| format!("parsing API proxy listen address `{listen}`"))?;
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding API proxy on {listen}"))?;
    serve(listener, proxy).await
}

/// Verify the host's codex login is usable — `CODEX_ACCESS_TOKEN`, else a
/// parseable `~/.codex/auth.json` with ChatGPT tokens. Lets a caller fail fast
/// *before* registering a tunnel to a proxy that would immediately exit because
/// [`serve`] loads the same auth and returns `Err` when it is missing.
pub async fn check_auth() -> Result<()> {
    load_auth_headers().await.map(|_| ())
}

/// Run the Responses API proxy on an already-bound `listener` until the task
/// is dropped. The in-app host binds `127.0.0.1:0` first (to learn the port it
/// registers it on the relay), then hands the listener here.
pub async fn serve(listener: TcpListener, proxy: Option<String>) -> Result<()> {
    ensure_rustls_crypto_provider();
    let auth_headers = load_auth_headers().await?;

    let proxy_url = resolve_proxy(proxy.as_deref());
    let upstream_proxy = match proxy_url.as_deref() {
        Some(raw) => {
            tracing::info!("API proxy routing upstream through {}", redact_proxy(raw));
            Some(parse_proxy(raw)?)
        },
        None => {
            tracing::warn!(
                "no upstream proxy configured; reaching chatgpt.com directly and will fail on \
                 networks that block it. Set --proxy or HTTPS_PROXY/ALL_PROXY/HTTP_PROXY."
            );
            None
        },
    };

    let mut client_builder = Client::builder();
    if let Some(raw) = proxy_url.as_deref() {
        let proxy = reqwest::Proxy::all(raw)
            .with_context(|| format!("building reqwest proxy from `{raw}`"))?;
        client_builder = client_builder.proxy(proxy);
    }

    let state = ProxyState {
        client: client_builder
            .build()
            .context("building API proxy HTTP client")?,
        auth_headers,
        http_upstream_url: format!("{}/responses", CHATGPT_CODEX_BASE_URL.trim_end_matches('/')),
        ws_upstream_url: format!(
            "wss://{}/responses",
            CHATGPT_CODEX_BASE_URL
                .trim_start_matches("https://")
                .trim_end_matches('/')
        ),
        proxy: upstream_proxy,
    };
    axum::serve(listener, proxy_router(state))
        .await
        .context("running API proxy server")
}

fn proxy_router(state: ProxyState) -> Router {
    Router::new()
        .route("/v1/responses", post(forward_http).get(forward_ws))
        .fallback(proxy_forbidden)
        .with_state(Arc::new(state))
}

async fn load_auth_headers() -> Result<HeaderMap> {
    if let Ok(token) = env::var("CODEX_ACCESS_TOKEN") {
        if !token.trim().is_empty() {
            return bearer_headers(token.trim(), None, false);
        }
    }

    let auth_path = codex_home().join("auth.json");
    let raw = tokio::fs::read_to_string(&auth_path)
        .await
        .with_context(|| format!("reading Codex auth file {}", auth_path.display()))?;
    let auth: AuthFile = serde_json::from_str(&raw)
        .with_context(|| format!("parsing Codex auth file {}", auth_path.display()))?;
    let tokens = auth
        .tokens
        .context("Codex auth.json does not contain ChatGPT tokens; run `codex login`")?;
    let claims = tokens.id_token.as_deref().and_then(parse_chatgpt_claims);
    let account_id = tokens.account_id.or_else(|| {
        claims
            .as_ref()
            .and_then(|claims| claims.chatgpt_account_id.clone())
    });
    let fedramp = claims
        .as_ref()
        .is_some_and(|claims| claims.chatgpt_account_is_fedramp);
    bearer_headers(tokens.access_token, account_id, fedramp)
}

fn codex_home() -> PathBuf {
    if let Some(home) = env::var_os("CODEX_HOME") {
        return PathBuf::from(home);
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|home| home.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn bearer_headers(
    token: impl AsRef<str>,
    account_id: Option<String>,
    fedramp: bool,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let mut auth = HeaderValue::from_str(&format!("Bearer {}", token.as_ref()))
        .context("building Authorization header")?;
    auth.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth);
    if let Some(account_id) = account_id {
        headers.insert(
            HeaderName::from_static("chatgpt-account-id"),
            HeaderValue::from_str(&account_id).context("building ChatGPT-Account-ID header")?,
        );
    }
    if fedramp {
        headers
            .insert(HeaderName::from_static("x-openai-fedramp"), HeaderValue::from_static("true"));
    }
    Ok(headers)
}

#[derive(Debug, Deserialize)]
struct AuthFile {
    #[serde(default)]
    tokens: Option<AuthTokens>,
}

#[derive(Debug, Deserialize)]
struct AuthTokens {
    access_token: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatgptClaims {
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<ChatgptAuthClaims>,
}

#[derive(Debug, Deserialize)]
struct ChatgptAuthClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    chatgpt_account_is_fedramp: bool,
}

fn parse_chatgpt_claims(jwt: &str) -> Option<ChatgptAuthClaims> {
    let mut parts = jwt.split('.');
    let (_header, payload, _signature) = (parts.next()?, parts.next()?, parts.next()?);
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<ChatgptClaims>(&bytes).ok()?.auth
}

async fn proxy_forbidden() -> impl IntoResponse {
    StatusCode::FORBIDDEN
}

async fn forward_http(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match forward_http_inner(state, headers, body).await {
        Ok(response) => response,
        Err(err) => {
            let body = Body::from(format!("API proxy error: {err}"));
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(body)
                .unwrap_or_else(|_| Response::new(Body::empty()))
        },
    }
}

async fn forward_http_inner(
    state: Arc<ProxyState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let mut upstream_headers = forwarded_headers(&headers);
    merge_auth_headers(&mut upstream_headers, &state.auth_headers);
    let upstream = state
        .client
        .request(Method::POST, &state.http_upstream_url)
        .headers(upstream_headers)
        .body(body)
        .send()
        .await
        .context("forwarding HTTP request to upstream Responses API")?;

    let status = upstream.status();
    let headers = response_headers(upstream.headers());
    let body = Body::from_stream(upstream.bytes_stream());
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}

async fn forward_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = proxy_websocket(socket, state, headers).await {
            tracing::warn!("API proxy websocket closed with error: {err}");
        }
    })
}

async fn proxy_websocket(
    downstream: WebSocket,
    state: Arc<ProxyState>,
    headers: HeaderMap,
) -> Result<()> {
    let mut request = state
        .ws_upstream_url
        .as_str()
        .into_client_request()
        .context("building upstream websocket request")?;
    {
        let request_headers = request.headers_mut();
        request_headers.extend(forwarded_headers(&headers));
        merge_auth_headers(request_headers, &state.auth_headers);
    }
    let upstream = open_upstream_ws(request, state.proxy.as_ref())
        .await
        .context("connecting upstream Responses websocket")?;
    let (mut downstream_tx, mut downstream_rx) = downstream.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    let downstream_to_upstream = async {
        while let Some(message) = downstream_rx.next().await {
            let message = message.context("reading local websocket message")?;
            let Some(message) = axum_to_tungstenite(message) else {
                break;
            };
            upstream_tx
                .send(message)
                .await
                .context("sending upstream websocket message")?;
        }
        Result::<()>::Ok(())
    };
    let upstream_to_downstream = async {
        while let Some(message) = upstream_rx.next().await {
            let message = message.context("reading upstream websocket message")?;
            let Some(message) = tungstenite_to_axum(message) else {
                break;
            };
            downstream_tx
                .send(message)
                .await
                .context("sending local websocket message")?;
        }
        Result::<()>::Ok(())
    };
    tokio::pin!(downstream_to_upstream);
    tokio::pin!(upstream_to_downstream);
    tokio::select! {
        res = &mut downstream_to_upstream => res?,
        res = &mut upstream_to_downstream => res?,
    }

    Ok(())
}

fn forwarded_headers(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        let lower = name.as_str();
        if matches!(
            lower,
            "authorization"
                | "host"
                | "content-length"
                | "connection"
                | "upgrade"
                | "sec-websocket-key"
                | "sec-websocket-version"
                | "sec-websocket-extensions"
                | "sec-websocket-protocol"
        ) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

fn merge_auth_headers(headers: &mut HeaderMap, auth_headers: &HeaderMap) {
    for (name, value) in auth_headers {
        headers.insert(name.clone(), value.clone());
    }
}

fn response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if matches!(
            name.as_str(),
            "content-length" | "transfer-encoding" | "connection" | "trailer" | "upgrade"
        ) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

fn axum_to_tungstenite(message: AxumMessage) -> Option<TungsteniteMessage> {
    match message {
        AxumMessage::Text(text) => Some(TungsteniteMessage::Text(text.to_string().into())),
        AxumMessage::Binary(bytes) => Some(TungsteniteMessage::Binary(bytes)),
        AxumMessage::Ping(bytes) => Some(TungsteniteMessage::Ping(bytes)),
        AxumMessage::Pong(bytes) => Some(TungsteniteMessage::Pong(bytes)),
        AxumMessage::Close(frame) => Some(TungsteniteMessage::Close(frame.map(|frame| {
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }
        }))),
    }
}

fn tungstenite_to_axum(message: TungsteniteMessage) -> Option<AxumMessage> {
    match message {
        TungsteniteMessage::Text(text) => Some(AxumMessage::Text(text.to_string().into())),
        TungsteniteMessage::Binary(bytes) => Some(AxumMessage::Binary(bytes)),
        TungsteniteMessage::Ping(bytes) => Some(AxumMessage::Ping(bytes)),
        TungsteniteMessage::Pong(bytes) => Some(AxumMessage::Pong(bytes)),
        TungsteniteMessage::Close(frame) => {
            Some(AxumMessage::Close(frame.map(|frame| axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            })))
        },
        TungsteniteMessage::Frame(_) => None,
    }
}

/// Parsed upstream proxy used for the WebSocket CONNECT / SOCKS tunnel.
#[derive(Clone, Debug)]
struct UpstreamProxy {
    kind: ProxyKind,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyKind {
    HttpConnect,
    Socks5,
}

/// Install a process-wide rustls crypto provider (ring) so the WebSocket
/// TLS path has a default provider. `tokio-tungstenite`'s `None` connector
/// builds a `ClientConfig` that needs this; without it rustls panics with
/// "Could not automatically determine the process-level CryptoProvider".
/// Mirrors codex's own `ensure_rustls_crypto_provider`.
fn ensure_rustls_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Resolve the effective upstream proxy: explicit `--proxy` wins, then the
/// standard proxy environment variables (HTTPS first, since the upstream is
/// HTTPS/WSS), then `ALL_PROXY`, then `HTTP_PROXY`. Empty values are ignored.
pub fn resolve_proxy(explicit: Option<&str>) -> Option<String> {
    if let Some(value) = explicit {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
    {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Redact userinfo (credentials) from a proxy URL before logging it.
/// Uses `Url`'s in-place setters so path/query/other components survive.
pub fn redact_proxy(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_string();
    };
    if url.username().is_empty() && url.password().is_none() {
        return raw.to_string();
    }
    let _ = url.set_username("***");
    let _ = url.set_password(None);
    url.to_string()
}

/// Validate a proxy URL without exposing the internal `UpstreamProxy`
/// type, so callers can fail fast on a bad `--proxy` before spawning.
pub fn validate_proxy(raw: &str) -> Result<()> {
    parse_proxy(raw).map(|_| ())
}

/// Parse a proxy URL into an [`UpstreamProxy`] for the WebSocket tunnel.
fn parse_proxy(raw: &str) -> Result<UpstreamProxy> {
    let url = Url::parse(raw).with_context(|| format!("parsing proxy URL `{raw}`"))?;
    let kind = match url.scheme() {
        "http" => ProxyKind::HttpConnect,
        "socks5" | "socks5h" => ProxyKind::Socks5,
        // An `https://` proxy speaks TLS itself; our WS tunnel writes a
        // plaintext CONNECT, so accepting it would silently break the
        // WebSocket path. Reject it rather than advertise broken support.
        "https" => bail!(
            "`https://` proxies are not supported (the WebSocket tunnel needs a plaintext \
             CONNECT); use an `http://` or `socks5://` proxy"
        ),
        other => bail!("unsupported proxy scheme `{other}` (use http or socks5)"),
    };
    let host = url
        .host_str()
        .with_context(|| format!("proxy URL `{raw}` is missing a host"))?
        .to_string();
    let port = url.port().unwrap_or(match kind {
        ProxyKind::HttpConnect => 8080,
        ProxyKind::Socks5 => 1080,
    });
    let username = (!url.username().is_empty()).then(|| url.username().to_string());
    let password = url.password().map(ToString::to_string);
    Ok(UpstreamProxy {
        kind,
        host,
        port,
        username,
        password,
    })
}

/// Open the upstream Responses WebSocket, optionally tunnelling through a
/// proxy. `tokio-tungstenite` has no proxy support of its own, so for the
/// proxied path we establish the raw TCP tunnel ourselves (HTTP `CONNECT`
/// or SOCKS5) and let tungstenite layer TLS + the WS handshake on top.
async fn open_upstream_ws(
    request: http::Request<()>,
    proxy: Option<&UpstreamProxy>,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let Some(proxy) = proxy else {
        let (stream, _) = connect_async(request)
            .await
            .context("direct websocket connect")?;
        return Ok(stream);
    };

    let uri = request.uri().clone();
    let host = uri
        .host()
        .context("websocket upstream URL is missing a host")?
        .to_string();
    let port = uri.port_u16().unwrap_or(443);

    let tcp = match proxy.kind {
        ProxyKind::HttpConnect => http_connect_tunnel(proxy, &host, port).await?,
        ProxyKind::Socks5 => socks5_tunnel(proxy, &host, port).await?,
    };

    let (stream, _) = client_async_tls_with_config(request, tcp, None, None)
        .await
        .context("websocket TLS handshake over proxy tunnel")?;
    Ok(stream)
}

/// Establish an HTTP `CONNECT` tunnel through `proxy` to `host:port`.
async fn http_connect_tunnel(proxy: &UpstreamProxy, host: &str, port: u16) -> Result<TcpStream> {
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .with_context(|| format!("connecting to HTTP proxy {}:{}", proxy.host, proxy.port))?;

    let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    // HTTP Basic permits an empty password, so authenticate whenever a
    // username is present and default the password to "".
    if let Some(user) = proxy.username.as_deref() {
        let pass = proxy.password.as_deref().unwrap_or("");
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .context("sending CONNECT request to proxy")?;
    stream.flush().await.context("flushing CONNECT request")?;

    // Read response headers byte-by-byte so we do not consume tunnelled bytes.
    let mut buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .await
            .context("reading CONNECT response")?;
        if n == 0 {
            bail!("proxy closed connection during CONNECT handshake");
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() > 8192 {
            bail!("proxy CONNECT response headers exceeded 8 KiB");
        }
    }

    let head = String::from_utf8_lossy(&buf);
    let status_line = head.lines().next().unwrap_or_default();
    let ok = status_line
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code == "200");
    if !ok {
        bail!("proxy CONNECT failed: {status_line}");
    }
    Ok(stream)
}

/// Establish a SOCKS5 tunnel through `proxy` to `host:port`. The domain is
/// resolved by the proxy (socks5h semantics), which matters when local DNS
/// to the upstream is blocked.
async fn socks5_tunnel(proxy: &UpstreamProxy, host: &str, port: u16) -> Result<TcpStream> {
    let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
    // SOCKS5 permits an empty password, so authenticate whenever a username
    // is present and default the password to "".
    let stream = match proxy.username.as_deref() {
        Some(user) => {
            let pass = proxy.password.as_deref().unwrap_or("");
            Socks5Stream::connect_with_password(proxy_addr.as_str(), (host, port), user, pass)
                .await
                .with_context(|| format!("SOCKS5 (auth) connect via {proxy_addr}"))?
        },
        None => Socks5Stream::connect(proxy_addr.as_str(), (host, port))
            .await
            .with_context(|| format!("SOCKS5 connect via {proxy_addr}"))?,
    };
    Ok(stream.into_inner())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::header::SET_COOKIE;

    use super::*;

    const HTTP_REQUEST: &str = r#"{"model":"test-model","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],"tool_choice":"auto","parallel_tool_calls":true,"reasoning":null,"store":false,"stream":true,"include":[]}"#;
    // ResponsesWsRequest is internally tagged: request fields stay at the top
    // level.
    const WS_REQUEST: &str = r#"{"type":"response.create","model":"test-model","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],"tool_choice":"auto","parallel_tool_calls":true,"reasoning":null,"store":false,"stream":true,"include":[],"previous_response_id":"resp-previous"}"#;

    async fn test_server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("test address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        (addr, task)
    }

    fn test_proxy(upstream: SocketAddr) -> Router {
        ensure_rustls_crypto_provider();
        proxy_router(ProxyState {
            client: Client::builder()
                .no_proxy()
                .build()
                .expect("test HTTP client"),
            auth_headers: bearer_headers("test-host-token", Some("test-account".into()), false)
                .expect("test credentials"),
            http_upstream_url: format!("http://{upstream}/responses"),
            ws_upstream_url: format!("ws://{upstream}/responses"),
            proxy: None,
        })
    }

    fn assert_upstream_headers(headers: &HeaderMap) {
        assert_eq!(headers[AUTHORIZATION], "Bearer test-host-token");
        assert_eq!(headers["chatgpt-account-id"], "test-account");
        assert_eq!(headers["x-client-marker"], "preserved");
    }

    #[tokio::test]
    async fn http_responses_preserve_streaming_errors_and_host_auth() {
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let upstream = Router::new().route(
            "/responses",
            post(move |headers: HeaderMap, body: Bytes| {
                let gate = gate.clone();
                async move {
                    assert_upstream_headers(&headers);
                    if body == "error" {
                        return Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .header("content-type", "application/json")
                            .header("retry-after", "2")
                            .body(Body::from(r#"{"error":{"code":"rate_limit_exceeded"}}"#))
                            .expect("error response");
                    }
                    assert_eq!(body, HTTP_REQUEST);
                    let first = futures::stream::once(async {
                        Ok::<_, std::io::Error>("data: {\"type\":\"response.created\"}\n\n")
                    });
                    let second = futures::stream::once(async move {
                        gate.notified().await;
                        Ok::<_, std::io::Error>("data: {\"type\":\"response.completed\"}\n\n")
                    });
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .header("x-request-id", "test-request")
                        .body(Body::from_stream(first.chain(second)))
                        .expect("stream response")
                }
            }),
        );
        let (upstream_addr, upstream_task) = test_server(upstream).await;
        let (proxy_addr, proxy_task) = test_server(test_proxy(upstream_addr)).await;
        let client = Client::builder()
            .no_proxy()
            .build()
            .expect("downstream client");
        let request = || {
            client
                .post(format!("http://{proxy_addr}/v1/responses"))
                .bearer_auth("downstream-token-must-not-reach-upstream")
                .header("chatgpt-account-id", "downstream-account")
                .header("x-client-marker", "preserved")
        };
        let response = request()
            .body(HTTP_REQUEST)
            .send()
            .await
            .expect("HTTP Responses request");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert_eq!(response.headers()["x-request-id"], "test-request");
        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("first event must arrive before the upstream completes")
            .expect("first chunk")
            .expect("first event");
        assert_eq!(first, "data: {\"type\":\"response.created\"}\n\n");
        release.notify_one();
        let second = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("completion timeout")
            .expect("completion chunk")
            .expect("completion event");
        assert_eq!(second, "data: {\"type\":\"response.completed\"}\n\n");
        let error = request().body("error").send().await.expect("error request");
        assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.headers()["retry-after"], "2");
        assert_eq!(
            error.text().await.expect("error body"),
            r#"{"error":{"code":"rate_limit_exceeded"}}"#
        );
        assert_eq!(
            client
                .get(format!("http://{proxy_addr}/other"))
                .send()
                .await
                .expect("other route")
                .status(),
            StatusCode::FORBIDDEN
        );
        proxy_task.abort();
        upstream_task.abort();
    }

    #[tokio::test]
    async fn websocket_responses_forward_frames_and_host_auth() {
        let upstream = Router::new().route(
            "/responses",
            axum::routing::get(|ws: WebSocketUpgrade, headers: HeaderMap| async move {
                assert_upstream_headers(&headers);
                ws.on_upgrade(|mut socket| async move {
                    let request = socket
                        .recv()
                        .await
                        .expect("request frame")
                        .expect("request");
                    assert_eq!(request.into_text().expect("text request"), WS_REQUEST);
                    socket
                        .send(AxumMessage::Text(r#"{"type":"response.created"}"#.into()))
                        .await
                        .expect("created");
                    socket
                        .send(AxumMessage::Text(r#"{"type":"response.completed"}"#.into()))
                        .await
                        .expect("completed");
                    let binary = socket.recv().await.expect("binary frame").expect("binary");
                    assert!(
                        matches!(&binary, AxumMessage::Binary(data) if data.as_ref() == b"payload")
                    );
                    socket.send(binary).await.expect("binary echo");
                })
            }),
        );
        let (upstream_addr, upstream_task) = test_server(upstream).await;
        let (proxy_addr, proxy_task) = test_server(test_proxy(upstream_addr)).await;
        let mut request = format!("ws://{proxy_addr}/v1/responses")
            .into_client_request()
            .expect("WS request");
        request
            .headers_mut()
            .insert(AUTHORIZATION, HeaderValue::from_static("Bearer downstream"));
        request
            .headers_mut()
            .insert("x-client-marker", HeaderValue::from_static("preserved"));
        let (mut socket, handshake) = connect_async(request).await.expect("WS handshake");
        assert_eq!(handshake.status(), StatusCode::SWITCHING_PROTOCOLS);
        socket
            .send(TungsteniteMessage::Text(WS_REQUEST.into()))
            .await
            .expect("send response.create");
        for kind in ["response.created", "response.completed"] {
            let event = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .expect("event timeout")
                .expect("event frame")
                .expect("event");
            assert_eq!(event.into_text().expect("event text"), format!(r#"{{"type":"{kind}"}}"#));
        }
        socket
            .send(TungsteniteMessage::Binary(Bytes::from_static(b"payload")))
            .await
            .expect("send binary");
        let echo = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("echo timeout")
            .expect("echo frame")
            .expect("echo");
        assert_eq!(echo, TungsteniteMessage::Binary(Bytes::from_static(b"payload")));
        proxy_task.abort();
        upstream_task.abort();
    }

    #[test]
    fn forwarded_headers_drop_hop_by_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(HeaderName::from_static("connection"), HeaderValue::from_static("close"));
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer local"),
        );
        headers.append(SET_COOKIE, HeaderValue::from_static("a=1"));
        headers.append(SET_COOKIE, HeaderValue::from_static("b=2"));

        let forwarded = forwarded_headers(&headers);

        assert!(!forwarded.contains_key("connection"));
        assert!(!forwarded.contains_key("authorization"));
        assert_eq!(forwarded.get_all(SET_COOKIE).iter().count(), 2);
    }

    #[test]
    fn extending_request_headers_preserves_multi_value_headers() {
        let mut incoming = HeaderMap::new();
        incoming.append(SET_COOKIE, HeaderValue::from_static("a=1"));
        incoming.append(SET_COOKIE, HeaderValue::from_static("b=2"));

        let mut request_headers = HeaderMap::new();
        request_headers.extend(forwarded_headers(&incoming));

        let cookies = request_headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|value| value.to_str().expect("ascii"))
            .collect::<Vec<_>>();

        assert_eq!(cookies, vec!["a=1", "b=2"]);
    }

    #[test]
    fn validate_proxy_rejects_unsupported_schemes() {
        assert!(validate_proxy("https://proxy.example:8443").is_err());
        assert!(validate_proxy("ftp://nope").is_err());
        assert!(validate_proxy("http://127.0.0.1:11111").is_ok());
        assert!(validate_proxy("socks5://127.0.0.1:1080").is_ok());
    }

    #[test]
    fn redact_proxy_hides_credentials_only() {
        assert_eq!(redact_proxy("http://127.0.0.1:11111"), "http://127.0.0.1:11111");
        assert_eq!(redact_proxy("http://user:pass@host:8080/"), "http://***@host:8080/");
    }
}

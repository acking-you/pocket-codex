//! Local Responses API proxy used by `pocket-codex api serve`.
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
//! Auth headers are loaded once per worker. The lookup order is:
//! 1. `CODEX_ACCESS_TOKEN` env var (used as a Bearer token).
//! 2. `~/.codex/auth.json` (written by `codex login`), parsed for the ChatGPT
//!    access token, account id, and FedRAMP claim from the embedded id_token.

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

pub(crate) async fn run(listen: String, proxy: Option<String>) -> Result<()> {
    ensure_rustls_crypto_provider();
    let listen: SocketAddr = listen
        .parse()
        .with_context(|| format!("parsing API proxy listen address `{listen}`"))?;
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
    let app = Router::new()
        .route("/v1/responses", post(forward_http).get(forward_ws))
        .fallback(proxy_forbidden)
        .with_state(Arc::new(state));
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding API proxy on {listen}"))?;
    axum::serve(listener, app)
        .await
        .context("running API proxy server")
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
pub(crate) fn resolve_proxy(explicit: Option<&str>) -> Option<String> {
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
pub(crate) fn redact_proxy(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(url) if !url.username().is_empty() || url.password().is_some() => {
            let scheme = url.scheme();
            let host = url.host_str().unwrap_or_default();
            match url.port() {
                Some(port) => format!("{scheme}://***@{host}:{port}"),
                None => format!("{scheme}://***@{host}"),
            }
        },
        _ => raw.to_string(),
    }
}

/// Parse a proxy URL into an [`UpstreamProxy`] for the WebSocket tunnel.
fn parse_proxy(raw: &str) -> Result<UpstreamProxy> {
    let url = Url::parse(raw).with_context(|| format!("parsing proxy URL `{raw}`"))?;
    let kind = match url.scheme() {
        "http" | "https" => ProxyKind::HttpConnect,
        "socks5" | "socks5h" => ProxyKind::Socks5,
        other => bail!("unsupported proxy scheme `{other}` (use http, https or socks5)"),
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
        let (stream, _) = connect_async(request).await.context("direct websocket connect")?;
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
    if let (Some(user), Some(pass)) = (proxy.username.as_deref(), proxy.password.as_deref()) {
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
        let n = stream.read(&mut byte).await.context("reading CONNECT response")?;
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
    let stream = match (proxy.username.as_deref(), proxy.password.as_deref()) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(proxy_addr.as_str(), (host, port), user, pass)
                .await
                .with_context(|| format!("SOCKS5 (auth) connect via {proxy_addr}"))?
        },
        _ => Socks5Stream::connect(proxy_addr.as_str(), (host, port))
            .await
            .with_context(|| format!("SOCKS5 connect via {proxy_addr}"))?,
    };
    Ok(stream.into_inner())
}

#[cfg(test)]
mod tests {
    use http::header::SET_COOKIE;

    use super::*;

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
}

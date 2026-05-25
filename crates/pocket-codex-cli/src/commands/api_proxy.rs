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

use anyhow::{Context, Result};
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
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage},
};

const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

#[derive(Clone)]
struct ProxyState {
    client: Client,
    auth_headers: HeaderMap,
    http_upstream_url: String,
    ws_upstream_url: String,
}

pub(crate) async fn run(listen: String) -> Result<()> {
    let listen: SocketAddr = listen
        .parse()
        .with_context(|| format!("parsing API proxy listen address `{listen}`"))?;
    let auth_headers = load_auth_headers().await?;
    let state = ProxyState {
        client: Client::builder()
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
    let raw = std::fs::read_to_string(&auth_path)
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
    let mut response = Response::builder().status(status);
    if let Some(response_headers) = response.headers_mut() {
        *response_headers = headers;
    } else {
        anyhow::bail!("response builder missing mutable headers");
    }
    response
        .body(body)
        .context("building API proxy HTTP response")
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
        for (name, value) in forwarded_headers(&headers) {
            if let Some(name) = name {
                request_headers.insert(name, value);
            }
        }
        merge_auth_headers(request_headers, &state.auth_headers);
    }
    let (upstream, _) = connect_async(request)
        .await
        .context("connecting upstream Responses websocket")?;
    let (mut downstream_tx, mut downstream_rx) = downstream.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    loop {
        tokio::select! {
            local = downstream_rx.next() => {
                let Some(local) = local else { break; };
                let local = local.context("reading local websocket message")?;
                let Some(message) = axum_to_tungstenite(local) else { break; };
                upstream_tx.send(message).await.context("sending upstream websocket message")?;
            }
            remote = upstream_rx.next() => {
                let Some(remote) = remote else { break; };
                let remote = remote.context("reading upstream websocket message")?;
                let Some(message) = tungstenite_to_axum(remote) else { break; };
                downstream_tx.send(message).await.context("sending local websocket message")?;
            }
        }
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

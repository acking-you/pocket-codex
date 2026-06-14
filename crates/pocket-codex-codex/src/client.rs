//! Async WebSocket JSON-RPC client for a `codex app-server`.
//!
//! The app-server speaks JSON-RPC 2.0 as WebSocket text frames (see
//! [`crate::protocol`]). This client owns one connection and:
//!
//! * correlates outbound [`Request`]s with their [`Response`]/[`ErrorResponse`]
//!   by `id` (UUID strings) using a per-request oneshot channel,
//! * forwards every inbound [`Notification`] to an unbounded mpsc receiver the
//!   caller drains (the event stream the UI renders),
//! * tolerates the relay/transport dropping by failing in-flight and future
//!   requests once the reader task exits.
//!
//! It is transport-only: it does not know about threads or turns, just the
//! request/response/notification envelopes. Higher layers (the bridge) drive
//! `initialize` / `thread/*` / `turn/*` on top.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use futures::{
    stream::{SplitSink, StreamExt},
    SinkExt,
};
use serde_json::Value;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, Mutex},
    task::JoinHandle,
};
use tokio_tungstenite::{
    connect_async, tungstenite::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};

use crate::protocol::{Message, Notification, Request, RequestId, Response};

/// An inbound server-originated message: a fire-and-forget notification
/// (`request_id` = `None`) or a server→client request awaiting a response
/// (`request_id` = `Some`, e.g. an `execCommandApproval` prompt).
#[derive(Debug, Clone)]
pub struct Inbound {
    /// JSON-RPC method name.
    pub method: String,
    /// Method params, if any.
    pub params: Option<serde_json::Value>,
    /// Opaque token identifying a server request to answer via
    /// [`AppClient::respond`]; `None` for notifications.
    pub request_id: Option<String>,
}

/// Default per-request timeout. A model turn streams via notifications, so
/// individual request/response round-trips (initialize, thread/start, …) are
/// quick; 60s is generous headroom for a slow relay hop.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;
type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value>>>>>;
/// token (stringified id) → original [`RequestId`], so a server request can be
/// answered with the exact id type (int stays int) it arrived with.
type ServerReqs = Arc<Mutex<HashMap<String, RequestId>>>;

/// How often to send a WebSocket ping. Keeps the pb-mapper relay tunnel from
/// idle-closing a backgrounded connection, and surfaces a dead socket promptly.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(25);

/// A connected app-server WebSocket JSON-RPC client.
pub struct AppClient {
    sink: Arc<Mutex<WsSink>>,
    pending: Pending,
    server_reqs: ServerReqs,
    next_id: AtomicU64,
    reader: JoinHandle<()>,
    keepalive: JoinHandle<()>,
}

impl Drop for AppClient {
    fn drop(&mut self) {
        self.reader.abort();
        self.keepalive.abort();
    }
}

impl AppClient {
    /// Connect to `ws_url` (e.g. `ws://127.0.0.1:28080`) and start the reader
    /// task. Returns the client plus the receiver of inbound notifications.
    pub async fn connect(
        ws_url: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Inbound>)> {
        let (stream, _resp) = connect_async(ws_url)
            .await
            .with_context(|| format!("connecting app-server websocket {ws_url}"))?;
        let (sink, mut read) = stream.split();

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let server_reqs: ServerReqs = Arc::new(Mutex::new(HashMap::new()));
        let (notify_tx, notify_rx) = mpsc::unbounded_channel();

        let reader_pending = Arc::clone(&pending);
        let reader_server_reqs = Arc::clone(&server_reqs);
        let reader = tokio::spawn(async move {
            while let Some(frame) = read.next().await {
                let text = match frame {
                    Ok(WsMessage::Text(t)) => t.to_string(),
                    Ok(WsMessage::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
                    Ok(WsMessage::Close(_)) | Err(_) => break,
                    // Ping/Pong/Frame: nothing to dispatch.
                    Ok(_) => continue,
                };
                let Ok(msg) = serde_json::from_str::<Message>(&text) else {
                    continue;
                };
                match msg {
                    Message::Response(r) => {
                        if let Some(tx) = take_pending(&reader_pending, &r.id).await {
                            let _ = tx.send(Ok(r.result));
                        }
                    },
                    Message::Error(e) => {
                        if let Some(tx) = take_pending(&reader_pending, &e.id).await {
                            let _ = tx.send(Err(anyhow!("{}", e.error.message)));
                        }
                    },
                    // Server-initiated requests (approvals etc.): record the id
                    // under a token so the UI can answer via `respond`.
                    Message::Request(req) => {
                        let token = match &req.id {
                            RequestId::String(s) => s.clone(),
                            RequestId::Number(n) => n.to_string(),
                        };
                        reader_server_reqs
                            .lock()
                            .await
                            .insert(token.clone(), req.id);
                        let _ = notify_tx.send(Inbound {
                            method: req.method,
                            params: req.params,
                            request_id: Some(token),
                        });
                    },
                    Message::Notification(n) => {
                        let _ = notify_tx.send(Inbound {
                            method: n.method,
                            params: n.params,
                            request_id: None,
                        });
                    },
                }
            }
            // Connection closed: fail every in-flight request so callers don't
            // hang on a oneshot that will never resolve.
            let mut map = reader_pending.lock().await;
            for (_, tx) in map.drain() {
                let _ = tx.send(Err(anyhow!("app-server connection closed")));
            }
        });

        let sink = Arc::new(Mutex::new(sink));
        // Periodic ping keeps the relay tunnel warm (so a backgrounded session
        // isn't idle-closed) and fails fast if the socket has died.
        let keepalive_sink = Arc::clone(&sink);
        let keepalive = tokio::spawn(async move {
            let mut tick = tokio::time::interval(KEEPALIVE_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tick.tick().await; // consume the immediate first tick
            loop {
                tick.tick().await;
                let sent = keepalive_sink
                    .lock()
                    .await
                    .send(WsMessage::Ping(Vec::new().into()))
                    .await;
                if sent.is_err() {
                    break;
                }
            }
        });

        Ok((
            Self {
                sink,
                pending,
                server_reqs,
                next_id: AtomicU64::new(1),
                reader,
                keepalive,
            },
            notify_rx,
        ))
    }

    /// Answer a server→client request (identified by the `request_id` token
    /// from an [`Inbound`]) with `result`. No-op if the token is unknown.
    pub async fn respond(&self, token: &str, result: Value) -> Result<()> {
        let id = self.server_reqs.lock().await.remove(token);
        let Some(id) = id else {
            return Ok(());
        };
        let resp = Response {
            jsonrpc: None,
            id,
            result,
        };
        let frame = serde_json::to_string(&resp).context("serializing response")?;
        self.sink
            .lock()
            .await
            .send(WsMessage::text(frame))
            .await
            .map_err(|e| anyhow!("sending response: {e}"))
    }

    /// Send a JSON-RPC request and await its result, erroring on timeout, a
    /// JSON-RPC error response, or a dropped connection.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let req = Request {
            jsonrpc: None,
            id: RequestId::String(id.clone()),
            method: method.to_string(),
            params: Some(params),
        };
        let frame = serde_json::to_string(&req).context("serializing request")?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        if let Err(e) = self.sink.lock().await.send(WsMessage::text(frame)).await {
            self.pending.lock().await.remove(&id);
            return Err(anyhow!("sending request `{method}`: {e}"));
        }

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow!("request `{method}` cancelled")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(anyhow!("request `{method}` timed out"))
            },
        }
    }

    /// Send a fire-and-forget notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let note = Notification {
            jsonrpc: None,
            method: method.to_string(),
            params: Some(params),
        };
        let frame = serde_json::to_string(&note).context("serializing notification")?;
        self.sink
            .lock()
            .await
            .send(WsMessage::text(frame))
            .await
            .map_err(|e| anyhow!("sending notification `{method}`: {e}"))
    }
}

async fn take_pending(pending: &Pending, id: &RequestId) -> Option<oneshot::Sender<Result<Value>>> {
    let key = match id {
        RequestId::String(s) => s.clone(),
        RequestId::Number(n) => n.to_string(),
    };
    pending.lock().await.remove(&key)
}

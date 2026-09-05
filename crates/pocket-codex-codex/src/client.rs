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
        Arc, Mutex as StdMutex,
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
use tokio_util::sync::CancellationToken;

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
type Pending = Arc<StdMutex<HashMap<String, oneshot::Sender<Result<Value>>>>>;
/// token (stringified id) → original [`RequestId`], so a server request can be
/// answered with the exact id type (int stays int) it arrived with.
type ServerReqs = Arc<Mutex<HashMap<String, RequestId>>>;

/// How often to send a WebSocket ping. Keeps the pb-mapper relay tunnel from
/// idle-closing a backgrounded connection, and drives the liveness watchdog.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// How long after a ping the socket may be silent (no Pong and no other frame)
/// before it is judged dead. A HALF-OPEN socket — TCP dropped without a clean
/// FIN, common on relay/mobile jitter — still accepts buffered writes, so the
/// ping "succeeds" while the read half hangs forever; only the absence of
/// return traffic reveals it. Comfortably longer than one round-trip over a
/// slow relay hop, short enough to reconnect long before a request times out.
const LIVENESS_DEADLINE: Duration = Duration::from_secs(20);

/// A connected app-server WebSocket JSON-RPC client.
pub struct AppClient {
    sink: Arc<Mutex<WsSink>>,
    pending: Pending,
    server_reqs: ServerReqs,
    next_id: AtomicU64,
    reader: JoinHandle<()>,
    keepalive: JoinHandle<()>,
    closed: CancellationToken,
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
    pub async fn connect(ws_url: &str) -> Result<(Self, mpsc::UnboundedReceiver<Inbound>)> {
        let (stream, _resp) = connect_async(ws_url)
            .await
            .with_context(|| format!("connecting app-server websocket {ws_url}"))?;
        let (sink, mut read) = stream.split();

        let sink = Arc::new(Mutex::new(sink));
        let pending: Pending = Arc::new(StdMutex::new(HashMap::new()));
        let server_reqs: ServerReqs = Arc::new(Mutex::new(HashMap::new()));
        let (notify_tx, notify_rx) = mpsc::unbounded_channel();

        // Monotonic "frames seen" counter: the reader bumps it on EVERY inbound
        // frame (data or Pong), and the keepalive watchdog uses it to tell a
        // live-but-quiet socket from a dead half-open one.
        let activity = Arc::new(AtomicU64::new(0));
        let closed = CancellationToken::new();

        let reader_pending = Arc::clone(&pending);
        let reader_server_reqs = Arc::clone(&server_reqs);
        let reader_activity = Arc::clone(&activity);
        let reader_closed = closed.clone();
        let reader = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = reader_closed.cancelled() => break,
                    frame = read.next() => match frame {
                        Some(frame) => frame,
                        None => break,
                    },
                };
                // Any frame — including the Pong answering our keepalive Ping —
                // proves the socket's read half is alive.
                reader_activity.fetch_add(1, Ordering::Relaxed);
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
                        if let Some(tx) = take_pending(&reader_pending, &r.id) {
                            let _ = tx.send(Ok(r.result));
                        }
                    },
                    Message::Error(e) => {
                        if let Some(tx) = take_pending(&reader_pending, &e.id) {
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
            close_connection(&reader_closed, &reader_pending);
        });

        // Keepalive + liveness watchdog. Each tick pings (keeping the relay
        // tunnel warm so a backgrounded session isn't idle-closed) then waits a
        // bounded window for ANY return frame. A healthy socket answers the Ping
        // with a Pong (or is already streaming), bumping `activity`; a HALF-OPEN
        // socket accepts the buffered Ping write but never reads back, so
        // `activity` stays put — that is the only reliable dead-socket signal,
        // since the write "succeeds". On silence we mark unhealthy (callers
        // reconnect) and best-effort close the sink to unwedge the reader.
        let keepalive_sink = Arc::clone(&sink);
        let keepalive_activity = Arc::clone(&activity);
        let keepalive_closed = closed.clone();
        let keepalive_pending = Arc::clone(&pending);
        let keepalive = tokio::spawn(async move {
            let mut tick = tokio::time::interval(KEEPALIVE_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tick.tick().await; // consume the immediate first tick
            loop {
                tokio::select! {
                    _ = keepalive_closed.cancelled() => break,
                    _ = tick.tick() => {},
                }
                let before = keepalive_activity.load(Ordering::Relaxed);
                let sent = tokio::select! {
                    _ = keepalive_closed.cancelled() => break,
                    sent = tokio::time::timeout(LIVENESS_DEADLINE, async {
                        keepalive_sink.lock().await.send(WsMessage::Ping(Vec::new().into())).await
                    }) => sent,
                };
                if !matches!(sent, Ok(Ok(()))) {
                    break;
                }
                // Give the Pong (or any traffic) a bounded window to arrive.
                tokio::select! {
                    _ = keepalive_closed.cancelled() => break,
                    _ = tokio::time::sleep(LIVENESS_DEADLINE) => {},
                }
                if keepalive_activity.load(Ordering::Relaxed) == before {
                    // No return frame within the deadline → half-open/dead.
                    break;
                }
            }
            close_connection(&keepalive_closed, &keepalive_pending);
            let _ = tokio::time::timeout(LIVENESS_DEADLINE, async {
                keepalive_sink.lock().await.close().await
            })
            .await;
        });

        Ok((
            Self {
                sink,
                pending,
                server_reqs,
                next_id: AtomicU64::new(1),
                reader,
                keepalive,
                closed,
            },
            notify_rx,
        ))
    }

    /// Whether the socket is still considered live. Goes `false` once the
    /// reader closes, a write fails, a request times out, or the keepalive
    /// watchdog sees silence past [`LIVENESS_DEADLINE`]. Higher layers poll
    /// this to reconnect instead of reusing an unresponsive connection.
    pub fn is_alive(&self) -> bool {
        !self.closed.is_cancelled()
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
        self.send_frame(frame).await.context("sending response")
    }

    /// Send a JSON-RPC request and await its result, erroring on timeout, a
    /// JSON-RPC error response, or a dropped connection.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_inner(method, Some(params), REQUEST_TIMEOUT)
            .await
    }

    /// Like [`request`](Self::request) but omits the `params` field entirely.
    /// A few methods (e.g. `account/rateLimits/read`) are typed no-params
    /// upstream (`Option<()>`, skipped when absent) and reject an empty `{}`
    /// body as invalid params, so they must be sent with no `params` key.
    pub async fn request_no_params(&self, method: &str) -> Result<Value> {
        self.request_inner(method, None, REQUEST_TIMEOUT).await
    }

    async fn request_inner(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let req = Request {
            jsonrpc: None,
            id: RequestId::String(id.clone()),
            method: method.to_string(),
            params,
        };
        let frame = serde_json::to_string(&req).context("serializing request")?;
        let frame_bytes = frame.len();

        let (tx, rx) = oneshot::channel();
        let in_flight = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if !self.is_alive() {
                return Err(anyhow!("app-server connection closed"));
            }
            pending.insert(id.clone(), tx);
            pending.len()
        };
        let _registration = PendingRequest {
            pending: Arc::clone(&self.pending),
            id: id.clone(),
        };
        // How many requests are queued on this socket when this one starts. A
        // rising number across successive reads is the signature of callers
        // outpacing the socket rather than any single request being slow.
        tracing::debug!(
            target: "pocket_codex_codex::rpc",
            "-> {method} id={id} bytes={frame_bytes} in_flight={in_flight}"
        );
        let started = std::time::Instant::now();

        // Bound the write and sink lock too: a half-open peer can stop reading
        // before the request has even reached its response wait.
        let exchange = async {
            self.send_frame(frame)
                .await
                .with_context(|| format!("sending request `{method}`"))?;
            rx.await
                .map_err(|_| anyhow!("app-server connection closed"))?
        };
        match tokio::time::timeout(timeout, exchange).await {
            Ok(result) => {
                let elapsed = started.elapsed();
                let ok = result.is_ok();
                // Slow answers are the interesting ones; a server-side walk over
                // a long thread shows up here and nowhere else.
                if elapsed > std::time::Duration::from_secs(2) {
                    tracing::warn!(
                        target: "pocket_codex_codex::rpc",
                        "<- {method} id={id} SLOW {elapsed:?} ok={ok}"
                    );
                } else {
                    tracing::debug!(
                        target: "pocket_codex_codex::rpc",
                        "<- {method} id={id} {elapsed:?} ok={ok}"
                    );
                }
                result
            },
            Err(_) => {
                close_connection(&self.closed, &self.pending);
                tracing::error!(
                    target: "pocket_codex_codex::rpc",
                    "<- {method} id={id} TIMED OUT after {:?}", started.elapsed()
                );
                Err(anyhow!("request `{method}` timed out; app-server connection closed"))
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
        self.send_frame(frame)
            .await
            .with_context(|| format!("sending notification `{method}`"))
    }

    async fn send_frame(&self, frame: String) -> Result<()> {
        if !self.is_alive() {
            return Err(anyhow!("app-server connection closed"));
        }
        let result = tokio::select! {
            biased;
            _ = self.closed.cancelled() => return Err(anyhow!("app-server connection closed")),
            result = tokio::time::timeout(REQUEST_TIMEOUT, async {
                self.sink.lock().await.send(WsMessage::text(frame)).await
            }) => result,
        };
        match result {
            Ok(Ok(())) => Ok(()),
            result => {
                close_connection(&self.closed, &self.pending);
                Err(anyhow!("app-server connection closed while sending: {result:?}"))
            },
        }
    }
}

fn take_pending(pending: &Pending, id: &RequestId) -> Option<oneshot::Sender<Result<Value>>> {
    let key = match id {
        RequestId::String(s) => s.clone(),
        RequestId::Number(n) => n.to_string(),
    };
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key)
}

fn close_connection(closed: &CancellationToken, pending: &Pending) {
    closed.cancel();
    for (_, tx) in pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
        let _ = tx.send(Err(anyhow!("app-server connection closed")));
    }
}

// Synchronous removal also runs when an outer timeout or an aborted caller
// drops the request future. No pending lock is held across an await.
struct PendingRequest {
    pending: Pending,
    id: String,
}

impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

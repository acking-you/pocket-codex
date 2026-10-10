//! The Pocket-Codex ACP gateway (`/acp/v1`): how controllers reach a
//! host-owned ACP client over loopback or a relay tunnel.
//!
//! This is **not** ACP. ACP is spoken only between the host and its agent
//! subprocess (stdio JSON-RPC). Controllers use this small versioned
//! HTTP + SSE API instead, so one host-owned connection can serve several
//! devices, survive controller reconnects and keep permissions owned by the
//! host. Recovery is snapshot + watermark: `GET snapshot` returns the state
//! and its sequence number, `GET events?generation=G&after=S` continues
//! strictly after it, and a `reset` event means "read a new snapshot".
//!
//! Every mutation (`sessions/new`, `open`, `prompt`, `cancel`, `config`,
//! `mode`) may carry the `hostId` and `generation` its caller acted on; a
//! request for another host incarnation or generation is refused (`409
//! stale_host`, or `{"cancelled": false}` for a cancel) before anything
//! reaches the agent.

use std::{convert::Infallible, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{broadcast::error::RecvError, mpsc, Semaphore},
};
use tokio_stream::wrappers::ReceiverStream;

use super::{
    host::{AgentHost, HostError, Identity},
    state::LogEntry,
};

/// Most concurrent event streams per host.
pub const MAX_STREAMS: usize = 16;
/// Largest prompt request body (text plus inline images).
pub const PROMPT_BODY_LIMIT: usize = 48 * 1024 * 1024;
const STREAM_BUFFER: usize = 64;

#[derive(Clone)]
struct ApiState {
    host: AgentHost,
    streams: Arc<Semaphore>,
}

struct ApiError(HostError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            HostError::NotReady | HostError::Start(_) => StatusCode::SERVICE_UNAVAILABLE,
            HostError::UnknownSession | HostError::UnknownPermission => StatusCode::NOT_FOUND,
            HostError::Busy
            | HostError::Loading
            | HostError::AlreadyResolved
            | HostError::NotReopenable
            | HostError::Stale => StatusCode::CONFLICT,
            HostError::Capacity => StatusCode::TOO_MANY_REQUESTS,
            HostError::InvalidOption | HostError::Unsupported(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            HostError::InvalidInput(_) => StatusCode::BAD_REQUEST,
            HostError::AuthRequired => StatusCode::FORBIDDEN,
            HostError::Agent(_) => StatusCode::BAD_GATEWAY,
            HostError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            HostError::Limit => StatusCode::TOO_MANY_REQUESTS,
        };
        let body = json!({"code": self.0.code(), "message": self.0.to_string()});
        (status, Json(body)).into_response()
    }
}

type ApiResult = std::result::Result<Json<Value>, ApiError>;

/// The gateway routes for `host`.
pub fn router(host: AgentHost) -> Router {
    let state = ApiState {
        host,
        streams: Arc::new(Semaphore::new(MAX_STREAMS)),
    };
    Router::new()
        .route("/acp/v1/info", get(info))
        .route("/acp/v1/snapshot", get(snapshot))
        .route("/acp/v1/events", get(events))
        .route("/acp/v1/sessions", get(list_sessions))
        .route("/acp/v1/sessions/new", post(new_session))
        .route("/acp/v1/sessions/open", post(open_session))
        .route("/acp/v1/sessions/history", get(history))
        .route(
            "/acp/v1/sessions/prompt",
            post(prompt).layer(DefaultBodyLimit::max(PROMPT_BODY_LIMIT)),
        )
        .route("/acp/v1/sessions/cancel", post(cancel))
        .route("/acp/v1/sessions/config", post(set_config))
        .route("/acp/v1/sessions/mode", post(set_mode))
        .route("/acp/v1/permissions/answer", post(answer))
        .with_state(state)
}

/// Serve the gateway on an already-bound listener until the task is dropped.
pub async fn serve(listener: TcpListener, host: AgentHost) -> Result<()> {
    axum::serve(listener, router(host))
        .await
        .context("running the ACP gateway")
}

async fn info(State(state): State<ApiState>) -> Json<Value> {
    Json(state.host.info())
}

async fn snapshot(State(state): State<ApiState>) -> Json<Value> {
    Json(state.host.snapshot())
}

#[derive(Deserialize)]
struct EventsQuery {
    /// The host incarnation the caller's watermark belongs to.
    host: Option<String>,
    generation: u64,
    after: u64,
}

fn sse_event(entry: &LogEntry) -> Event {
    Event::default()
        .id(entry.seq.to_string())
        .data(&*entry.body)
}

fn reset_event(reason: &str) -> Event {
    Event::default().data(json!({"type": "reset", "reason": reason}).to_string())
}

async fn events(State(state): State<ApiState>, Query(query): Query<EventsQuery>) -> Response {
    let Ok(permit) = state.streams.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many event streams").into_response();
    };
    let (tx, rx) = mpsc::channel::<std::result::Result<Event, Infallible>>(STREAM_BUFFER);
    // A watermark of another host incarnation means nothing here.
    let same_host = query
        .host
        .as_deref()
        .is_none_or(|host| host == state.host.host_id());
    let subscription = same_host
        .then(|| state.host.subscribe(query.generation, query.after))
        .flatten();
    tokio::spawn(async move {
        let _permit = permit;
        let Some((backlog, mut live)) = subscription else {
            let reason = if same_host { "resync" } else { "host" };
            let _ = tx.send(Ok(reset_event(reason))).await;
            return;
        };
        let mut last = query.after;
        for entry in backlog {
            last = entry.seq;
            if tx.send(Ok(sse_event(&entry))).await.is_err() {
                return;
            }
        }
        loop {
            // The response stream owns `rx`: when the controller goes away,
            // stop at once — even if the host stays idle — so the stream
            // permit and this task are released.
            let next = tokio::select! {
                () = tx.closed() => return,
                next = live.recv() => next,
            };
            match next {
                Ok(entry) if entry.seq <= last => {},
                Ok(entry) if entry.generation != query.generation => {
                    let _ = tx.send(Ok(reset_event("generation"))).await;
                    return;
                },
                Ok(entry) => {
                    last = entry.seq;
                    if tx.send(Ok(sse_event(&entry))).await.is_err() {
                        return;
                    }
                },
                Err(RecvError::Lagged(_)) => {
                    let _ = tx.send(Ok(reset_event("lagged"))).await;
                    return;
                },
                Err(RecvError::Closed) => return,
            }
        }
    });
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Deserialize)]
struct ListQuery {
    cursor: Option<String>,
}

async fn list_sessions(State(state): State<ApiState>, Query(query): Query<ListQuery>) -> ApiResult {
    state
        .host
        .list_sessions(query.cursor)
        .await
        .map(Json)
        .map_err(ApiError)
}

/// The identity fields a mutation may carry (`hostId`, `generation`).
fn expected(identity: &Identity) -> Option<&Identity> {
    (identity.host_id.is_some() || identity.generation.is_some()).then_some(identity)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewBody {
    cwd: String,
    #[serde(flatten)]
    identity: Identity,
}

async fn new_session(State(state): State<ApiState>, Json(body): Json<NewBody>) -> ApiResult {
    state
        .host
        .new_session(&body.cwd, expected(&body.identity))
        .await
        .map(Json)
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBody {
    session_id: String,
    #[serde(flatten)]
    identity: Identity,
}

async fn open_session(State(state): State<ApiState>, Json(body): Json<SessionBody>) -> ApiResult {
    state
        .host
        .open_session(&body.session_id, expected(&body.identity))
        .await
        .map(Json)
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryQuery {
    session_id: String,
    before: Option<String>,
    turn: Option<String>,
    limit: Option<usize>,
}

async fn history(State(state): State<ApiState>, Query(query): Query<HistoryQuery>) -> ApiResult {
    state
        .host
        .history(&query.session_id, query.before.as_deref(), query.turn.as_deref(), query.limit)
        .map(Json)
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptBody {
    session_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<String>,
    /// The host and generation the caller believes are current; a stale
    /// one is refused rather than started in a replacement process.
    #[serde(flatten)]
    identity: Identity,
}

async fn prompt(State(state): State<ApiState>, Json(body): Json<PromptBody>) -> ApiResult {
    state
        .host
        .prompt(&body.session_id, &body.text, &body.images, expected(&body.identity))
        .await
        .map(|turn| Json(json!({"turnId": turn})))
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelBody {
    session_id: String,
    /// Cancel only this turn; a different (newer) turn is left alone.
    #[serde(default)]
    turn_id: Option<String>,
    /// Cancel only on this host and generation.
    #[serde(flatten)]
    identity: Identity,
}

async fn cancel(State(state): State<ApiState>, Json(body): Json<CancelBody>) -> ApiResult {
    state
        .host
        .cancel(&body.session_id, body.turn_id.as_deref(), expected(&body.identity))
        .await
        .map(|cancelled| Json(json!({"cancelled": cancelled})))
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBody {
    session_id: String,
    config_id: String,
    value: String,
    #[serde(flatten)]
    identity: Identity,
}

async fn set_config(State(state): State<ApiState>, Json(body): Json<ConfigBody>) -> ApiResult {
    state
        .host
        .set_config_option(&body.session_id, &body.config_id, &body.value, expected(&body.identity))
        .await
        .map(Json)
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModeBody {
    session_id: String,
    mode_id: String,
    #[serde(flatten)]
    identity: Identity,
}

async fn set_mode(State(state): State<ApiState>, Json(body): Json<ModeBody>) -> ApiResult {
    state
        .host
        .set_mode(&body.session_id, &body.mode_id, expected(&body.identity))
        .await
        .map(Json)
        .map_err(ApiError)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerBody {
    handle: String,
    option_id: String,
}

async fn answer(State(state): State<ApiState>, Json(body): Json<AnswerBody>) -> ApiResult {
    state
        .host
        .answer_permission(&body.handle, &body.option_id)
        .await
        .map(|()| Json(json!({})))
        .map_err(ApiError)
}

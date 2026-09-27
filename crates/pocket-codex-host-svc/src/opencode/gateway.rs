//! Restricted HTTP gateway for an attached OpenCode instance.

use std::{collections::HashMap, convert::Infallible, net::SocketAddr, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::{net::TcpListener, task::JoinHandle};

use super::{
    connection::{Connection, GatewayInfo, NativeMessage, NativePage},
    Capabilities, Error, Health, PermissionReply, PromptInput, Session,
};

/// A loopback-only HTTP gateway that owns no OpenCode process.
#[derive(Clone)]
pub struct OpenCodeGateway {
    client: Arc<Connection>,
}

/// Owns only the Pocket-Codex gateway task, never the upstream process.
pub struct GatewayHandle {
    task: JoinHandle<()>,
}

impl GatewayHandle {
    /// Stop accepting gateway traffic without stopping OpenCode.
    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Serve a gateway on a pre-bound loopback listener.
pub async fn serve(
    listener: TcpListener,
    client: impl Into<Connection>,
) -> anyhow::Result<GatewayHandle> {
    OpenCodeGateway::new(client)
        .serve(listener)
        .map_err(Into::into)
}

impl OpenCodeGateway {
    /// Create a gateway around an already configured upstream client.
    pub fn new(client: impl Into<Connection>) -> Self {
        Self {
            client: Arc::new(client.into()),
        }
    }

    /// Bind a loopback listener and return it together with the router.
    pub async fn bind(self, address: SocketAddr) -> Result<(TcpListener, Router), std::io::Error> {
        if !address.ip().is_loopback() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "OpenCode gateway must bind to loopback",
            ));
        }
        let listener = TcpListener::bind(address).await?;
        let router = self.router();
        Ok((listener, router))
    }

    /// Serve on a caller-owned loopback listener until the returned handle is
    /// stopped.
    pub fn serve(self, listener: TcpListener) -> Result<GatewayHandle, std::io::Error> {
        if !listener.local_addr()?.ip().is_loopback() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "OpenCode gateway must bind to loopback",
            ));
        }
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, self.router()).await {
                tracing::warn!(%error, "OpenCode gateway stopped");
            }
        });
        Ok(GatewayHandle {
            task,
        })
    }

    /// Build the restricted gateway router.
    pub(crate) fn router(&self) -> Router {
        let router = Router::new()
            .route("/pocket/opencode/v2/info", get(gateway_info))
            .route("/pocket/opencode/v2/session", get(list_sessions).post(create_session))
            .route("/pocket/opencode/v2/session/status", get(status))
            .route("/pocket/opencode/v2/session/{session_id}", get(session))
            .route("/pocket/opencode/v2/session/{session_id}/message", get(native_history))
            .route("/pocket/opencode/v2/session/{session_id}/message/{message_id}", get(message))
            .route("/pocket/opencode/v2/session/{session_id}/prompt", post(native_prompt))
            .route("/pocket/opencode/v2/session/{session_id}/abort", post(abort))
            .route("/pocket/opencode/v2/event", get(events))
            .route("/pocket/opencode/v2/permission", get(permissions))
            .route("/pocket/opencode/v2/permission/{request_id}/reply", post(reply_permission))
            .route("/pocket/opencode/v2/question", get(questions))
            .route("/pocket/opencode/v2/question/{request_id}/reply", post(reply_question))
            .route("/pocket/opencode/v2/question/{request_id}/reject", post(reject_question))
            .route("/pocket/opencode/v2/form/{request_id}/reply", post(reply_form));
        let router = if self.client.is_v2() {
            router
        } else {
            router
                .route("/global/health", get(health))
                .route("/doc", get(doc))
                .route("/capabilities", get(capabilities))
                .route("/session", get(list_sessions).post(create_session))
                .route("/session/status", get(status))
                .route("/session/{session_id}", get(session))
                .route("/session/{session_id}/message", get(history))
                .route("/session/{session_id}/message/{message_id}", get(message))
                .route("/session/{session_id}/prompt_async", post(prompt))
                .route("/session/{session_id}/abort", post(abort))
                .route("/event", get(events))
                .route("/permission", get(permissions))
                .route("/permission/{request_id}/reply", post(reply_permission))
                .route("/question", get(questions))
                .route("/question/{request_id}/reply", post(reply_question))
                .route("/question/{request_id}/reject", post(reject_question))
        };
        router
            .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
            .with_state(self.clone())
    }
}

#[derive(Debug, Deserialize)]
struct DirectoryQuery {
    directory: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionQuery {
    directory: Option<String>,
    search: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    directory: Option<String>,
    limit: Option<u32>,
    before: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInput {
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionInput {
    reply: PermissionReply,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionInput {
    answers: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FormInput {
    answer: Value,
}

#[derive(Debug)]
struct GatewayError(StatusCode);

impl From<Error> for GatewayError {
    fn from(error: Error) -> Self {
        let status = match error {
            Error::InvalidInput => StatusCode::BAD_REQUEST,
            Error::Scope => StatusCode::FORBIDDEN,
            Error::Rejected(code) => StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY),
            Error::Limit => StatusCode::PAYLOAD_TOO_LARGE,
            Error::Transport | Error::SubmissionUnknown | Error::Disconnected => {
                StatusCode::BAD_GATEWAY
            },
            Error::Protocol => StatusCode::BAD_GATEWAY,
        };
        Self(status)
    }
}

impl axum::response::IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(serde_json::json!({"error":"OpenCode gateway request failed"})))
            .into_response()
    }
}

fn check_directory(client: &Connection, directory: Option<&str>) -> Result<(), GatewayError> {
    if directory.is_some_and(|value| value != client.directory()) {
        return Err(GatewayError(StatusCode::FORBIDDEN));
    }
    Ok(())
}

async fn health(State(gateway): State<OpenCodeGateway>) -> Result<Json<Health>, GatewayError> {
    let info = gateway.client.gateway_info().await?;
    Ok(Json(Health {
        healthy: true,
        version: info.version,
    }))
}

async fn gateway_info(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<GatewayInfo>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.gateway_info().await?))
}

async fn doc() -> Json<Value> {
    let mut paths = serde_json::Map::new();
    for (path, method) in super::client::REQUIRED_ROUTES {
        paths
            .entry((*path).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(route) = paths.get_mut(*path).and_then(Value::as_object_mut) {
            route.insert((*method).to_string(), serde_json::json!({}));
        }
    }
    Json(serde_json::json!({"openapi": "3.1.0", "paths": paths}))
}

async fn capabilities(
    State(gateway): State<OpenCodeGateway>,
) -> Result<Json<Capabilities>, GatewayError> {
    let info = gateway.client.gateway_info().await?;
    Ok(Json(Capabilities {
        tested_version: matches!(info.version.as_str(), "1.18.32" | "2.0.18"),
        version: info.version,
    }))
}

async fn list_sessions(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Vec<Session>>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    let sessions = gateway.client.sessions(query.search.as_deref()).await?;
    if query.limit.is_some_and(|limit| !(1..=100).contains(&limit)) {
        return Err(GatewayError(StatusCode::BAD_REQUEST));
    }
    Ok(Json(match query.limit {
        Some(limit) => sessions.into_iter().take(limit as usize).collect(),
        None => sessions,
    }))
}

async fn create_session(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<CreateInput>,
) -> Result<Json<Session>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.create(input.title.as_deref()).await?))
}

async fn status(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<HashMap<String, Value>>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.status().await?.into_iter().collect()))
}

async fn session(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Session>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.session(&session_id).await?))
}

async fn history(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Response, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    let page: NativePage = gateway
        .client
        .history(&session_id, query.limit.unwrap_or(20), query.before.as_deref())
        .await?;
    let mut headers = HeaderMap::new();
    if let Some(cursor) = page.next_cursor {
        headers.insert(
            "x-next-cursor",
            cursor
                .parse()
                .map_err(|_| GatewayError(StatusCode::BAD_GATEWAY))?,
        );
    }
    Ok((headers, Json(page.messages)).into_response())
}

async fn native_history(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<NativePage>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(
        gateway
            .client
            .history(&session_id, query.limit.unwrap_or(20), query.before.as_deref())
            .await?,
    ))
}

async fn message(
    State(gateway): State<OpenCodeGateway>,
    Path((session_id, message_id)): Path<(String, String)>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<NativeMessage>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.message(&session_id, &message_id).await?))
}

async fn prompt(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<PromptInput>,
) -> Result<StatusCode, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway.client.prompt(&session_id, &input).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn native_prompt(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<PromptInput>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway.client.prompt(&session_id, &input).await?;
    Ok(Json(true))
}

async fn abort(
    State(gateway): State<OpenCodeGateway>,
    Path(session_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway.client.abort(&session_id).await?;
    Ok(Json(true))
}

async fn permissions(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<super::connection::NativePermission>>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.permissions().await?))
}

async fn reply_permission(
    State(gateway): State<OpenCodeGateway>,
    Path(request_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<PermissionInput>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway
        .client
        .reply_permission(&request_id, input.reply, input.message.as_deref())
        .await?;
    Ok(Json(true))
}

async fn questions(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<super::connection::NativeQuestion>>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    Ok(Json(gateway.client.questions().await?))
}

async fn reply_question(
    State(gateway): State<OpenCodeGateway>,
    Path(request_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<QuestionInput>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway
        .client
        .reply_question(&request_id, input.answers)
        .await?;
    Ok(Json(true))
}

async fn reject_question(
    State(gateway): State<OpenCodeGateway>,
    Path(request_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway.client.reject_question(&request_id).await?;
    Ok(Json(true))
}

async fn reply_form(
    State(gateway): State<OpenCodeGateway>,
    Path(request_id): Path<String>,
    Query(query): Query<DirectoryQuery>,
    Json(input): Json<FormInput>,
) -> Result<Json<bool>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    gateway.client.reply_form(&request_id, input.answer).await?;
    Ok(Json(true))
}

async fn events(
    State(gateway): State<OpenCodeGateway>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, GatewayError> {
    check_directory(&gateway.client, query.directory.as_deref())?;
    let stream = gateway.client.events().await?.scan(false, |done, item| {
        let frame = match item {
            Ok(event) => Event::default().event(event.kind()).json_data(event).ok(),
            Err(error) => {
                tracing::debug!(%error, "OpenCode gateway event stream interrupted");
                *done = true;
                None
            },
        };
        std::future::ready(if *done { None } else { frame.map(Ok::<_, Infallible>) })
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

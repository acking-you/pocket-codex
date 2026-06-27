//! The HTTP API: GitHub device-flow auth, session refresh/logout, and the
//! per-account `/v1/me` + `/v1/services` views.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use pocket_codex_account_proto::{
    http::{
        DevicePollRequest, DevicePollResponse, DeviceStartRequest, DeviceStartResponse,
        LogoutRequest, MeResponse, RefreshRequest, RefreshResponse, ServiceEntry, ServicesResponse,
    },
    key::NamespacedServiceId,
};
use pocket_codex_auth::{Auth, AuthError, Claims};
use pocket_codex_broker_server::BrokerServer;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer, trace::TraceLayer};

/// Shared state for the HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// The identity/session service.
    pub auth: Arc<Auth>,
    /// Loopback relay address, queried for the account's service listing.
    pub relay_addr: SocketAddr,
    /// The broker, used to force-deregister a caller's relay key on demand.
    pub broker: BrokerServer,
}

/// Build the HTTP API router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/auth/device/start", post(device_start))
        .route("/auth/device/poll", post(device_poll))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/v1/me", get(me))
        .route("/v1/services", get(services))
        .route("/v1/services/{device}/{kind}/{name}", delete(deregister_service))
        // Bound every request so a slow upstream (GitHub) can't pin connections
        // on the unauthenticated /auth/* surface indefinitely.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(20),
        ))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// API error mapped to an HTTP status + JSON `{ "error": … }`.
enum ApiError {
    Unauthorized,
    BadRequest(&'static str),
    Internal(String),
}

type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Internal(detail) => {
                // Log the detail server-side but never leak raw upstream/store/JWT
                // error strings (which fingerprint the backend) to the client.
                tracing::error!(error = %detail, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            },
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

fn auth_err(err: AuthError) -> ApiError {
    match err {
        AuthError::BadRefresh => ApiError::Unauthorized,
        other => ApiError::Internal(other.to_string()),
    }
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn authed(state: &AppState, headers: &HeaderMap) -> ApiResult<Claims> {
    let token = bearer(headers).ok_or(ApiError::Unauthorized)?;
    state.auth.verify(token).map_err(|_| ApiError::Unauthorized)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn device_start(
    State(state): State<AppState>,
    Json(req): Json<DeviceStartRequest>,
) -> ApiResult<Json<DeviceStartResponse>> {
    let resp = state
        .auth
        .device_start(req.device_label.as_deref(), now())
        .await
        .map_err(auth_err)?;
    Ok(Json(resp))
}

async fn device_poll(
    State(state): State<AppState>,
    Json(req): Json<DevicePollRequest>,
) -> ApiResult<Json<DevicePollResponse>> {
    let resp = state
        .auth
        .device_poll(&req.poll_handle, now())
        .await
        .map_err(auth_err)?;
    Ok(Json(resp))
}

async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<RefreshResponse>> {
    let credential = state
        .auth
        .refresh(&req.refresh_token, now())
        .await
        .map_err(auth_err)?;
    Ok(Json(RefreshResponse {
        credential,
    }))
}

async fn logout(
    State(state): State<AppState>,
    Json(req): Json<LogoutRequest>,
) -> ApiResult<StatusCode> {
    state
        .auth
        .logout(&req.refresh_token, now())
        .await
        .map_err(auth_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<MeResponse>> {
    let claims = authed(&state, &headers)?;
    Ok(Json(MeResponse {
        login: claims.login,
        account_id: Some(claims.gh_id.to_string()),
    }))
}

async fn services(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<ServicesResponse>> {
    let claims = authed(&state, &headers)?;
    let prefix = NamespacedServiceId::user_prefix(&claims.sub);
    let keys = pocket_codex_pb::keys(state.relay_addr)
        .await
        .map_err(|e| ApiError::Internal(format!("relay status: {e}")))?;
    let services = keys
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .filter_map(|k| NamespacedServiceId::parse_key(&k))
        .map(|nsid| ServiceEntry {
            device: nsid.service.device,
            kind: nsid.service.kind,
            name: nsid.service.name,
        })
        .collect();
    Ok(Json(ServicesResponse {
        services,
    }))
}

/// Force-deregister one of the caller's own services from the relay. The relay
/// key is derived server-side from the verified user id, so a caller can only
/// ever drop keys in their own `pcxu:<user>:` namespace. Best-effort: a client
/// still hosting the service will reconnect and re-register shortly after.
async fn deregister_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((device, kind, name)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let claims = authed(&state, &headers)?;
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid service kind"))?;
    let relay_key =
        NamespacedServiceId::new(&claims.sub, ServiceId::new(&device, kind, &name)).key();
    state.broker.deregister_key(&relay_key).await;
    Ok(StatusCode::NO_CONTENT)
}

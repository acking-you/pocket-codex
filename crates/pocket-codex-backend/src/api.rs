//! The HTTP API: GitHub device-flow auth, session refresh/logout, and the
//! per-account `/v1/me` + `/v1/services` views.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
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
use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer, trace::TraceLayer};

/// Shared state for the HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// The identity/session service.
    pub auth: Arc<Auth>,
    /// Loopback relay address, queried for the account's service listing.
    pub relay_addr: SocketAddr,
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
    Internal(String),
}

type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
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

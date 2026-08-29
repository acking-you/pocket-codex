//! The HTTP API: GitHub device-flow auth, session refresh/logout, the
//! per-account `/v1/me` + `/v1/services` views, and `/v1/relay` — the endpoint
//! that hands a client its own relay credential so the backend leaves the data
//! path entirely.

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use pocket_codex_account_proto::{
    http::{
        DevicePollRequest, DevicePollResponse, DeviceStartRequest, DeviceStartResponse,
        LogoutRequest, MeResponse, RefreshRequest, RefreshResponse, RelayCredentialResponse,
        ServiceEntry, ServicesResponse, WebExchangeRequest, WebExchangeResponse, WebStartRequest,
        WebStartResponse,
    },
    key::NamespacedServiceId,
};
use pocket_codex_auth::{Auth, AuthError, Claims};
use pocket_codex_core::service::{ServiceId, ServiceKind};
use serde::Deserialize;
use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer, trace::TraceLayer};

/// Shared state for the HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// The identity/session service.
    pub auth: Arc<Auth>,
    /// The relay plus the ADMINISTRATOR credential, for the two things the
    /// backend still does on an account's behalf: listing its services across
    /// namespaces, and retiring a registration whose owner is gone.
    pub relay: pocket_codex_pb::RelaySession,
    /// Vends each account its own short-lived relay credential.
    pub credentials: crate::credentials::Credentials,
}

/// Build the HTTP API router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/auth/device/start", post(device_start))
        .route("/auth/device/poll", post(device_poll))
        .route("/auth/web/start", post(web_start))
        .route("/auth/web/callback", get(web_callback))
        .route("/auth/web/exchange", post(web_exchange))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/v1/me", get(me))
        .route("/v1/services", get(services))
        .route("/v1/relay", get(relay_credential))
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
    Unavailable(&'static str),
    Internal(String),
}

type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
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
        AuthError::BadRefresh | AuthError::BadExchange => ApiError::Unauthorized,
        AuthError::WebDisabled => ApiError::Unavailable("web login is not configured"),
        AuthError::BadRedirect => ApiError::BadRequest("redirect_uri is not allowed"),
        AuthError::BadWebState => ApiError::BadRequest("invalid or expired web login state"),
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

async fn web_start(
    State(state): State<AppState>,
    Json(req): Json<WebStartRequest>,
) -> ApiResult<Json<WebStartResponse>> {
    let resp = state
        .auth
        .web_start(
            &req.redirect_uri,
            &req.state,
            &req.code_challenge,
            req.device_label.as_deref(),
            now(),
        )
        .await
        .map_err(auth_err)?;
    Ok(Json(resp))
}

/// Query params GitHub appends to the backend's OAuth callback. `code` +
/// `state` on success; `error` (+ `state`) when the user denied or GitHub
/// failed.
#[derive(Debug, Deserialize)]
struct WebCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// GitHub's redirect lands here. We never return JSON to the browser: on a
/// recognized flow we 302 the browser back to the app (custom scheme /
/// loopback) with the one-time exchange code or an error; on an unknown/expired
/// state (or when the flow is disabled) we render a generic page instead of
/// redirecting somewhere unvalidated.
async fn web_callback(
    State(state): State<AppState>,
    Query(q): Query<WebCallbackQuery>,
) -> Response {
    let gh_state = q.state.unwrap_or_default();
    match state
        .auth
        .web_callback(q.code.as_deref(), &gh_state, q.error.as_deref(), now())
        .await
    {
        Ok(redirect) => Redirect::to(&redirect.location).into_response(),
        Err(AuthError::BadWebState | AuthError::WebDisabled) => web_callback_page(
            "This sign-in link is no longer valid. Return to Pocket-Codex and try again.",
        ),
        Err(e) => {
            tracing::error!(error = %e, "web callback failed");
            web_callback_page(
                "Something went wrong signing in. Return to Pocket-Codex and try again.",
            )
        },
    }
}

/// A minimal self-contained HTML page shown when the callback can't redirect
/// back into the app (unknown state, disabled flow, or an internal error).
fn web_callback_page(message: &str) -> Response {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width, initial-scale=1\"><title>Pocket-Codex</title></head><body \
         style=\"font-family: system-ui, sans-serif; max-width: 28rem; margin: 4rem auto; \
         padding: 0 1.5rem; text-align: center; color: #1a1a1a;\"><h1 style=\"font-size: \
         1.25rem;\">Pocket-Codex</h1><p>{message}</p></body></html>"
    );
    Html(body).into_response()
}

async fn web_exchange(
    State(state): State<AppState>,
    Json(req): Json<WebExchangeRequest>,
) -> ApiResult<Json<WebExchangeResponse>> {
    let credential = state
        .auth
        .web_exchange(&req.exchange_code, &req.code_verifier, now())
        .await
        .map_err(auth_err)?;
    Ok(Json(WebExchangeResponse {
        credential,
    }))
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
    let keys = pocket_codex_pb::all_service_keys(&state.relay)
        .await
        .map_err(|e| ApiError::Internal(format!("relay status: {e}")))?;
    let services = keys
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .filter_map(|k| NamespacedServiceId::parse_key(&k))
        // Only surface the kinds clients consume directly. The meta service is a
        // colocated implementation detail (clients derive its key from the app
        // key), and an unrecognised kind must never be sent — an older client
        // can't deserialize a kind it doesn't know and would fail the whole
        // listing, so omit both here.
        .filter(|nsid| matches!(nsid.service.kind, ServiceKind::App | ServiceKind::Api))
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
    // Asks the RELAY to drop the registration's connections. It used to cancel a
    // broker session that owned the tunnel; with clients registering directly the
    // backend no longer holds anything to cancel, so the authority it does have —
    // its administrator credential — is what retires the key.
    //
    // The key is derived from the verified token, so a caller can only ever name
    // a service inside its own namespace.
    pocket_codex_pb::retire_service(&state.relay, &relay_key)
        .await
        .map_err(|e| ApiError::Internal(format!("retiring {relay_key}: {e:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Hand this account the relay address and a short-lived credential, so the
/// client can register and subscribe against the relay directly.
///
/// This is the endpoint that takes the backend off the data path: everything
/// after it is client↔relay. The credential is per-account (see
/// [`crate::credentials`]) and scoped to the account's namespace by the relay
/// itself, so possession of it grants nothing outside that account.
async fn relay_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<RelayCredentialResponse>> {
    let claims = authed(&state, &headers)?;
    let (credential, expires_at) = state
        .credentials
        .for_account(&claims.sub)
        .await
        .map_err(|e| ApiError::Internal(format!("issuing a relay credential: {e:#}")))?;
    Ok(Json(RelayCredentialResponse {
        relay_addr: state.credentials.relay_addr().to_string(),
        credential,
        expires_at,
        // From the VERIFIED token, never from the request: this is the one place
        // an account's namespace is decided, and a client that asked for another
        // one would still be confined by the credential it gets back.
        namespace: NamespacedServiceId::namespace_of(&claims.sub),
    }))
}

//! Negotiation tests through the same public interface used by the bridge.

use axum::{http::StatusCode, routing::get, Json, Router};
use pocket_codex_host_svc::opencode::{connection::Connection, BasicCredentials};
use serde_json::json;

#[tokio::test]
async fn unauthenticated_v2_does_not_become_an_offline_or_legacy_connection() -> anyhow::Result<()>
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let app = Router::new()
        .route("/global/health", get(|| async { axum::response::Html("<html>OpenCode</html>") }))
        .route("/api/info", get(|| async { StatusCode::UNAUTHORIZED }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let result = Connection::connect(&origin, "/fixture", None).await;
    server.abort();
    assert!(matches!(result, Err(pocket_codex_host_svc::opencode::Error::Rejected(401))));
    Ok(())
}

#[tokio::test]
async fn a_gateway_must_advertise_an_explicit_supported_contract() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let app = Router::new().route(
        "/pocket/opencode/v2/info",
        get(|| async {
            Json(json!({"gateway_protocol":99,"upstream_protocol":"v2","version":"2.0.18"}))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let result = Connection::connect(
        &origin,
        "/fixture",
        Some(BasicCredentials::new("opencode", "secret-canary")),
    )
    .await;
    server.abort();
    assert!(result.is_err());
    assert!(!format!("{result:?}").contains("secret-canary"));
    Ok(())
}

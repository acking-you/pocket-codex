//! HTTP contract tests for the restricted OpenCode gateway.

use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{extract::Query, http::HeaderMap, response::IntoResponse, routing::get, Json, Router};
use bytes::Bytes;
use futures::{stream, StreamExt};
use pocket_codex_host_svc::opencode::{BasicCredentials, OpenCodeClient, OpenCodeGateway};
use reqwest::header::{AUTHORIZATION, COOKIE};
use serde_json::json;
use tokio::sync::Mutex;

async fn spawn_upstream(
    seen_headers: Arc<Mutex<Option<HeaderMap>>>,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let app = Router::new()
        .route(
            "/global/health",
            get({
                let seen_headers = seen_headers.clone();
                move |headers: HeaderMap| {
                    let seen_headers = seen_headers.clone();
                    async move {
                        *seen_headers.lock().await = Some(headers);
                        Json(json!({"healthy": true, "version": "1.18.32"}))
                    }
                }
            }),
        )
        .route(
            "/session/ses_other",
            get(|| async { Json(json!({"id":"ses_other","title":"Other","directory":"/other"})) }),
        )
        .route(
            "/event",
            get(|| async {
                let first = stream::once(async {
                    Ok::<_, std::io::Error>(Bytes::from(
                        "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
                    ))
                });
                let never = stream::pending::<Result<Bytes, std::io::Error>>();
                let body = axum::body::Body::from_stream(first.chain(never));
                ([("content-type", "text/event-stream")], body).into_response()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((origin, task))
}

#[tokio::test]
async fn gateway_keeps_controller_headers_out_of_upstream_and_does_not_stop_it(
) -> anyhow::Result<()> {
    let seen_headers = Arc::new(Mutex::new(None));
    let (origin, upstream) = spawn_upstream(seen_headers.clone()).await?;
    let upstream_client = OpenCodeClient::new(
        &origin,
        "/project",
        Some(BasicCredentials::new("opencode", "canary")),
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = listener.local_addr()?;
    let gateway = OpenCodeGateway::new(upstream_client).serve(listener)?;
    let gateway_url = format!("http://{gateway_addr}");
    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/global/health"))
        .header(AUTHORIZATION, "Bearer controller-secret")
        .header(COOKIE, "session=controller-secret")
        .send()
        .await?;
    assert!(response.status().is_success());
    let headers = seen_headers
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("upstream was not called"))?;
    assert!(headers.get(COOKIE).is_none());
    assert_eq!(
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("Basic ")),
        Some(true)
    );

    gateway.stop().await;
    let health = reqwest::get(format!("{origin}/global/health")).await?;
    assert!(health.status().is_success(), "stopping the gateway must not stop upstream OpenCode");
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_rejects_scope_violations_and_unlisted_routes() -> anyhow::Result<()> {
    let seen_headers = Arc::new(Mutex::new(None));
    let (origin, upstream) = spawn_upstream(seen_headers).await?;
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = listener.local_addr()?;
    let gateway = OpenCodeGateway::new(client).serve(listener)?;
    let http = reqwest::Client::new();
    let outside = http
        .get(format!("http://{gateway_addr}/session/ses_other"))
        .send()
        .await?;
    assert!(matches!(
        outside.status(),
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::FORBIDDEN
    ));
    let gateway_client = OpenCodeClient::new(&format!("http://{gateway_addr}"), "/project", None)?;
    assert!(gateway_client.capabilities().await?.tested_version);
    for path in ["/config", "/auth", "/dispose", "/shell", "/pty"] {
        let response = http
            .get(format!("http://{gateway_addr}{path}"))
            .send()
            .await?;
        assert!(
            matches!(
                response.status(),
                reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
            ),
            "unexpected status for {path}: {}",
            response.status()
        );
    }
    gateway.stop().await;
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_sse_forwards_a_frame_without_buffering_the_upstream_stream() -> anyhow::Result<()>
{
    let seen_headers = Arc::new(Mutex::new(None));
    let (origin, upstream) = spawn_upstream(seen_headers).await?;
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = listener.local_addr()?;
    let gateway = OpenCodeGateway::new(client).serve(listener)?;
    let mut response = reqwest::Client::new()
        .get(format!("http://{gateway_addr}/event"))
        .send()
        .await?;
    let first = tokio::time::timeout(Duration::from_secs(2), response.chunk())
        .await??
        .ok_or_else(|| anyhow::anyhow!("gateway closed SSE before first frame"))?;
    assert!(
        first
            .windows(b"server.connected".len())
            .any(|window| window == b"server.connected"),
        "gateway should stream the first SSE frame promptly"
    );
    gateway.stop().await;
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_preserves_bounded_history_cursor_and_filters_foreign_pending_requests(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/session/ses_one", get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }))
        .route("/session/ses_other", get(|| async { Json(json!({"id":"ses_other","title":"Other","directory":"/other"})) }))
        .route("/session/ses_one/message", get(|Query(query): Query<HashMap<String, String>>| async move {
            assert_eq!(query["limit"], "20");
            assert_eq!(query["directory"], "/project");
            ([ ("x-next-cursor", "opaque+/=") ], Json(json!([])))
        }))
        .route("/permission", get(|| async { Json(json!([
            {"id":"per_one","sessionID":"ses_one","permission":"bash","patterns":["cargo test"],"metadata":{},"always":[]},
            {"id":"per_other","sessionID":"ses_other","permission":"bash","patterns":["private"],"metadata":{},"always":[]}
        ])) }));
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", upstream_listener.local_addr()?);
    let upstream = tokio::spawn(async move { axum::serve(upstream_listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let gateway = OpenCodeGateway::new(client).serve(listener)?;
    let history = reqwest::get(format!("http://{addr}/session/ses_one/message?limit=20")).await?;
    assert_eq!(
        history
            .headers()
            .get("x-next-cursor")
            .and_then(|value| value.to_str().ok()),
        Some("opaque+/=")
    );
    assert_eq!(history.json::<serde_json::Value>().await?, json!([]));
    let pending = reqwest::get(format!("http://{addr}/permission"))
        .await?
        .json::<serde_json::Value>()
        .await?;
    assert_eq!(pending.as_array().map(Vec::len), Some(1));
    assert_eq!(pending[0]["id"], "per_one");
    gateway.stop().await;
    upstream.abort();
    Ok(())
}

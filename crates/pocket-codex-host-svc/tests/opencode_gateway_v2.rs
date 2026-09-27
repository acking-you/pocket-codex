//! Loopback integration contracts for the versioned Pocket gateway over
//! OpenCode v2.

use std::collections::HashMap;

use axum::{
    extract::Query,
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use bytes::Bytes;
use futures::{stream, StreamExt};
use pocket_codex_host_svc::opencode::{
    connection::Connection, v2::V2Client, OpenCodeGateway, PromptInput,
};
use serde_json::{json, Value};

fn contract() -> Value {
    serde_json::from_str(include_str!("fixtures/opencode_v2_contract.json"))
        .expect("the checked-in v2 contract fixture is valid JSON")
}

async fn serve_upstream() -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                Json(json!({
                    "version":"2.0.18",
                    "pid":4242,
                    "urls":[],
                    "paths":{"tmp":"/tmp"}
                }))
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }))
        .route(
            "/api/session",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query.get("directory").map(String::as_str), Some("/project"));
                assert_eq!(query.get("limit").map(String::as_str), Some("100"));
                Json(json!({
                    "data":[{"id":"ses_one","title":"Native gateway","location":{"directory":"/project"}}],
                    "cursor":{"previous":null,"next":null}
                }))
            }),
        )
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({
                    "data":{"id":"ses_one","title":"Native gateway","location":{"directory":"/project"}}
                }))
            }),
        )
        .route(
            "/api/session/ses_one/message",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query.get("limit").map(String::as_str), Some("20"));
                assert_eq!(query.get("order").map(String::as_str), Some("desc"));
                Json(json!({
                    "data":[
                        {"id":"msg_new","type":"assistant","content":[],"time":{"created":2}},
                        {"id":"msg_old","type":"user","text":"before","time":{"created":1}}
                    ],
                    "cursor":{"previous":null,"next":"opaque-v2-cursor"}
                }))
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
async fn negotiates_a_versioned_gateway_and_keeps_native_history_pagination() -> anyhow::Result<()>
{
    let (upstream_origin, upstream_task) = serve_upstream().await?;
    let upstream = V2Client::new(&upstream_origin, "/project", None)?;

    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway = OpenCodeGateway::new(Connection::from(upstream)).serve(gateway_listener)?;
    let gateway_origin = format!("http://{gateway_addr}");

    let connection = Connection::connect(&gateway_origin, "/project", None).await?;
    assert!(connection.is_v2());
    let sessions = connection.sessions(None).await?;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_one");

    let page = connection.history("ses_one", 20, None).await?;
    assert_eq!(page.messages.len(), 2);
    assert_eq!(page.messages[0].id(), "msg_old");
    assert_eq!(page.messages[1].id(), "msg_new");
    assert_eq!(page.next_cursor.as_deref(), Some("opaque-v2-cursor"));

    gateway.stop().await;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_scopes_native_permissions_and_typed_forms_without_v1_conversion(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                Json(json!({"version":"2.0.18","pid":4242,"urls":[],"paths":{"tmp":"/tmp"}}))
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }))
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))
            }),
        )
        .route(
            "/api/session/ses_other",
            get(|| async {
                Json(json!({"data":{"id":"ses_other","location":{"directory":"/other"}}}))
            }),
        )
        .route(
            "/api/permission/request",
            get(|| async {
                Json(json!({
                    "location":{"directory":"/project"},
                    "data":[
                        {"id":"per_one","sessionID":"ses_one","action":"shell","resources":["echo ok"],"save":["echo *"]},
                        {"id":"per_other","sessionID":"ses_other","action":"shell","resources":["secret"]}
                    ]
                }))
            }),
        )
        .route(
            "/api/session/ses_one/permission/per_one/reply",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body, json!({"decision":"once","message":"approve"}));
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/api/form",
            get(|| async {
                Json(json!({
                    "location":{"directory":"/project"},
                    "data":[
                        {"id":"frm_one","sessionID":"ses_one","title":"Native form","fields":[{"key":"enabled","type":"boolean","required":true}]},
                        {"id":"frm_other","sessionID":"ses_other","title":"Foreign form","fields":[{"key":"text","type":"string"}]}
                    ]
                }))
            }),
        )
        .route(
            "/api/session/ses_one/form/frm_one/reply",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body, json!({"answer":{"enabled":false}}));
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/api/session/ses_one/form/frm_one",
            delete(|| async { StatusCode::NO_CONTENT }),
        );
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_origin = format!("http://{}", upstream_listener.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        let _ = axum::serve(upstream_listener, app).await;
    });
    let upstream = V2Client::new(&upstream_origin, "/project", None)?;
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway = OpenCodeGateway::new(Connection::from(upstream)).serve(gateway_listener)?;
    let connection =
        Connection::connect(&format!("http://{gateway_addr}"), "/project", None).await?;

    let permissions = connection.permissions().await?;
    assert_eq!(permissions.len(), 1);
    assert_eq!(permissions[0].id(), "per_one");
    connection
        .reply_permission(
            "per_one",
            pocket_codex_host_svc::opencode::PermissionReply::Once,
            Some("approve"),
        )
        .await?;
    assert!(
        connection
            .reply_permission(
                "per_other",
                pocket_codex_host_svc::opencode::PermissionReply::Once,
                None,
            )
            .await
            .is_err()
    );

    let forms = connection.questions().await?;
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].id(), "frm_one");
    connection
        .reply_form("frm_one", json!({"enabled":false}))
        .await?;
    assert!(connection
        .reply_form("frm_other", json!({"text":"foreign"}))
        .await
        .is_err());
    connection.reject_question("frm_one").await?;

    gateway.stop().await;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_rejects_v2_directory_and_session_scope_and_stop_leaves_upstream_alive(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                Json(json!({"version":"2.0.18","pid":4242,"urls":[],"paths":{"tmp":"/tmp"}}))
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }))
        .route(
            "/api/session",
            get(|| async { Json(json!({"data":[],"cursor":{"previous":null,"next":null}})) }),
        )
        .route(
            "/api/session/ses_other",
            get(|| async {
                Json(json!({"data":{"id":"ses_other","location":{"directory":"/other"}}}))
            }),
        );
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_origin = format!("http://{}", upstream_listener.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        let _ = axum::serve(upstream_listener, app).await;
    });
    let upstream = V2Client::new(&upstream_origin, "/project", None)?;
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway = OpenCodeGateway::new(Connection::from(upstream)).serve(gateway_listener)?;
    let http = reqwest::Client::new();
    let wrong_directory = http
        .get(format!("http://{gateway_addr}/pocket/opencode/v2/session?directory=/other"))
        .send()
        .await?;
    assert_eq!(wrong_directory.status(), reqwest::StatusCode::FORBIDDEN);
    let foreign_session = http
        .get(format!("http://{gateway_addr}/pocket/opencode/v2/session/ses_other"))
        .send()
        .await?;
    assert_eq!(foreign_session.status(), reqwest::StatusCode::FORBIDDEN);
    let unlisted = http
        .get(format!("http://{gateway_addr}/pocket/opencode/v2/config"))
        .send()
        .await?;
    assert_eq!(unlisted.status(), reqwest::StatusCode::NOT_FOUND);
    gateway.stop().await;
    let upstream_health = http
        .get(format!("{upstream_origin}/api/info"))
        .send()
        .await?;
    assert_eq!(upstream_health.status(), reqwest::StatusCode::OK);
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_admits_one_native_prompt_and_interrupts_only_the_selected_session(
) -> anyhow::Result<()> {
    let prompt_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let interrupt_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let prompt_seen = prompt_calls.clone();
    let interrupt_seen = interrupt_calls.clone();
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                Json(json!({"version":"2.0.18","pid":4242,"urls":[],"paths":{"tmp":"/tmp"}}))
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }))
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))
            }),
        )
        .route(
            "/api/session/ses_one/prompt",
            post(move |Json(body): Json<Value>| {
                let prompt_seen = prompt_seen.clone();
                async move {
                    prompt_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert_eq!(body, json!({"id":"msg_input","text":"hello from gateway"}));
                    Json(json!({"data":{"id":"msg_input","sessionID":"ses_one","type":"user","payload":{"text":"hello from gateway"},"delivery":"queue","time":{"created":1}}}))
                }
            }),
        )
        .route(
            "/api/session/ses_one/interrupt",
            post(move |Query(query): Query<HashMap<String, String>>| {
                let interrupt_seen = interrupt_seen.clone();
                async move {
                    interrupt_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert!(!query.contains_key("resume"));
                    Json(json!({"interrupted":true}))
                }
            }),
        );
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_origin = format!("http://{}", upstream_listener.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        let _ = axum::serve(upstream_listener, app).await;
    });
    let upstream = V2Client::new(&upstream_origin, "/project", None)?;
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway = OpenCodeGateway::new(Connection::from(upstream)).serve(gateway_listener)?;
    let connection =
        Connection::connect(&format!("http://{gateway_addr}"), "/project", None).await?;
    connection
        .prompt("ses_one", &PromptInput::text("hello from gateway", Some("msg_input".to_owned())))
        .await?;
    connection.abort("ses_one").await?;
    assert_eq!(prompt_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(interrupt_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    gateway.stop().await;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_forwards_native_sse_events_and_closes_after_upstream_disconnect(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                Json(json!({"version":"2.0.18","pid":4242,"urls":[],"paths":{"tmp":"/tmp"}}))
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }))
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))
            }),
        )
        .route(
            "/api/event",
            get(|| async {
                let first = stream::iter([
                    Ok::<_, std::io::Error>(Bytes::from(concat!(
                        ": heartbeat\r\n\r\n",
                        "data: {\"id\":\"evt_connected\",\"type\":\"server.connected\",",
                        "\"data\":{}}\r\n\r\n",
                    ))),
                    Ok::<_, std::io::Error>(Bytes::from(concat!(
                        "data: {\"id\":\"evt_text\",\"type\":\"session.text.delta\",\"location\":",
                        "{\"directory\":\"/project\"},\"data\":{\"sessionID\":\"ses_one\",",
                        "\"assistantMessageID\":\"msg_assistant\",\"ordinal\":0,",
                        "\"delta\":\"hello\"}}\r\n\r\n",
                    ))),
                ]);
                let body = axum::body::Body::from_stream(first);
                ([("content-type", "text/event-stream")], body)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_origin = format!("http://{}", listener.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let upstream = V2Client::new(&upstream_origin, "/project", None)?;
    let mut direct_events = upstream.events().await?;
    let direct_connected = direct_events
        .next()
        .await
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("missing direct connected event"))?;
    assert_eq!(direct_connected.kind, "server.connected");
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway = OpenCodeGateway::new(Connection::from(upstream)).serve(gateway_listener)?;
    let connection =
        Connection::connect(&format!("http://{gateway_addr}"), "/project", None).await?;
    let mut events = connection.events().await?;
    let connected = events
        .next()
        .await
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("missing connected event"))?;
    assert_eq!(connected.kind(), "server.connected");
    let text = events
        .next()
        .await
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("missing text event"))?;
    assert_eq!(text.kind(), "session.text.delta");
    assert_eq!(serde_json::to_value(&text)?["data"]["delta"], "hello");
    assert!(serde_json::to_value(&text)?.get("properties").is_none());
    assert!(events.next().await.is_none());
    gateway.stop().await;
    upstream_task.abort();
    Ok(())
}

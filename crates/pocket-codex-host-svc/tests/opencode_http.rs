//! Contract tests through the public OpenCode client and a real HTTP listener.

use std::collections::HashMap;

use axum::{extract::Query, http::HeaderMap, routing::get, Json, Router};
use pocket_codex_host_svc::opencode::{BasicCredentials, OpenCodeClient, OpenCodeGateway};
use serde_json::json;

#[tokio::test]
async fn lists_sessions_with_basic_auth_and_an_explicit_bounded_directory() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/session", get(
            |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(headers["authorization"], "Basic b3BlbmNvZGU6Zml4dHVyZS1zZWNyZXQ=");
                assert_eq!(query.get("directory").map(String::as_str), Some("/project space"));
                assert_eq!(query.get("limit").map(String::as_str), Some("100"));
                Json(json!([{"id":"ses_one","title":"Existing conversation","directory":"/project space"}]))
            }
        ))).await
    });
    let client = OpenCodeClient::new(
        &origin,
        "/project space",
        Some(BasicCredentials::new("opencode", "fixture-secret")),
    )?;
    let sessions = client.sessions(None).await?;
    server.abort();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_one");
    assert_eq!(sessions[0].title, "Existing conversation");
    Ok(())
}

#[test]
fn unsafe_connection_input_never_becomes_a_credential_destination() {
    for origin in [
        "http://opencode:canary@127.0.0.1:4096",
        "http://127.0.0.1:4096/?auth_token=canary",
        "http://127.0.0.1:4096/#canary",
        "http://127.0.0.1:4096/nested/path",
        "ftp://127.0.0.1:4096",
        "http://remote.example:4096",
    ] {
        let result = OpenCodeClient::new(
            origin,
            "/project",
            Some(BasicCredentials::new("opencode", "canary")),
        );
        assert!(result.is_err(), "unsafe destination accepted");
        assert!(!format!("{result:?}").contains("canary"));
    }
    assert!(OpenCodeClient::new(
        "https://remote.example",
        "/project",
        Some(BasicCredentials::new("opencode", "canary"))
    )
    .is_ok());
    assert!(OpenCodeClient::new("http://127.0.0.1:4096", "", None).is_err());
}

#[tokio::test]
async fn history_uses_only_the_opaque_cursor_and_keeps_native_message_parts() -> anyhow::Result<()>
{
    let app = Router::new()
        .route("/session/ses_one", get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }))
        .route("/session/ses_one/message", get(|Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(query["limit"], "20");
            assert_eq!(query["directory"], "/project");
            if query.contains_key("before") { assert_eq!(query["before"], "opaque+/="); }
            ([ ("x-next-cursor", "opaque+/="), ("link", "<https://untrusted.invalid/steal>; rel=\"next\"") ], Json(json!([
                {"info":{"id":"msg_one","sessionID":"ses_one","role":"assistant","custom":"preserved"},"parts":[{"id":"prt_one","messageID":"msg_one","sessionID":"ses_one","type":"text","text":"hello"}]}
            ])))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let page = client.history("ses_one", 20, None).await?;
    assert_eq!(page.next_cursor.as_deref(), Some("opaque+/="));
    assert_eq!(page.messages[0].info.id, "msg_one");
    assert_eq!(page.messages[0].parts[0].extra["text"], "hello");
    assert_eq!(page.messages[0].info.extra["custom"], "preserved");
    let next = client
        .history("ses_one", 20, page.next_cursor.as_deref())
        .await?;
    assert_eq!(next.messages[0].info.id, "msg_one");
    assert!(client.history("ses_one", 0, None).await.is_err());
    assert!(client.history("ses_one", 101, None).await.is_err());
    server.abort();
    Ok(())
}

#[tokio::test]
async fn directory_selection_filters_foreign_and_remote_workspace_sessions() -> anyhow::Result<()> {
    let app = Router::new().route("/session", get(|| async { Json(json!([
        {"id":"ses_local","title":"Local","directory":"/project"},
        {"id":"ses_other","title":"Other","directory":"/other"},
        {"id":"ses_remote","title":"Remote","directory":"/project","workspaceID":"wrk_remote"}
    ])) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let sessions = OpenCodeClient::new(&origin, "/project", None)?
        .sessions(None)
        .await?;
    server.abort();
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["ses_local"]
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn directory_scope_accepts_canonical_equivalent_host_paths() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let canonical = root.path().join("canonical");
    std::fs::create_dir(&canonical)?;
    let alias = root.path().join("alias");
    symlink(&canonical, &alias)?;
    let expected_directory = canonical.to_string_lossy().into_owned();
    let app = Router::new().route(
        "/session/ses_one",
        get(move || {
            let directory = expected_directory.clone();
            async move { Json(json!({"id":"ses_one","title":"One","directory":directory})) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, &alias.to_string_lossy(), None)?;
    assert_eq!(client.session("ses_one").await?.id, "ses_one");
    server.abort();
    Ok(())
}

#[tokio::test]
async fn healthy_but_incomplete_openapi_is_not_a_compatible_connection() -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/global/health",
            get(|| async { Json(json!({"healthy":true,"version":"1.18.32"})) }),
        )
        .route("/doc", get(|| async { Json(json!({"openapi":"3.1.0","paths":{}})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    assert!(client.health().await?.healthy);
    assert!(client.capabilities().await.is_err());
    server.abort();
    Ok(())
}

#[tokio::test]
async fn prompt_acceptance_keeps_the_official_async_contract() -> anyhow::Result<()> {
    use pocket_codex_host_svc::opencode::PromptInput;
    let app = Router::new()
        .route(
            "/session/ses_one",
            get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }),
        )
        .route(
            "/session/ses_one/prompt_async",
            axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                assert_eq!(
                    body,
                    json!({"messageID":"msg_one","parts":[{"type":"text","text":"Continue"}]})
                );
                axum::http::StatusCode::NO_CONTENT
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    client
        .prompt("ses_one", &PromptInput::text("Continue", Some("msg_one".into())))
        .await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn permission_reply_uses_current_pending_scope_and_official_reply_values(
) -> anyhow::Result<()> {
    use pocket_codex_host_svc::opencode::PermissionReply;
    let app = Router::new()
        .route("/session/ses_one", get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }))
        .route("/permission", get(|| async { Json(json!([{"id":"per_one","sessionID":"ses_one","permission":"bash","patterns":["git status"],"metadata":{},"always":["git *"]}])) }))
        .route("/permission/per_one/reply", axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
            assert_eq!(body, json!({"reply":"once"}));
            Json(true)
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    assert_eq!(client.permissions().await?[0].permission, "bash");
    client
        .reply_permission("per_one", PermissionReply::Once, None)
        .await?;
    assert!(client
        .reply_permission("per_stale", PermissionReply::Always, None)
        .await
        .is_err());
    server.abort();
    Ok(())
}

#[tokio::test]
async fn questions_accept_ordered_answers_and_explicit_rejection() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/session/ses_one", get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }))
        .route("/question", get(|| async { Json(json!([{"id":"que_one","sessionID":"ses_one","questions":[{"question":"Which?","header":"Choice","options":[{"label":"A","description":"First"}],"multiple":false}]}])) }))
        .route("/question/que_one/reply", axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
            assert_eq!(body, json!({"answers":[["A"]]})); Json(true)
        }))
        .route("/question/que_one/reject", axum::routing::post(|| async { Json(true) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    assert_eq!(client.questions().await?[0].questions[0].options[0].label, "A");
    client
        .reply_question("que_one", vec![vec!["A".into()]])
        .await?;
    assert!(client.reply_question("que_one", vec![]).await.is_err());
    client.reject_question("que_one").await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn abort_targets_the_session_and_leaves_the_server_healthy() -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/session/ses_one",
            get(|| async { Json(json!({"id":"ses_one","title":"One","directory":"/project"})) }),
        )
        .route("/session/status", get(|| async { Json(json!({"ses_one":{"type":"busy"}})) }))
        .route("/session/ses_one/abort", axum::routing::post(|| async { Json(true) }))
        .route(
            "/global/health",
            get(|| async { Json(json!({"healthy":true,"version":"1.18.32"})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    assert_eq!(client.status().await?["ses_one"]["type"], "busy");
    client.abort("ses_one").await?;
    assert!(client.health().await?.healthy);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn creates_a_scoped_session_and_reads_one_message_for_reconciliation() -> anyhow::Result<()> {
    let message = json!({"info":{"id":"msg_one","sessionID":"ses_one","role":"user"},"parts":[]});
    let app = Router::new()
        .route(
            "/session",
            axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                assert_eq!(body, json!({"title":"New"}));
                Json(json!({"id":"ses_one","title":"New","directory":"/project"}))
            }),
        )
        .route(
            "/session/ses_one",
            get(|| async { Json(json!({"id":"ses_one","title":"New","directory":"/project"})) }),
        )
        .route(
            "/session/ses_one/message/msg_one",
            get(move || {
                let message = message.clone();
                async move { Json(message) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let created = client.create(Some("New")).await?;
    assert_eq!(created.id, "ses_one");
    assert_eq!(client.message(&created.id, "msg_one").await?.info.role, "user");
    server.abort();
    Ok(())
}

#[tokio::test]
async fn gateway_is_loopback_only_and_stopping_it_does_not_touch_upstream() -> anyhow::Result<()> {
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_origin = format!("http://{}", upstream_listener.local_addr()?);
    let upstream = tokio::spawn(async move {
        axum::serve(
            upstream_listener,
            Router::new().route(
                "/global/health",
                get(|| async { Json(json!({"healthy":true,"version":"1.18.32"})) }),
            ),
        )
        .await
    });

    let client = OpenCodeClient::new(&upstream_origin, "/project", None)?;
    let gateway = OpenCodeGateway::new(client.clone());
    assert!(gateway.clone().bind("0.0.0.0:0".parse()?).await.is_err());
    let (listener, router) = gateway.bind("127.0.0.1:0".parse()?).await?;
    let gateway_addr = listener.local_addr()?;
    let task = tokio::spawn(async move { axum::serve(listener, router).await });

    let health: serde_json::Value = reqwest::get(format!("http://{gateway_addr}/global/health"))
        .await?
        .json()
        .await?;
    assert_eq!(health["version"], "1.18.32");
    task.abort();
    assert!(client.health().await?.healthy, "gateway teardown must not stop upstream OpenCode");
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn sse_preserves_split_utf8_multiline_data_and_scoped_unknown_events() -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tokio_stream::StreamExt;
    let session_reads = Arc::new(AtomicUsize::new(0));
    let frames = concat!(
        "\u{feff}: comment\r\n",
        "data: {\"id\":\"evt_one\",\"type\":\"server.connected\",\r\n",
        "data: \"properties\":{}}\r\n\r\n",
        "data: {\"id\":\"evt_two\",\"type\":\"future.event\",\"properties\":{\"sessionID\":\"\
         ses_one\",\"text\":\"\u{4e2d}\u{6587}\"}}\n\n",
    )
    .to_owned()
        + &(0..32)
            .map(|index| {
                format!(
                    "data: {{\"id\":\"evt_burst_{index}\",\"type\":\"message.part.delta\",\"\
                     properties\":{{\"sessionID\":\"ses_one\",\"text\":\"{index}\"}}}}\n\n"
                )
            })
            .collect::<String>();
    let session_reads_for_route = session_reads.clone();
    let app = Router::new()
        .route(
            "/session/ses_one",
            get(move || {
                let session_reads = session_reads_for_route.clone();
                async move {
                    session_reads.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"id":"ses_one","title":"One","directory":"/project"}))
                }
            }),
        )
        .route(
            "/event",
            get(move || async move {
                let chunks = frames
                    .as_bytes()
                    .iter()
                    .map(|byte| Ok::<_, std::io::Error>(vec![*byte]))
                    .collect::<Vec<_>>();
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from_stream(tokio_stream::iter(chunks)),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = OpenCodeClient::new(&origin, "/project", None)?;
    let mut events = client.events().await?;
    let first = events
        .next()
        .await
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("event stream ended before first event"))?;
    assert_eq!(first.kind, "server.connected");
    let unknown = events
        .next()
        .await
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("event stream ended before unknown event"))?;
    assert_eq!(unknown.kind, "future.event");
    assert_eq!(unknown.properties["text"], "\u{4e2d}\u{6587}");
    for index in 0..32 {
        let event = events
            .next()
            .await
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("event stream ended during burst at {index}"))?;
        assert_eq!(event.kind, "message.part.delta");
    }
    assert_eq!(session_reads.load(Ordering::SeqCst), 1);
    assert!(
        matches!(events.next().await, Some(Err(_))),
        "EOF must require authoritative resynchronization"
    );
    assert!(events.next().await.is_none());
    server.abort();
    Ok(())
}

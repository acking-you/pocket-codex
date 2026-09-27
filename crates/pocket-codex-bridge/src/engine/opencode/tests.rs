use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::Result;
use axum::{
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures::StreamExt;
use pocket_codex_host_svc::opencode::{
    connection::{EventStream, NativeEvent},
    OpenCodeClient, OpenCodeEvent,
};
use serde_json::json;
use tokio::sync::Notify;

#[tokio::test]
async fn event_burst_is_drained_before_one_snapshot_refresh() -> Result<()> {
    let consumed = Arc::new(AtomicUsize::new(0));
    let mut stream: EventStream = Box::pin(
        futures::stream::iter((0..32).map({
            let consumed = consumed.clone();
            move |index| {
                consumed.fetch_add(1, Ordering::SeqCst);
                Ok(NativeEvent::V1(OpenCodeEvent {
                    id: Some(format!("evt_{index}")),
                    kind: "message.part.delta".into(),
                    properties: serde_json::json!({"sessionID":"ses_one"}),
                }))
            }
        }))
        .chain(futures::stream::pending()),
    );

    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            super::drain_event_burst(&mut stream),
        )
        .await??
        .map(|events| events.len()),
        Some(32)
    );
    assert_eq!(consumed.load(Ordering::SeqCst), 32);
    Ok(())
}

use super::{disconnect, register, track_event_task, OpenCodeController};

#[tokio::test]
async fn disconnect_aborts_registered_event_tasks() -> Result<()> {
    let id = register(OpenCodeClient::new("http://127.0.0.1:4096", "/workspace", None)?, None);
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _dropped_tx = dropped_tx;
        std::future::pending::<()>().await;
    });
    assert!(track_event_task(&id, task));

    disconnect(&id);
    assert!(dropped_rx.await.is_err());
    assert!(super::connections()
        .lock()
        .expect("connection registry")
        .get(&id)
        .is_none());
    Ok(())
}

#[tokio::test]
async fn late_history_cannot_replace_the_newly_selected_session() -> Result<()> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = Router::new()
        .route("/permission", get(|| async { Json(json!([])) }))
        .route("/question", get(|| async { Json(json!([])) }))
        .route("/session/status", get(|| async { Json(json!({})) }))
        .route("/session/{id}", get(|Path(id): Path<String>| async move {
            Json(json!({"id":id,"title":"Fixture","directory":"/workspace"}))
        }))
        .route("/session/{id}/message", get({
            let entered = entered.clone();
            let release = release.clone();
            move |Path(id): Path<String>| {
                let entered = entered.clone();
                let release = release.clone();
                async move {
                    if id == "ses_a" {
                        entered.notify_one();
                        release.notified().await;
                    }
                    Json(json!([{"info":{"id":"msg_one","sessionID":id,"role":"user"},"parts":[]}]))
                }
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller =
        Arc::new(OpenCodeController::new(OpenCodeClient::new(&origin, "/workspace", None)?));
    let old = tokio::spawn({
        let controller = controller.clone();
        async move { controller.open_session("ses_a").await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await?;
    let selected = controller.open_session("ses_b").await?;
    release.notify_one();
    assert!(old.await?.is_err());
    assert_eq!(selected.session_id, "ses_b");
    assert_eq!(controller.snapshot().await?.session_id, "ses_b");
    server.abort();
    Ok(())
}

#[tokio::test]
async fn failed_older_page_preserves_history_and_can_be_explicitly_retried() -> Result<()> {
    let reads = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/permission", get(|| async { Json(json!([])) }))
        .route("/question", get(|| async { Json(json!([])) }))
        .route("/session/status", get(|| async { Json(json!({})) }))
        .route("/session/{id}", get(|Path(id): Path<String>| async move {
            Json(json!({"id":id,"title":"Fixture","directory":"/workspace"}))
        }))
        .route("/session/{id}/message", get({
            let reads = reads.clone();
            move |Query(query): Query<HashMap<String, String>>| {
                let reads = reads.clone();
                async move {
                    let call = reads.fetch_add(1, Ordering::SeqCst);
                    if query.contains_key("before") && call == 1 {
                        return StatusCode::BAD_GATEWAY.into_response();
                    }
                    let ids = if query.contains_key("before") { vec!["msg_one"] } else { vec!["msg_two", "msg_three"] };
                    let mut headers = HeaderMap::new();
                    if !query.contains_key("before") { headers.insert("x-next-cursor", "cursor-one".parse().expect("fixture header")); }
                    let messages: Vec<_> = ids.into_iter().map(|id| json!({"info":{"id":id,"sessionID":"ses_one","role":"user"},"parts":[]})).collect();
                    (headers, Json(messages)).into_response()
                }
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = OpenCodeController::new(OpenCodeClient::new(&origin, "/workspace", None)?);
    controller.open_session("ses_one").await?;
    assert!(controller.older("ses_one").await.is_err());
    let retained = controller.snapshot().await?;
    assert_eq!(retained.messages.len(), 2);
    assert_eq!(retained.next_cursor.as_deref(), Some("cursor-one"));
    let loaded = controller.older("ses_one").await?;
    assert_eq!(
        loaded
            .messages
            .iter()
            .map(|message| message.id())
            .collect::<Vec<_>>(),
        vec!["msg_one", "msg_two", "msg_three"]
    );
    assert!(loaded.next_cursor.is_none());
    controller.older("ses_one").await?;
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn execution_status_comes_from_the_server_not_the_last_message() -> Result<()> {
    let app = Router::new()
        .route("/permission", get(|| async { Json(json!([])) }))
        .route("/question", get(|| async { Json(json!([])) }))
        .route("/session/status", get(|| async { Json(json!({"ses_one":{"type":"busy"}})) }))
        .route("/session/{id}", get(|Path(id): Path<String>| async move {
            Json(json!({"id":id,"title":"Fixture","directory":"/workspace"}))
        }))
        .route("/session/{id}/message", get(|| async {
            Json(json!([{"info":{"id":"msg_old","sessionID":"ses_one","role":"assistant","time":{"completed":123}},"parts":[]}]))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = OpenCodeController::new(OpenCodeClient::new(&origin, "/workspace", None)?);
    assert_eq!(controller.open_session("ses_one").await?.status, "busy");
    server.abort();
    Ok(())
}

#[tokio::test]
async fn pending_interactions_belong_only_to_the_selected_session() -> Result<()> {
    let app = Router::new()
        .route("/session/status", get(|| async { Json(json!({})) }))
        .route("/session/{id}", get(|Path(id): Path<String>| async move {
            Json(json!({"id":id,"title":"Fixture","directory":"/workspace"}))
        }))
        .route("/session/{id}/message", get(|| async { Json(json!([])) }))
        .route("/permission", get(|| async {
            Json(json!([
                {"id":"per_one","sessionID":"ses_one","permission":"bash","patterns":["cargo test"],"metadata":{},"always":["cargo *"]},
                {"id":"per_other","sessionID":"ses_other","permission":"bash","patterns":["other"],"metadata":{},"always":[]}
            ]))
        }))
        .route("/question", get(|| async {
            Json(json!([
                {"id":"que_one","sessionID":"ses_one","questions":[{"question":"Which?","header":"Choice","options":[{"label":"A","description":"First"}]}]},
                {"id":"que_other","sessionID":"ses_other","questions":[]}
            ]))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = OpenCodeController::new(OpenCodeClient::new(&origin, "/workspace", None)?);
    let snapshot = controller.open_session("ses_one").await?;
    assert_eq!(
        snapshot
            .permissions
            .iter()
            .map(|request| request.id())
            .collect::<Vec<_>>(),
        vec!["per_one"]
    );
    assert_eq!(
        snapshot
            .questions
            .iter()
            .map(|request| request.id())
            .collect::<Vec<_>>(),
        vec!["que_one"]
    );
    server.abort();
    Ok(())
}

#[tokio::test]
async fn send_and_abort_only_target_the_selected_session() -> Result<()> {
    use std::sync::Arc;

    use axum::routing::post;
    use tokio::sync::Mutex;

    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let aborts = Arc::new(Mutex::new(Vec::<String>::new()));
    let app = Router::new()
        .route("/permission", get(|| async { Json(json!([])) }))
        .route("/question", get(|| async { Json(json!([])) }))
        .route("/session/status", get(|| async { Json(json!({})) }))
        .route(
            "/session/{id}",
            get(|Path(id): Path<String>| async move {
                Json(json!({"id":id,"title":"Fixture","directory":"/workspace"}))
            }),
        )
        .route("/session/{id}/message", get(|| async { Json(json!([])) }))
        .route(
            "/session/{id}/prompt_async",
            post({
                let prompts = prompts.clone();
                move |Path(id): Path<String>, Json(body): Json<serde_json::Value>| {
                    let prompts = prompts.clone();
                    async move {
                        let text = body["parts"][0]["text"].as_str().unwrap_or_default();
                        prompts.lock().await.push(format!("{id}:{text}"));
                        StatusCode::NO_CONTENT
                    }
                }
            }),
        )
        .route(
            "/session/{id}/abort",
            post({
                let aborts = aborts.clone();
                move |Path(id): Path<String>| {
                    let aborts = aborts.clone();
                    async move {
                        aborts.lock().await.push(id);
                        Json(true)
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = OpenCodeController::new(OpenCodeClient::new(&origin, "/workspace", None)?);
    controller.open_session("ses_one").await?;
    controller.send("ses_one", "hello").await?;
    controller.abort("ses_one").await?;
    assert_eq!(prompts.lock().await.as_slice(), ["ses_one:hello"]);
    assert_eq!(aborts.lock().await.as_slice(), ["ses_one"]);
    server.abort();
    Ok(())
}

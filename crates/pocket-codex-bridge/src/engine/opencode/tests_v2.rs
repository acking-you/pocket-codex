use std::collections::HashMap;

use axum::{extract::Query, routing::get, Json, Router};
use pocket_codex_host_svc::opencode::{
    connection::{NativeEvent, NativeMessage},
    v2,
};
use serde_json::{json, Value};

fn text_event(kind: &str, id: &str, value: &str) -> NativeEvent {
    let mut data = json!({"sessionID":"ses_one","assistantMessageID":"msg_assistant","ordinal":0});
    if kind.ends_with("delta") {
        data["delta"] = json!(value);
    }
    if kind.ends_with("ended") {
        data["text"] = json!(value);
    }
    NativeEvent::V2(v2::Event {
        id: Some(id.into()),
        kind: kind.into(),
        data,
        location: Some(v2::Location {
            directory: "/fixture".into(),
            extra: Default::default(),
        }),
        extra: Default::default(),
    })
}

#[tokio::test]
async fn opening_a_nondefault_location_reads_history_and_pending_interactions() -> anyhow::Result<()>
{
    const DIRECTORY: &str = "/projects/work tree";
    async fn pending(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
        // OpenCode's LocationMiddleware falls back to its cwd for unknown query keys.
        let directory = query
            .get("location[directory]")
            .map(String::as_str)
            .unwrap_or("/server-default");
        Json(json!({"location":{"directory":directory},"data":[]}))
    }
    let app = Router::new()
        .route("/api/session/ses_one", get(|| async {
            Json(json!({"data":{"id":"ses_one","location":{"directory":DIRECTORY}}}))
        }))
        .route("/api/session/ses_one/message", get(|| async {
            Json(json!({"data":[{"id":"msg_user","type":"user","text":"History survives location selection"}],"cursor":{"next":null}}))
        }))
        .route("/api/session/active", get(|| async { Json(json!({"data":{}})) }))
        .route("/api/permission/request", get(pending))
        .route("/api/form", get(pending));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = super::OpenCodeController::new(v2::V2Client::new(&origin, DIRECTORY, None)?);
    let result = controller.open_session("ses_one").await;
    server.abort();
    let snapshot = result?;
    assert_eq!(snapshot.session_id, "ses_one");
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].id(), "msg_user");
    assert_eq!(snapshot.status, "idle");
    assert!(snapshot.permissions.is_empty());
    assert!(snapshot.questions.is_empty());
    Ok(())
}

#[tokio::test]
async fn v2_live_text_is_not_duplicated_and_refresh_preserves_loaded_older_history(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/session/ses_one", get(|| async { Json(json!({"data":{"id":"ses_one","title":"Fixture","location":{"directory":"/fixture"}}})) }))
        .route("/api/session/ses_one/message", get(|Query(query): Query<HashMap<String,String>>| async move {
            if query.contains_key("cursor") { return Json(json!({"data":[{"id":"msg_old","type":"user","text":"older"}],"cursor":{"next":null}})); }
            Json(json!({"data":[{"id":"msg_assistant","type":"assistant","content":[{"type":"text","text":""}]},{"id":"msg_user","type":"user","text":"hello"}],"cursor":{"next":"older"}}))
        }))
        .route("/api/session/active", get(|| async { Json(json!({"data":{"ses_one":{"type":"running"}}})) }))
        .route("/api/permission/request", get(|| async { Json(json!({"location":{"directory":"/fixture"},"data":[]})) }))
        .route("/api/form", get(|| async { Json(json!({"location":{"directory":"/fixture"},"data":[]})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let controller = super::OpenCodeController::new(v2::V2Client::new(&origin, "/fixture", None)?);
    controller.open_session("ses_one").await?;
    controller.older("ses_one").await?;
    let started = text_event("session.text.started", "evt_start", "");
    let delta = text_event("session.text.delta", "evt_delta", "你好");
    controller
        .refresh("ses_one", vec![started, delta.clone(), delta])
        .await?;
    let snapshot = controller.snapshot().await?;
    assert_eq!(
        snapshot
            .messages
            .iter()
            .map(NativeMessage::id)
            .collect::<Vec<_>>(),
        ["msg_old", "msg_user", "msg_assistant"]
    );
    assert!(snapshot.next_cursor.is_none());
    let NativeMessage::V2(message) = &snapshot.messages[2] else {
        panic!("native message expected")
    };
    assert_eq!(message.extra["content"][0]["text"], "你好");
    let snapshot = controller
        .refresh("ses_one", vec![text_event("session.text.ended", "evt_end", "你好，完成")])
        .await?;
    let NativeMessage::V2(message) = &snapshot.messages[2] else {
        panic!("native message expected")
    };
    assert_eq!(message.extra["content"][0]["text"], "你好，完成");
    assert!(snapshot.next_cursor.is_none());
    server.abort();
    Ok(())
}

#[tokio::test]
async fn form_answers_cannot_target_a_request_outside_the_selected_snapshot() -> anyhow::Result<()>
{
    let controller =
        super::OpenCodeController::new(v2::V2Client::new("http://127.0.0.1:1", "/fixture", None)?);
    assert!(controller
        .reply_form("form_foreign", Value::Object(Default::default()))
        .await
        .is_err());
    Ok(())
}

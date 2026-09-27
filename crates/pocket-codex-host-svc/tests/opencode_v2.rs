//! Native OpenCode 2.0.18 contracts exercised through an actual HTTP listener.

use std::collections::HashMap;

use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use futures::StreamExt;
use pocket_codex_host_svc::opencode::{v2::V2Client, BasicCredentials, PermissionReply};
use serde_json::{json, Value};

fn contract() -> Value {
    serde_json::from_str(include_str!("fixtures/opencode_v2_contract.json"))
        .expect("checked-in OpenCode v2 contract fixture is valid JSON")
}

async fn server(
    app: Router,
) -> anyhow::Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    Ok((origin, tokio::spawn(async move { axum::serve(listener, app).await })))
}

#[tokio::test]
async fn connects_using_v2_identity_without_probing_v1_or_following_urls() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/info", get(|headers: HeaderMap| async move {
            assert_eq!(headers["authorization"], "Basic b3BlbmNvZGU6Zml4dHVyZS1zZWNyZXQ=");
            Json(json!({"version":"2.0.18","pid":123,"urls":["https://untrusted.invalid"],"paths":{"tmp":"/tmp"}}))
        }))
        .route("/openapi.json", get(|| async { Json(contract()) }));
    let (origin, task) = server(app).await?;
    let client = V2Client::new(
        &origin,
        "/project",
        Some(BasicCredentials::new("opencode", "fixture-secret")),
    )?;
    let info = client.connect().await?;
    assert_eq!(info.version, "2.0.18");
    assert_eq!(info.pid, 123);
    assert_eq!(client.directory(), "/project");
    assert!(!format!("{client:?}").contains("fixture-secret"));
    task.abort();
    Ok(())
}

#[tokio::test]
async fn browses_native_scoped_history_in_chronological_order_with_opaque_cursors(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/session", get(|Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(query["directory"], "/project");
            assert_eq!(query["order"], "desc");
            assert_eq!(query["limit"], "100");
            Json(json!({"data":[{"id":"ses_one","location":{"directory":"/project"}}, {"id":"ses_other","location":{"directory":"/other"}}],"cursor":{"previous":null,"next":null}}))
        }))
        .route("/api/session/ses_one", get(|| async {Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"},"title":"Native title"}}))}))
        .route("/api/session/ses_other", get(|| async {Json(json!({"data":{"id":"ses_other","location":{"directory":"/other"}}}))}))
        .route("/api/session/ses_one/message", get(|Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(query["limit"], "20");
            if query.contains_key("cursor") {
                assert_eq!(query["cursor"], "https://untrusted.invalid/?cursor=opaque");
                assert!(!query.contains_key("order"));
            } else {assert_eq!(query["order"], "desc");}
            Json(json!({"data":[
                {"id":"msg_new","type":"assistant","content":[{"type":"text","text":"Native answer"}],"time":{"created":2}},
                {"id":"msg_old","type":"future-visible","opaque":{"keep":true},"time":{"created":1}}
            ],"cursor":{"previous":null,"next":"https://untrusted.invalid/?cursor=opaque"}}))
        }))
        .route("/api/session/ses_one/message/msg_new", get(|| async {Json(json!({"data":{"id":"msg_new","type":"assistant","content":[]}}))}))
        .route("/api/session/active", get(|| async {Json(json!({"data":{"ses_one":{"type":"running"},"ses_other":{"type":"running"}}}))}));
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    let sessions = client.sessions(None).await?;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_one");
    assert!(sessions[0].title.is_none());
    let page = client.history("ses_one", 20, None).await?;
    assert_eq!(page.messages[0].id, "msg_old");
    assert_eq!(page.messages[0].kind, "future-visible");
    assert_eq!(page.messages[0].extra["opaque"]["keep"], true);
    assert_eq!(page.messages[1].extra["content"][0]["text"], "Native answer");
    client
        .history("ses_one", 20, page.next_cursor.as_deref())
        .await?;
    assert_eq!(client.message("ses_one", "msg_new").await?.id, "msg_new");
    assert_eq!(client.active().await?.len(), 1);
    assert!(client.session("ses_other").await.is_err());
    assert!(client.history("ses_one", 201, None).await.is_err());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn requires_native_schema_not_just_matching_route_names() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/info", get(|| async { Json(json!({"version":"2.0.18","pid":123,"urls":[],"paths":{"tmp":"/tmp"}})) }))
        .route("/openapi.json", get(|| async {
            let mut doc = contract();
            doc["paths"]["/api/session/{sessionID}/prompt"]["post"]["requestBody"]["content"]["application/json"]["schema"] = json!({"type":"object","properties":{"parts":{"type":"array"}},"required":["parts"]});
            Json(doc)
        }));
    let (origin, task) = server(app).await?;
    assert!(V2Client::new(&origin, "/project", None)?
        .connect()
        .await
        .is_err());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn creates_and_admits_native_prompts_then_interrupts_only_the_selected_execution(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/session", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"title":"Test only","location":{"directory":"/project"}}));
            Json(json!({"data":{"id":"ses_one","title":"Test only","location":{"directory":"/project"}}}))
        }))
        .route("/api/session/ses_one", get(|| async {Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))}))
        .route("/api/session/ses_one/prompt", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"id":"msg_input","text":"Hello native protocol"}));
            Json(json!({"data":{"id":"msg_input","sessionID":"ses_one","type":"user","payload":{"text":"Hello native protocol"},"delivery":"queue","time":{"created":1}}}))
        }))
        .route("/api/session/ses_one/interrupt", post(|Query(query): Query<HashMap<String,String>>| async move {
            assert!(!query.contains_key("resume"));
            Json(json!({"interrupted":false}))
        }));
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    let session = client.create(Some("Test only")).await?;
    let accepted = client
        .prompt(&session.id, "Hello native protocol", Some("msg_input"))
        .await?;
    assert_eq!(accepted.id, "msg_input");
    assert_eq!(accepted.delivery, "queue");
    assert!(!client.abort(&session.id).await?);
    task.abort();
    Ok(())
}

#[tokio::test]
async fn answers_only_current_scoped_permissions_and_typed_forms() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/session/ses_one", get(|| async {Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))}))
        .route("/api/session/ses_other", get(|| async {Json(json!({"data":{"id":"ses_other","location":{"directory":"/other"}}}))}))
        .route("/api/permission/request", get(|Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(query["location[directory]"], "/project");
            Json(json!({"location":{"directory":"/project"},"data":[
                {"id":"per_one","sessionID":"ses_one","action":"shell","resources":["echo ok"],"save":["echo *"]},
                {"id":"per_other","sessionID":"ses_other","action":"shell","resources":["other"]}
            ]}))
        }))
        .route("/api/session/ses_one/permission/per_one/reply", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"decision":"once","message":"Explicit approval"}));
            StatusCode::NO_CONTENT
        }))
        .route("/api/form", get(|Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(query["location[directory]"], "/project");
            Json(json!({"location":{"directory":"/project"},"data":[
            {"id":"frm_one","sessionID":"ses_one","title":"Typed question","fields":[{"key":"enabled","type":"boolean","required":true},{"key":"count","type":"integer","required":true,"minimum":1,"maximum":5}]},
            {"id":"frm_other","sessionID":"ses_other","title":"Other","fields":[{"key":"text","type":"string"}]}
        ]}))}))
        .route("/api/session/ses_one/form/frm_one/reply", post(|Json(body): Json<Value>| async move {
            assert_eq!(body, json!({"answer":{"enabled":false,"count":3}}));
            StatusCode::NO_CONTENT
        }))
        .route("/api/session/ses_one/form/frm_one", delete(|| async {StatusCode::NO_CONTENT}));
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    let permissions = client.permissions().await?;
    assert_eq!(permissions.len(), 1);
    assert_eq!(permissions[0].action, "shell");
    assert_eq!(permissions[0].save.as_deref(), Some(["echo *".to_owned()].as_slice()));
    client
        .reply_permission("per_one", PermissionReply::Once, Some("Explicit approval"))
        .await?;
    assert!(client
        .reply_permission("per_stale", PermissionReply::Once, None)
        .await
        .is_err());
    assert_eq!(client.forms().await?.len(), 1);
    assert!(client
        .reply_form("frm_one", json!({"enabled":"false","count":3}))
        .await
        .is_err());
    assert!(client
        .reply_form("frm_one", json!({"enabled":false}))
        .await
        .is_err());
    assert!(client
        .reply_form("frm_other", json!({"text":"wrong scope"}))
        .await
        .is_err());
    client
        .reply_form("frm_one", json!({"enabled":false,"count":3}))
        .await?;
    client.reject_form("frm_one").await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn streams_native_data_without_cross_location_or_unknown_events_and_invalidates_moves(
) -> anyhow::Result<()> {
    let frames = [
        json!({"id":"evt_connected","type":"server.connected","data":{}}),
        json!({"id":"evt_foreign","type":"session.text.delta","location":{"directory":"/other"},"data":{"sessionID":"ses_other","delta":"private"}}),
        json!({"id":"evt_unknown","type":"future.secret","location":{"directory":"/project"},"data":{"sessionID":"ses_one","secret":"private"}}),
        json!({"id":"evt_text","type":"session.text.delta","location":{"directory":"/project"},"data":{"sessionID":"ses_one","assistantMessageID":"msg_assistant","ordinal":0,"delta":"Hello"}}),
        json!({"id":"evt_move","type":"session.moved","location":{"directory":"/project"},"data":{"sessionID":"ses_one","location":{"directory":"/other"}}}),
        json!({"id":"evt_stale","type":"session.text.delta","location":{"directory":"/project"},"data":{"sessionID":"ses_one","delta":"private after move"}}),
    ].iter().map(|event| format!("data: {event}\r\n\r\n: heartbeat\r\n\r\n")).collect::<String>();
    let app = Router::new()
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))
            }),
        )
        .route(
            "/api/event",
            get(move |headers: HeaderMap| {
                let frames = frames.clone();
                async move {
                    assert!(!headers.contains_key("last-event-id"));
                    ([("content-type", "text/event-stream")], format!("\u{feff}{frames}"))
                }
            }),
        );
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    let mut stream = client.events().await?;
    assert_eq!(
        stream
            .next()
            .await
            .transpose()?
            .expect("fixture emits server.connected")
            .kind,
        "server.connected"
    );
    let event = stream
        .next()
        .await
        .transpose()?
        .expect("fixture emits a scoped session event");
    assert_eq!(event.data["delta"], "Hello");
    assert!(serde_json::to_value(&event)?.get("properties").is_none());
    assert!(matches!(
        stream.next().await,
        Some(Err(pocket_codex_host_svc::opencode::Error::Disconnected))
    ));
    assert!(stream.next().await.is_none());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn conditional_forms_require_only_active_fields_and_never_answer_hidden_branches(
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/session/ses_one", get(|| async {Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))}))
        .route("/api/form", get(|| async {Json(json!({"location":{"directory":"/project"},"data":[
            {"id":"frm_conditional","sessionID":"ses_one","title":"Conditional","fields":[
                {"key":"enabled","type":"boolean","required":true},
                {"key":"detail","type":"string","required":true,"when":[{"key":"enabled","op":"eq","value":true}]}
            ]}
        ]}))}))
        .route("/api/session/ses_one/form/frm_conditional/reply", post(|Json(body): Json<Value>| async move {
            assert!(body == json!({"answer":{"enabled":false}}) || body == json!({"answer":{"enabled":true,"detail":"Explicit answer"}}));
            StatusCode::NO_CONTENT
        }));
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    client
        .reply_form("frm_conditional", json!({"enabled":false}))
        .await?;
    client
        .reply_form("frm_conditional", json!({"enabled":true,"detail":"Explicit answer"}))
        .await?;
    assert!(client
        .reply_form("frm_conditional", json!({"enabled":true}))
        .await
        .is_err());
    assert!(client
        .reply_form("frm_conditional", json!({"enabled":false,"detail":"Inactive leak"}))
        .await
        .is_err());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn rejects_missing_native_response_and_approval_schemas() -> anyhow::Result<()> {
    for pointer in [
        "/components/schemas/ServerInfo/properties/pid",
        "/components/schemas/SessionsResponse/properties/data",
        "/components/schemas/Session.Info/properties/location",
        "/components/schemas/SessionMessagesResponse/properties/cursor",
        "/components/schemas/Session.Message.Assistant/properties/content",
        "/components/schemas/Permission.Reply",
        "/components/schemas/Form.Reply/properties/answer",
        "/components/schemas/V2EventEncoded",
    ] {
        let mut doc = contract();
        *doc.pointer_mut(pointer)
            .expect("fixture schema pointer exists") = json!({"type":"null"});
        let app = Router::new()
            .route(
                "/api/info",
                get(|| async {
                    Json(json!({"version":"2.0.18","pid":123,"urls":[],"paths":{"tmp":"/tmp"}}))
                }),
            )
            .route(
                "/openapi.json",
                get(move || {
                    let doc = doc.clone();
                    async move { Json(doc) }
                }),
            );
        let (origin, task) = server(app).await?;
        assert!(
            V2Client::new(&origin, "/project", None)?
                .connect()
                .await
                .is_err(),
            "unsupported schema: {pointer}"
        );
        task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn does_not_mistake_non_ready_success_status_for_a_ready_server() -> anyhow::Result<()> {
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async {
                (
                    StatusCode::ACCEPTED,
                    Json(json!({"version":"2.0.18","pid":123,"urls":[],"paths":{"tmp":"/tmp"}})),
                )
            }),
        )
        .route("/openapi.json", get(|| async { Json(contract()) }));
    let (origin, task) = server(app).await?;
    assert!(V2Client::new(&origin, "/project", None)?
        .connect()
        .await
        .is_err());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn preserves_ambiguous_prompt_admission_without_automatically_repeating_it(
) -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = Router::new()
        .route(
            "/api/session/ses_one",
            get(|| async {
                Json(json!({"data":{"id":"ses_one","location":{"directory":"/project"}}}))
            }),
        )
        .route(
            "/api/session/ses_one/prompt",
            post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    ([("content-type", "application/json")], "{truncated")
                }
            }),
        );
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, "/project", None)?;
    assert!(matches!(
        client
            .prompt("ses_one", "One admission only", Some("msg_one"))
            .await,
        Err(pocket_codex_host_svc::opencode::Error::SubmissionUnknown)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
    Ok(())
}

#[tokio::test]
async fn rejects_non_json_and_redirects_without_forwarding_credentials() -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let app = Router::new()
        .route(
            "/api/info",
            get(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/credential-sink")]) }),
        )
        .route(
            "/credential-sink",
            get(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        );
    let (origin, task) = server(app).await?;
    let client = V2Client::new(
        &origin,
        "/project",
        Some(BasicCredentials::new("opencode", "never-log-this")),
    )?;
    let failure = client
        .connect()
        .await
        .expect_err("invalid contract must be rejected");
    assert_eq!(failure, pocket_codex_host_svc::opencode::Error::Rejected(307));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(!format!("{failure:?} {client:?}").contains("never-log-this"));
    task.abort();
    let (origin, task) =
        server(Router::new().route("/api/info", get(|| async { "<html>Not an API</html>" })))
            .await?;
    assert!(matches!(
        V2Client::new(&origin, "/project", None)?.connect().await,
        Err(pocket_codex_host_svc::opencode::Error::Protocol)
    ));
    task.abort();
    Ok(())
}

#[tokio::test]
async fn bounds_unterminated_event_frames_before_parsing_them() -> anyhow::Result<()> {
    let app = Router::new().route(
        "/api/event",
        get(|| async {
            (
                [("content-type", "text/event-stream")],
                format!("data: {}", "x".repeat(8 * 1024 * 1024)),
            )
        }),
    );
    let (origin, task) = server(app).await?;
    let mut stream = V2Client::new(&origin, "/project", None)?.events().await?;
    assert!(matches!(
        stream.next().await,
        Some(Err(pocket_codex_host_svc::opencode::Error::Limit))
    ));
    assert!(stream.next().await.is_none());
    task.abort();
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn controller_filesystem_aliases_do_not_expand_remote_directory_scope() -> anyhow::Result<()>
{
    let temporary = tempfile::tempdir()?;
    let selected = temporary.path().join("selected");
    std::fs::create_dir(&selected)?;
    let alias = temporary.path().join("untrusted-remote-path");
    std::os::unix::fs::symlink(&selected, &alias)?;
    let foreign = alias.to_string_lossy().into_owned();
    let app = Router::new().route(
        "/api/session/ses_one",
        get(move || {
            let directory = foreign.clone();
            async move { Json(json!({"data":{"id":"ses_one","location":{"directory":directory}}})) }
        }),
    );
    let (origin, task) = server(app).await?;
    let client = V2Client::new(&origin, &selected.to_string_lossy(), None)?;
    assert!(matches!(
        client.session("ses_one").await,
        Err(pocket_codex_host_svc::opencode::Error::Scope)
    ));
    task.abort();
    Ok(())
}

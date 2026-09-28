//! OpenCode client, discovery, contract and gateway against a fake upstream
//! that speaks the 2.0.18 wire shapes and enforces Basic authentication.

use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use pocket_codex_host_svc::opencode::{
    contract, discovery,
    gateway::{self, Gateway, Upstream},
    BasicCredentials, Client, Error, PermissionReply, PromptRequest, SessionQuery,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;

const FIXTURE: &str = include_str!("fixtures/opencode_2.0.18_openapi.json");
const AUTH: &str = "Basic b3BlbmNvZGU6c2VjcmV0"; // opencode:secret

#[derive(Default)]
struct Seen {
    requests: Vec<String>,
    bodies: Vec<Value>,
}

type Shared = Arc<Mutex<Seen>>;

fn authed(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(AUTH)
}

fn record(seen: &Shared, line: String, body: Option<Value>) {
    let mut seen = seen.lock().expect("seen");
    seen.requests.push(line);
    if let Some(body) = body {
        seen.bodies.push(body);
    }
}

fn message(id: &str, kind: &str, created: i64) -> Value {
    json!({"id": id, "type": kind, "time": {"created": created}, "text": format!("t-{id}")})
}

async fn fake(seen: Shared, version: &'static str) -> String {
    async fn guard(headers: HeaderMap) -> Result<(), Response> {
        if authed(&headers) {
            Ok(())
        } else {
            Err((StatusCode::UNAUTHORIZED, Json(json!({"_tag": "UnauthorizedError"})))
                .into_response())
        }
    }
    let app = Router::new()
        .route(
            "/api/info",
            get(move |headers: HeaderMap| async move {
                guard(headers).await?;
                Ok::<_, Response>(Json(json!({"version": version, "pid": 4242, "urls": [], "paths": {"tmp": "/tmp"}})))
            }),
        )
        .route(
            "/openapi.json",
            get(|headers: HeaderMap| async move {
                guard(headers).await?;
                Ok::<_, Response>(Json(serde_json::from_str::<Value>(FIXTURE).expect("fixture")))
            }),
        )
        .route(
            "/api/session",
            get(|State(seen): State<Shared>, headers: HeaderMap, Query(q): Query<Vec<(String, String)>>| async move {
                guard(headers).await?;
                record(&seen, format!("GET /api/session {q:?}"), None);
                Ok::<_, Response>(Json(json!({
                    "data": [
                        {"id": "ses_b", "location": {"directory": "/w/b"}, "time": {"created": 1, "updated": 5}, "title": "B"},
                        {"id": "ses_a", "location": {"directory": "/w/a"}, "time": {"created": 1, "updated": 3}}
                    ],
                    "cursor": {"next": "c1"}
                })))
            }),
        )
        .route(
            "/api/session/{id}/message",
            get(|State(seen): State<Shared>, headers: HeaderMap, Path(id): Path<String>, Query(q): Query<Vec<(String, String)>>| async move {
                guard(headers).await?;
                record(&seen, format!("GET messages {id} {q:?}"), None);
                let older = q.iter().any(|(k, _)| k == "cursor");
                let data = if older {
                    vec![message("msg_1", "user", 1)]
                } else {
                    vec![message("msg_3", "assistant", 3), message("msg_2", "user", 2)]
                };
                Ok::<_, Response>(Json(json!({"data": data, "cursor": {"next": "older"}})))
            }),
        )
        .route(
            "/api/session/{id}/prompt",
            post(|State(seen): State<Shared>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>| async move {
                guard(headers).await?;
                record(&seen, format!("POST prompt {id}"), Some(body.clone()));
                Ok::<_, Response>(Json(json!({"data": {
                    "id": "msg_9", "sessionID": id, "type": "user",
                    "delivery": body["delivery"], "payload": {"text": body["text"]}, "time": {"created": 9}
                }})))
            }),
        )
        .route(
            "/api/session/{id}/permission/{rid}/reply",
            post(|State(seen): State<Shared>, headers: HeaderMap, Path((id, rid)): Path<(String, String)>, Json(body): Json<Value>| async move {
                guard(headers).await?;
                record(&seen, format!("POST reply {id} {rid}"), Some(body));
                Ok::<_, Response>(StatusCode::NO_CONTENT)
            }),
        )
        .route(
            "/api/credential/{id}",
            get(|State(seen): State<Shared>| async move {
                record(&seen, "GET credential".into(), None);
                StatusCode::OK
            }),
        )
        .route(
            "/api/event",
            get(|headers: HeaderMap| async move {
                guard(headers).await?;
                let events = futures::stream::iter(vec![
                    Ok::<_, Infallible>(SseEvent::default().data(r#"{"type":"server.connected","data":{}}"#)),
                    Ok(SseEvent::default().data(
                        r#"{"type":"session.text.delta","data":{"sessionID":"ses_a","assistantMessageID":"msg_3","ordinal":0,"delta":"hi"}}"#,
                    )),
                ])
                .chain(futures::stream::pending());
                Ok::<_, Response>(Sse::new(events))
            }),
        )
        .with_state(seen);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{addr}/")
}

async fn gateway_for(upstream: &str) -> String {
    let gw = Gateway::new(
        Upstream::new(upstream, Some(BasicCredentials::new("opencode", "secret")))
            .expect("upstream"),
    )
    .expect("gateway");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(gateway::serve(listener, gw));
    format!("http://{addr}/")
}

#[test]
fn the_verified_contract_has_nothing_missing() {
    let doc: Value = serde_json::from_str(FIXTURE).expect("fixture");
    assert_eq!(contract::check(&doc).missing, Vec::<String>::new());

    let mut broken = doc.clone();
    broken["paths"]
        .as_object_mut()
        .expect("paths")
        .remove("/api/session/{sessionID}/interrupt");
    assert_eq!(contract::check(&broken).missing, vec![
        "POST /api/session/{sessionID}/interrupt".to_string()
    ]);
}

#[tokio::test]
async fn gateway_injects_credentials_and_hides_everything_else() {
    let seen = Shared::default();
    let upstream = fake(seen.clone(), "2.0.18").await;
    let gw = gateway_for(&upstream).await;

    // The controller side holds no credentials and still connects.
    let client = Client::new(&gw, None).expect("client");
    let (info, verified) = client.connect().await.expect("connect");
    assert_eq!(info.version, "2.0.18");
    assert!(verified);

    let http = reqwest::Client::new();
    let credential = http
        .get(format!("{gw}api/credential/x"))
        .send()
        .await
        .expect("send");
    assert_eq!(credential.status(), 404);
    assert!(!seen
        .lock()
        .expect("seen")
        .requests
        .iter()
        .any(|r| r.contains("credential")));

    // Without injected credentials the upstream challenge is not relayed.
    let bare = Gateway::new(Upstream::new(&upstream, None).expect("upstream")).expect("gateway");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let bare_addr = listener.local_addr().expect("addr");
    tokio::spawn(gateway::serve(listener, bare));
    let status = http
        .get(format!("http://{bare_addr}/api/info"))
        .send()
        .await
        .expect("send")
        .status();
    assert_eq!(status, 502);
}

#[tokio::test]
async fn client_pages_prompts_replies_and_streams_through_the_gateway() {
    let seen = Shared::default();
    let upstream = fake(seen.clone(), "2.0.18").await;
    let client = Client::new(&gateway_for(&upstream).await, None).expect("client");

    let page = client
        .sessions(&SessionQuery {
            roots_only: true,
            limit: 2,
            ..SessionQuery::default()
        })
        .await
        .expect("sessions");
    assert_eq!(page.sessions.len(), 2);
    assert_eq!(page.next_cursor.as_deref(), Some("c1"));
    assert_eq!(page.sessions[0].location.directory, "/w/b");

    let tail = client.messages("ses_a", 2, None, None).await.expect("tail");
    let ids: Vec<_> = tail.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["msg_2", "msg_3"], "chronological order");
    assert_eq!(tail.older_cursor.as_deref(), Some("older"));
    let older = client
        .messages("ses_a", 2, Some("older"), None)
        .await
        .expect("older");
    assert_eq!(older.messages.len(), 1);
    assert_eq!(older.older_cursor, None, "a short page is the oldest one");

    let accepted = client
        .prompt("ses_a", &PromptRequest {
            text: "hello".into(),
            files: vec![("file:///tmp/a.png".into(), Some("a.png".into()))],
            steer: true,
        })
        .await
        .expect("prompt");
    assert_eq!(accepted.delivery, "steer");
    client
        .reply_permission("ses_a", "per_1", PermissionReply::Always)
        .await
        .expect("reply");

    let bodies = seen.lock().expect("seen").bodies.clone();
    assert_eq!(bodies[0]["files"][0]["uri"], "file:///tmp/a.png");
    assert_eq!(bodies[1], json!({"decision": "always"}));
    let requests = seen.lock().expect("seen").requests.clone();
    assert!(requests[0].contains("(\"parentID\", \"null\")"), "{requests:?}");

    let mut events = client.events().await.expect("events");
    let first = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("first");
    assert_eq!(first.expect("item").expect("event").kind, "server.connected");
    let second = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("second")
        .expect("item")
        .expect("event");
    assert_eq!(second.session_id(), Some("ses_a"));
    assert_eq!(second.data["delta"], "hi");
}

#[tokio::test]
async fn discovery_reads_an_owner_only_registration() {
    let seen = Shared::default();
    let upstream = fake(seen, "2.0.19").await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("service.json");
    std::fs::write(
        &path,
        json!({"id": "x", "version": "2.0.19", "url": upstream.trim_end_matches('/'), "pid": 4242, "password": "secret"}).to_string(),
    )
    .expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        assert_eq!(
            discovery::discover_from_file(&path)
                .await
                .expect_err("must fail"),
            Error::InvalidInput
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
    let attached = discovery::discover_from_file(&path).await.expect("attach");
    assert_eq!(attached.info.version, "2.0.19");
    assert!(!attached.verified, "a newer contract-compatible version is accepted but unverified");

    assert_eq!(
        discovery::discover_from_file(&dir.path().join("missing.json"))
            .await
            .expect_err("must fail"),
        Error::NotRunning
    );
}

/// Read-only check against the user's real background service. Opt in with
/// `PCX_OPENCODE_LIVE=1`; it lists sessions, reads one tail and briefly
/// listens to events, without any mutation.
#[tokio::test]
async fn live_readonly_against_the_local_service() {
    if std::env::var_os("PCX_OPENCODE_LIVE").is_none() {
        return;
    }
    let attached = discovery::discover()
        .await
        .expect("discover the local OpenCode service");
    let gw = gateway_for_attached(&attached).await;
    let client = Client::new(&gw, None).expect("client");
    let (info, verified) = client
        .connect()
        .await
        .expect("contract through the gateway");
    eprintln!("OpenCode {} verified={verified}", info.version);
    let page = client
        .sessions(&SessionQuery {
            roots_only: true,
            limit: 5,
            ..SessionQuery::default()
        })
        .await
        .expect("sessions");
    assert!(page.sessions.iter().all(|s| s.parent_id.is_none()));
    if let Some(session) = page.sessions.first() {
        let tail = client
            .messages(&session.id, 20, None, None)
            .await
            .expect("tail");
        eprintln!("tail {} messages, older={}", tail.messages.len(), tail.older_cursor.is_some());
    }
    client.active().await.expect("active");
    let mut events = client.events().await.expect("events");
    let first = tokio::time::timeout(Duration::from_secs(10), events.next())
        .await
        .expect("first event");
    assert!(first.expect("item").is_ok());
}

async fn gateway_for_attached(attached: &discovery::Attached) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(gateway::serve(
        listener,
        Gateway::new(attached.upstream.clone()).expect("gateway"),
    ));
    format!("http://{addr}/")
}

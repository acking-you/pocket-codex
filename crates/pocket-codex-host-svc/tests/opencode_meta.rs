//! `/fs/thread-file` on the OpenCode meta service: session links resolve
//! against the working directory a fake OpenCode upstream reports.

use std::sync::Arc;

use axum::{extract::Path, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use pocket_codex_host_svc::{
    opencode::{Client, SessionDirs},
    serve_generic,
    store::{ConfigStore, HostStore},
};
use reqwest::StatusCode as Status;
use serde_json::json;
use tokio::net::TcpListener;

async fn listen(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

/// A fake OpenCode that knows one session, `ses_known`, located in `dir`.
async fn fake_opencode(dir: String) -> String {
    listen(Router::new().route(
        "/api/session/{id}",
        get(move |Path(id): Path<String>| {
            let dir = dir.clone();
            async move {
                if id == "ses_known" {
                    Json(json!({"data": {"id": id, "location": {"directory": dir}}}))
                        .into_response()
                } else {
                    (StatusCode::NOT_FOUND, Json(json!({"_tag": "NotFoundError"}))).into_response()
                }
            }
        }),
    ))
    .await
}

/// The meta service with no project roots, resolving sessions through `dirs`.
async fn meta(dir: &std::path::Path, dirs: SessionDirs) -> String {
    let store = Arc::new(
        ConfigStore::open(dir.join("threads.json"))
            .await
            .expect("store"),
    );
    let host = Arc::new(HostStore::open(dir.join("host.json")).await.expect("host"));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let uploads = dir.join("uploads");
    tokio::spawn(async move {
        let _ = serve_generic(listener, store, host, uploads, Arc::new(dirs)).await;
    });
    format!("http://{addr}")
}

async fn thread_file(base: &str, thread: &str, href: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{base}/fs/thread-file"))
        .query(&[("thread", thread), ("href", href), ("preview", "true")])
        .send()
        .await
        .expect("request")
}

#[tokio::test]
async fn session_links_resolve_against_the_opencode_session_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("notes")).expect("project");
    std::fs::write(project.join("notes/a.txt"), b"inside").expect("inside");
    let secret = temp.path().join("secret.txt");
    std::fs::write(&secret, b"outside").expect("secret");

    let upstream = fake_opencode(project.to_string_lossy().into_owned()).await;
    let base =
        meta(temp.path(), SessionDirs::new(Client::new(&upstream, None).expect("client"))).await;

    let inside = thread_file(&base, "ses_known", "notes/a.txt:3").await;
    assert_eq!(inside.status(), Status::OK);
    assert_eq!(inside.headers()["x-file-size"], "6");
    assert_eq!(inside.bytes().await.expect("body").as_ref(), b"inside");
    let absolute = url::Url::from_file_path(project.join("notes/a.txt")).expect("uri");
    assert_eq!(
        thread_file(&base, "ses_known", absolute.as_str())
            .await
            .status(),
        Status::OK
    );

    for denied in ["../secret.txt", secret.to_str().expect("utf8")] {
        let response = thread_file(&base, "ses_known", denied).await;
        assert_eq!(response.status(), Status::FORBIDDEN, "{denied}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, project.join("escape")).expect("symlink");
        assert_eq!(thread_file(&base, "ses_known", "escape").await.status(), Status::FORBIDDEN);
    }

    for unknown in ["ses_missing", "../ses_known"] {
        let response = thread_file(&base, unknown, "notes/a.txt").await;
        assert_eq!(response.status(), Status::NOT_FOUND, "{unknown}");
    }
}

#[tokio::test]
async fn session_lookups_follow_a_reattached_opencode_server() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("a.txt"), b"hello").expect("file");
    // Nothing listens on port 1: the original server is gone.
    let dirs = SessionDirs::new(Client::new("http://127.0.0.1:1", None).expect("client"));
    let base = meta(temp.path(), dirs.clone()).await;
    let gone = thread_file(&base, "ses_known", "a.txt").await;
    assert_eq!(gone.status(), Status::INTERNAL_SERVER_ERROR);

    let upstream = fake_opencode(temp.path().to_string_lossy().into_owned()).await;
    dirs.set_client(Client::new(&upstream, None).expect("client"));
    let found = thread_file(&base, "ses_known", "a.txt").await;
    assert_eq!(found.status(), Status::OK);
    assert_eq!(found.bytes().await.expect("body").as_ref(), b"hello");
}

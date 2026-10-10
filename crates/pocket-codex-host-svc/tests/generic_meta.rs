//! Generic meta routes confine file links to the directory of a known session.

use std::sync::Arc;

use pocket_codex_host_svc::{
    file_links::SessionDirResolver,
    serve_generic,
    store::{ConfigStore, HostStore},
};
use reqwest::StatusCode as Status;
use tokio::net::TcpListener;

struct SessionDirs(String);

#[async_trait::async_trait]
impl SessionDirResolver for SessionDirs {
    async fn session_dir(&self, session: &str) -> anyhow::Result<Option<String>> {
        Ok((session == "ses_known").then(|| self.0.clone()))
    }
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
async fn session_links_require_a_known_session_and_stay_within_its_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("notes")).expect("project");
    std::fs::write(project.join("notes/a.txt"), b"inside").expect("inside");
    let secret = temp.path().join("secret.txt");
    std::fs::write(&secret, b"outside").expect("secret");

    let base = meta(temp.path(), SessionDirs(project.to_string_lossy().into_owned())).await;

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

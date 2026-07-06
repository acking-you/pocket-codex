//! End-to-end HTTP test of the project-folder meta endpoints: it binds the real
//! router on a loopback listener and drives it with the same reqwest client the
//! bridge uses, so the whole path — routing, JSON (de)serialization, the
//! `?path=` query, and the root-confinement 403 — is exercised for real, not
//! just the handler helpers.

use std::{net::SocketAddr, sync::Arc};

use pocket_codex_host_svc::{
    serve,
    store::{ConfigStore, HostConfig, HostStore},
};
use tokio::net::TcpListener;

/// Bring up the meta service on an ephemeral loopback port over a temp
/// CODEX_HOME-like dir; returns the base URL, the created project root, and the
/// temp dir guard (kept alive for the test's duration).
async fn spawn() -> (String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    // A project root with one sub-directory to list.
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).expect("mkdir root");
    std::fs::create_dir(root.join("crate-a")).expect("mkdir sub");

    let store = Arc::new(
        ConfigStore::open(dir.path().join("threads.json"))
            .await
            .expect("config store"),
    );
    let host = Arc::new(
        HostStore::open(dir.path().join("host.json"))
            .await
            .expect("host store"),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("addr");
    // app_ws_addr is unused by the /projects and /fs/list handlers.
    let dummy: SocketAddr = "127.0.0.1:1".parse().expect("dummy addr");
    tokio::spawn(async move {
        let _ = serve(listener, dummy, store, host).await;
    });

    (format!("http://{addr}"), root.to_string_lossy().into_owned(), dir)
}

#[tokio::test]
async fn projects_round_trip_and_confined_listing() {
    let (base, root, _guard) = spawn().await;
    let client = reqwest::Client::new();

    // Fresh host: no roots yet.
    let cfg: HostConfig = client
        .get(format!("{base}/projects"))
        .send()
        .await
        .expect("get projects")
        .json()
        .await
        .expect("decode projects");
    assert!(cfg.project_roots.is_empty());

    // Configure a root + default.
    let put = HostConfig {
        project_roots: vec![root.clone()],
        default_project: Some(root.clone()),
    };
    let stored: HostConfig = client
        .put(format!("{base}/projects"))
        .json(&put)
        .send()
        .await
        .expect("put projects")
        .json()
        .await
        .expect("decode put");
    assert_eq!(stored.project_roots, vec![root.clone()]);
    assert_eq!(stored.default_project.as_deref(), Some(root.as_str()));

    // Listing the root returns its sub-directory.
    let resp = client
        .get(format!("{base}/fs/list"))
        .query(&[("path", root.as_str())])
        .send()
        .await
        .expect("list root");
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.expect("decode list");
    let names: Vec<&str> = body["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|e| e["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["crate-a"]);

    // Listing OUTSIDE the configured roots is refused with 403.
    let outside = client
        .get(format!("{base}/fs/list"))
        .query(&[("path", "/definitely/not/a/root")])
        .send()
        .await
        .expect("list outside");
    assert_eq!(outside.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn file_transfer_list_download_upload_confined() {
    let (base, root, _guard) = spawn().await;
    let client = reqwest::Client::new();

    // Configure the root.
    let put = HostConfig {
        project_roots: vec![root.clone()],
        default_project: None,
    };
    client
        .put(format!("{base}/projects"))
        .json(&put)
        .send()
        .await
        .expect("put projects");

    // Seed a file in the root.
    std::fs::write(std::path::Path::new(&root).join("report.txt"), b"hello").expect("seed file");

    // List files (not the crate-a sub-directory).
    let resp = client
        .get(format!("{base}/fs/files"))
        .query(&[("path", root.as_str())])
        .send()
        .await
        .expect("list files");
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.expect("decode files");
    let names: Vec<&str> = body["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|e| e["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["report.txt"]);

    // Download the file's bytes.
    let file_path = format!("{root}/report.txt");
    let dl = client
        .get(format!("{base}/fs/read"))
        .query(&[("path", file_path.as_str())])
        .send()
        .await
        .expect("read file");
    assert!(dl.status().is_success());
    assert_eq!(dl.bytes().await.expect("bytes").as_ref(), b"hello");

    // Reading outside the roots is refused with 403.
    let outside = client
        .get(format!("{base}/fs/read"))
        .query(&[("path", "/definitely/not/a/root/x")])
        .send()
        .await
        .expect("read outside");
    assert_eq!(outside.status(), reqwest::StatusCode::FORBIDDEN);

    // Upload a new file into the root.
    let up = client
        .post(format!("{base}/fs/write"))
        .query(&[("dir", root.as_str()), ("name", "note.txt")])
        .body("uploaded".to_string())
        .send()
        .await
        .expect("write file");
    assert!(up.status().is_success());
    assert_eq!(
        std::fs::read(std::path::Path::new(&root).join("note.txt")).expect("read note"),
        b"uploaded"
    );

    // Re-uploading the same name never overwrites → 409.
    let dup = client
        .post(format!("{base}/fs/write"))
        .query(&[("dir", root.as_str()), ("name", "note.txt")])
        .body("again".to_string())
        .send()
        .await
        .expect("write dup");
    assert_eq!(dup.status(), reqwest::StatusCode::CONFLICT);

    // Uploading into a directory outside the roots is refused with 403.
    let out = client
        .post(format!("{base}/fs/write"))
        .query(&[("dir", "/definitely/not/a/root"), ("name", "x.txt")])
        .body("x".to_string())
        .send()
        .await
        .expect("write outside");
    assert_eq!(out.status(), reqwest::StatusCode::FORBIDDEN);
}

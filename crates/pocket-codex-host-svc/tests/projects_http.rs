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
async fn large_json_negotiates_compression_and_legacy_identity_still_works() {
    let (base, _root, _guard) = spawn().await;
    let text = "long-model-configuration-".repeat(1000);
    let url = format!("{base}/threads/compression/config");
    let client = reqwest::Client::new();
    client
        .put(&url)
        .json(&serde_json::json!({"model": text}))
        .send()
        .await
        .expect("store")
        .error_for_status()
        .expect("stored");
    let wire = reqwest::Client::builder()
        .no_zstd()
        .no_gzip()
        .build()
        .expect("raw client");
    for encoding in ["zstd", "gzip", "identity"] {
        let response = wire
            .get(&url)
            .header("accept-encoding", encoding)
            .send()
            .await
            .expect("response");
        if encoding == "identity" {
            assert!(!response.headers().contains_key("content-encoding"));
            assert_eq!(response.json::<serde_json::Value>().await.expect("json")["model"], text);
        } else {
            assert_eq!(response.headers()["content-encoding"], encoding);
            assert!(response.bytes().await.expect("compressed body").len() < text.len() / 5);
            let decoded: serde_json::Value = client
                .get(&url)
                .header("accept-encoding", encoding)
                .send()
                .await
                .expect("auto decode")
                .json()
                .await
                .expect("json");
            assert_eq!(decoded["model"], text);
        }
    }
}

#[tokio::test]
#[ignore = "manual: PCX_HISTORY_THREAD_ID selects an existing local long session"]
async fn real_monitor_first_event_omits_the_full_rollout() {
    let thread = std::env::var("PCX_HISTORY_THREAD_ID").expect("thread id");
    let (base, _, _guard) = spawn().await;
    let started = std::time::Instant::now();
    let mut response = reqwest::Client::new()
        .get(format!("{base}/sessions/{thread}/follow?metadata_only=true"))
        .send()
        .await
        .expect("follow")
        .error_for_status()
        .expect("success");
    let chunk = response
        .chunk()
        .await
        .expect("read event")
        .expect("first event");
    let text = std::str::from_utf8(&chunk).expect("utf8");
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data");
    let update: serde_json::Value = serde_json::from_str(data).expect("update");
    assert_eq!(update["items"], serde_json::json!([]));
    assert!(update["history_revision"].is_string());
    assert!(chunk.len() < 4096);
    eprintln!("metadata-only first event: {} bytes in {:?}", chunk.len(), started.elapsed());
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

/// Run against an isolated CODEX_HOME containing an actual completed native
/// generation; no model call, credentials, relay registration or live-host
/// edits.
#[tokio::test]
#[ignore = "requires PCX_IMAGE_THREAD_ID, PCX_IMAGE_OUTPUT and isolated CODEX_HOME"]
async fn native_generated_image_round_trips_over_meta_http() {
    let thread = std::env::var("PCX_IMAGE_THREAD_ID").expect("isolated generated thread");
    let path = std::env::var("PCX_IMAGE_OUTPUT").expect("native generated image path");
    let expected = std::fs::read(&path).expect("native image");
    let (base, _root, _guard) = spawn().await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base}/fs/thread-image"))
        .query(&[("thread", &thread), ("path", &path)])
        .send()
        .await
        .expect("image response")
        .error_for_status()
        .expect("authorized artifact");
    let downloaded = response.bytes().await.expect("remote image bytes");
    assert_eq!(downloaded.as_ref(), expected);
    let denied = client
        .get(format!("{base}/fs/thread-image"))
        .query(&[("thread", &thread), ("path", &format!("{path}.other.png"))])
        .send()
        .await
        .expect("unreferenced image response");
    assert!(!denied.status().is_success());
    println!("verified native generated artifact: {} bytes over meta HTTP", downloaded.len());
}

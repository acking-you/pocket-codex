//! Read-only local discovery contracts using disposable registration files.

use std::{io::Write, path::Path, sync::Arc};

use axum::{http::StatusCode, routing::get, Json, Router};
use pocket_codex_host_svc::opencode::{
    discovery::{discover, discover_from_file},
    Error,
};
use serde_json::json;

fn registration_file(path: &Path, origin: &str) -> anyhow::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(
        serde_json::to_string(&json!({
            "id": "fixture-service",
            "version": "2.0.18",
            "url": origin,
            "pid": 42,
            "password": "fixture-secret"
        }))?
        .as_bytes(),
    )?;
    Ok(())
}

async fn no_lifecycle_request() -> StatusCode {
    panic!("discovery must not call lifecycle endpoints")
}

#[tokio::test]
async fn authentication_failure_preserves_registration_and_redacts_secrets() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let router = Router::new()
        .route(
            "/api/info",
            get(move |headers: axum::http::HeaderMap| {
                seen.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(headers["authorization"], "Basic b3BlbmNvZGU6Zml4dHVyZS1zZWNyZXQ=");
                    (StatusCode::UNAUTHORIZED, "fixture-secret")
                }
            }),
        )
        .fallback(no_lifecycle_request);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, &origin)?;
    let original = std::fs::read(&path)?;

    let result = discover_from_file(&path, "/project").await;
    server.abort();

    assert!(matches!(result, Err(Error::Rejected(401))));
    assert!(!format!("{result:?}").contains("fixture-secret"));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(path)?, original);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_or_shared_registration_files_before_authentication() -> anyhow::Result<()>
{
    use std::os::unix::fs::{symlink, PermissionsExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let router = Router::new().route("/api/info", get(|| async { StatusCode::UNAUTHORIZED }));
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let root = tempfile::tempdir()?;
    let target = root.path().join("actual-registration.json");
    registration_file(&target, &origin)?;
    let linked = root.path().join("service.json");
    symlink(&target, &linked)?;

    assert!(matches!(discover_from_file(&linked, "/project").await, Err(Error::InvalidInput)));
    for mode in [0o640, 0o604, 0o666, 0o200] {
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))?;
        assert!(matches!(discover_from_file(&target, "/project").await, Err(Error::InvalidInput)));
        assert_eq!(std::fs::metadata(&target)?.permissions().mode() & 0o777, mode);
    }
    server.abort();
    Ok(())
}

#[tokio::test]
async fn bounds_registration_input_before_parsing_or_contacting_a_service() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, "http://127.0.0.1:1")?;
    let mut file = std::fs::OpenOptions::new().append(true).open(&path)?;
    file.write_all(&vec![b' '; 16 * 1024])?;

    assert!(matches!(discover_from_file(&path, "/project").await, Err(Error::Limit)));
    Ok(())
}

fn ready_service(pid: u32) -> Router {
    Router::new()
        .route(
            "/api/info",
            get(move || async move {
                Json(json!({
                    "version": "2.0.18", "pid": pid,
                    "urls": ["https://untrusted.invalid"], "paths": {"tmp": "/tmp"}
                }))
            }),
        )
        .route(
            "/openapi.json",
            get(|| async {
                Json(
                    serde_json::from_str::<serde_json::Value>(include_str!(
                        "fixtures/opencode_v2_contract.json"
                    ))
                    .expect("fixed official contract"),
                )
            }),
        )
        .fallback(no_lifecycle_request)
}

#[tokio::test]
async fn discovers_a_ready_private_registration_without_rewriting_it() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, ready_service(42)).await });
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, &origin)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))?;
    }
    let original = std::fs::read(&path)?;

    let client = discover_from_file(&path, "/project").await?;
    assert_eq!(client.directory(), "/project");
    assert!(!format!("{client:?}").contains("fixture-secret"));
    assert_eq!(std::fs::read(path)?, original);
    drop(client);
    assert!(!server.is_finished());
    server.abort();
    Ok(())
}

#[tokio::test]
async fn rejects_a_registration_whose_pid_does_not_match_the_live_service() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, ready_service(43)).await });
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, &origin)?;

    let result = discover_from_file(&path, "/project").await;
    server.abort();
    assert!(matches!(result, Err(Error::Protocol)));
    assert!(!format!("{result:?}").contains("fixture-secret"));
    Ok(())
}

#[tokio::test]
async fn rejects_untrusted_origins_before_sending_discovered_credentials() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let router = Router::new().fallback(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        async { StatusCode::UNAUTHORIZED }
    });
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let root = tempfile::tempdir()?;

    for (index, origin) in [
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}/api/"),
        format!("http://127.0.0.1:{port}/?auth_token=fixture-secret"),
        format!("http://127.0.0.1:{port}/#fixture-secret"),
        format!("http://opencode:fixture-secret@127.0.0.1:{port}"),
        format!("http://0.0.0.0:{port}"),
        "https://untrusted.invalid".to_string(),
        "file:///private/fixture-secret".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = root.path().join(format!("registration-{index}.json"));
        registration_file(&path, &origin)?;
        let result = discover_from_file(&path, "/project").await;
        assert!(matches!(result, Err(Error::InvalidInput)));
        assert!(!format!("{result:?}").contains("fixture-secret"));
    }
    server.abort();
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn rejects_unversioned_or_invalid_process_registrations_without_network() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    for (index, changes) in [
        json!({"version": "1.18.32"}),
        json!({"version": "2.0.19"}),
        json!({"version": null}),
        json!({"pid": 0}),
        json!({"pid": -1}),
        json!({"pid": 1.5}),
        json!({"pid": null}),
    ]
    .into_iter()
    .enumerate()
    {
        let path = root.path().join(format!("registration-{index}.json"));
        registration_file(&path, "http://127.0.0.1:1")?;
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        for (key, replacement) in changes.as_object().expect("fixture object") {
            value[key] = replacement.clone();
        }
        std::fs::write(&path, serde_json::to_vec(&value)?)?;
        let result = discover_from_file(&path, "/project").await;
        assert!(matches!(result, Err(Error::InvalidInput | Error::Protocol)));
    }
    Ok(())
}

#[tokio::test]
async fn discovery_uses_isolated_default_registration_paths() -> anyhow::Result<()> {
    const PROBE_ENV: &str = "PCX_DISCOVERY_PATH_FIXTURE";
    if std::env::var_os(PROBE_ENV).is_some() {
        assert!(matches!(discover("/project").await, Err(Error::Protocol)));
        return Ok(());
    }

    for use_xdg in [true, false] {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        let state = if use_xdg { root.path().join("xdg-state") } else { home.join(".local/state") };
        let directory = state.join("opencode");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("service.json");
        registration_file(&path, "http://127.0.0.1:1")?;
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        value["version"] = json!("1.18.32");
        std::fs::write(&path, serde_json::to_vec(&value)?)?;

        let mut child = tokio::process::Command::new(std::env::current_exe()?);
        child
            .arg("--exact")
            .arg("discovery_uses_isolated_default_registration_paths")
            .env(PROBE_ENV, "1")
            .env("HOME", &home);
        if use_xdg {
            child.env("XDG_STATE_HOME", &state);
        } else {
            child.env_remove("XDG_STATE_HOME");
        }
        let output = child.output().await?;
        assert!(
            output.status.success(),
            "isolated default discovery failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[tokio::test]
async fn discovered_credentials_never_follow_redirects() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let router = Router::new()
        .route(
            "/api/info",
            get(|| async { axum::response::Redirect::temporary("/credential-sink") }),
        )
        .route(
            "/credential-sink",
            get(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::UNAUTHORIZED }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, &origin)?;

    let result = discover_from_file(&path, "/project").await;
    server.abort();
    assert!(matches!(result, Err(Error::Rejected(307))));
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn malformed_or_nonregular_registrations_fail_without_exposing_their_contents(
) -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("service.json");
    registration_file(&path, "http://127.0.0.1:1")?;
    std::fs::write(&path, b"{\"password\":\"fixture-secret\"")?;

    for invalid in [path.as_path(), root.path()] {
        let result = discover_from_file(invalid, "/project").await;
        assert!(matches!(result, Err(Error::InvalidInput)));
        assert!(!format!("{result:?}").contains("fixture-secret"));
    }
    #[cfg(unix)]
    {
        let socket = root.path().join("service.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket)?;
        assert!(matches!(discover_from_file(&socket, "/project").await, Err(Error::InvalidInput)));
    }
    Ok(())
}

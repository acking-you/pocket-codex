//! Opt-in verification against an isolated, user-supplied OpenCode 2.0.18
//! binary.
//!
//! The test creates only temporary OpenCode state and a temporary session. It
//! never attaches to the user's registration, sends a model prompt, or emits
//! credentials and upstream object identities.

use std::{
    net::TcpListener,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{ensure, Context, Result};
use pocket_codex_host_svc::opencode::{
    connection::Connection, v2::V2Client, BasicCredentials, OpenCodeGateway,
};
use tempfile::TempDir;
use tokio::process::Child;

#[tokio::test]
#[ignore = "requires PCX_TEST_OPENCODE_BINARY; starts an isolated OpenCode service"]
async fn real_opencode_v2_reads_and_hosts_an_isolated_session() -> Result<()> {
    if std::env::var("PCX_RUN_REAL_OPENCODE").as_deref() != Ok("1") {
        return Ok(());
    }
    let binary = PathBuf::from(
        std::env::var_os("PCX_TEST_OPENCODE_BINARY")
            .context("PCX_TEST_OPENCODE_BINARY required")?,
    );
    ensure!(binary.is_absolute(), "PCX_TEST_OPENCODE_BINARY must be absolute");
    let temporary = tempfile::tempdir()?;
    let directory = temporary.path().join("project");
    std::fs::create_dir(&directory)?;
    let server_directory = temporary.path().join("server-default");
    std::fs::create_dir(&server_directory)?;
    for name in ["home", "config", "data", "cache", "state"] {
        std::fs::create_dir(temporary.path().join(name))?;
    }

    let reserved = TcpListener::bind("127.0.0.1:0")?;
    let address = reserved.local_addr()?;
    drop(reserved);
    let password = format!("pocket-test-{}", uuid::Uuid::new_v4().simple());
    let mut process = spawn_server(&binary, &temporary, &server_directory, address, &password)?;
    let origin = format!("http://{address}");
    let credentials = Some(BasicCredentials::new("opencode", password));
    let directory_text = directory.to_str().context("temporary path is UTF-8")?;
    let native = V2Client::new(&origin, directory_text, credentials)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let info = loop {
        match native.connect().await {
            Ok(info) => break info,
            Err(error) => {
                ensure!(
                    Instant::now() < deadline && process.try_wait()?.is_none(),
                    "isolated OpenCode service did not become ready: {error}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            },
        }
    };
    ensure!(
        info.version == "2.0.18"
            && info.pid == u64::from(process.id().context("service pid missing")?)
    );

    let direct = Connection::from(native.clone());
    ensure!(direct.sessions(None).await?.is_empty());
    let created = direct.create(Some("Pocket backend verification")).await?;
    let listed = direct.sessions(None).await?;
    ensure!(listed.iter().any(|session| session.id == created.id));
    ensure!(direct
        .history(&created.id, 20, None)
        .await?
        .messages
        .is_empty());
    ensure!(direct.permissions().await?.is_empty());
    ensure!(direct.questions().await?.is_empty());

    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    let gateway_handle = OpenCodeGateway::new(direct.clone()).serve(gateway_listener)?;
    let gateway =
        Connection::connect(&format!("http://{gateway_addr}"), directory_text, None).await?;
    ensure!(gateway.is_v2());
    let hosted = gateway.sessions(None).await?;
    ensure!(hosted.iter().any(|session| session.id == created.id));
    ensure!(gateway
        .history(&created.id, 20, None)
        .await?
        .messages
        .is_empty());
    ensure!(gateway.permissions().await?.is_empty());
    ensure!(gateway.questions().await?.is_empty());
    gateway_handle.stop().await;

    let after = native.connect().await?;
    ensure!((info.version, info.pid) == (after.version, after.pid));
    if process.try_wait()?.is_none() {
        process.kill().await?;
    }
    process.wait().await?;
    drop(temporary);
    Ok(())
}

fn spawn_server(
    binary: &PathBuf,
    temporary: &TempDir,
    directory: &std::path::Path,
    address: std::net::SocketAddr,
    password: &str,
) -> Result<Child> {
    let root = temporary.path();
    let env = [
        ("HOME", root.join("home")),
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_CACHE_HOME", root.join("cache")),
        ("XDG_STATE_HOME", root.join("state")),
        ("OPENCODE_CONFIG_DIR", root.join("config")),
    ];
    let mut command = tokio::process::Command::new(binary);
    command
        .args(["serve", "--hostname", "127.0.0.1", "--port", &address.port().to_string()])
        .current_dir(directory)
        .env("OPENCODE_SERVER_PASSWORD", password)
        .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "true")
        .env("OPENCODE_CONFIG_CONTENT", "{\"plugin\":[]}")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    Ok(command.kill_on_drop(true).spawn()?)
}

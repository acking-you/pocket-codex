//! Reproduce app-server stalls without the relay or Flutter.
//!
//! `cargo run -p pocket-codex-codex --example app_server_probe -- <ws-url>`
//! adds repeated list/read requests to an existing server. Build with
//! `--features embedded-codex` and append `--embedded` to start an isolated
//! listener using the same Codex configuration as the desktop host.
//! `--resume` also resumes threads before reading history; use a copied
//! `CODEX_HOME` for this mode. `--hold` keeps the listener up for a UI client.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use pocket_codex_codex::client::AppClient;
use serde_json::{json, Value};

async fn request(client: &AppClient, method: &str, params: Value) -> Result<Value> {
    let started = Instant::now();
    let result = client.request(method, params).await;
    println!(
        "{method}: {:?}, ok={}, alive={}",
        started.elapsed(),
        result.is_ok(),
        client.is_alive()
    );
    result.with_context(|| format!("{method} failed after {:?}", started.elapsed()))
}

fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()?
        .block_on(run())
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args
        .first()
        .context("expected ws://host:port [--embedded] [--resume] [--hold]")?;
    if args.iter().any(|arg| arg == "--embedded") {
        #[cfg(feature = "embedded-codex")]
        {
            let listen = url.clone();
            tokio::spawn(async move {
                if let Err(error) = pocket_codex_codex::embedded::run(&listen).await {
                    eprintln!("embedded app-server: {error:#}");
                }
            });
        }
        #[cfg(not(feature = "embedded-codex"))]
        anyhow::bail!("--embedded requires --features embedded-codex");
    }

    for round in 1..=5 {
        let deadline = Instant::now() + Duration::from_secs(20);
        let (client, mut events) = loop {
            match AppClient::connect(url).await {
                Ok(connection) => break connection,
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        };
        let notifications = tokio::spawn(async move { while events.recv().await.is_some() {} });
        println!("connection {round}");
        request(
            &client,
            "initialize",
            json!({
                "clientInfo": {"name": "pocket-codex", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            }),
        )
        .await?;
        for _ in 0..3 {
            let list =
                request(&client, "thread/list", json!({"limit": 100, "sortKey": "updated_at"}))
                    .await?;
            if let Some(threads) = list["data"].as_array() {
                println!("threads={}", threads.len());
                for thread in threads.iter().skip(round - 1).take(3) {
                    if args.iter().any(|arg| arg == "--resume") {
                        request(&client, "thread/resume", json!({"threadId": thread["id"]}))
                            .await?;
                    }
                    let metadata = request(
                        &client,
                        "thread/read",
                        json!({
                            "threadId": thread["id"], "includeTurns": false
                        }),
                    )
                    .await?;
                    if args.iter().any(|arg| arg == "--resume") {
                        read_history(&client, &metadata["thread"]).await?;
                    }
                }
            }
        }
        drop(client);
        notifications.await?;
    }
    if args.iter().any(|arg| arg == "--hold") {
        println!("probe complete; waiting for Ctrl+C");
        tokio::signal::ctrl_c().await?;
    }
    Ok(())
}

async fn read_history(client: &AppClient, thread: &Value) -> Result<()> {
    if thread["historyMode"] != "paginated" {
        request(client, "thread/read", json!({"threadId": thread["id"], "includeTurns": true}))
            .await?;
        return Ok(());
    }
    for view in ["notLoaded", "summary"] {
        request(
            client,
            "thread/turns/list",
            json!({
                "threadId": thread["id"], "limit": 100, "sortDirection": "desc", "itemsView": view
            }),
        )
        .await?;
    }
    let mut cursor = Value::Null;
    for _ in 0..3 {
        let page = request(
            client,
            "thread/items/list",
            json!({
                "threadId": thread["id"], "limit": 100, "sortDirection": "desc", "cursor": cursor
            }),
        )
        .await?;
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    Ok(())
}

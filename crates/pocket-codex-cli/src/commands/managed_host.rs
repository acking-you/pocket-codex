//! Foreground computer management using the desktop's native hosting engine.

use std::{sync::mpsc, time::Duration};

use anyhow::{bail, Context, Result};
use pocket_codex_bridge::headless::{Manager, Options};
use pocket_codex_core::{config::Config, paths};

use super::{api_proxy, relay, ui};
use crate::cli::ServeArgs;

pub(super) fn state_dir() -> Result<std::path::PathBuf> {
    Ok(paths::state_dir()?.join("managed-host"))
}

pub(super) async fn run(args: ServeArgs) -> Result<()> {
    if args.host != "127.0.0.1" {
        bail!(
            "--allow-remote-management requires --host 127.0.0.1; devices connect through the \
             relay"
        );
    }
    let config = Config::load()?;
    let explicit_relay = args
        .relay
        .relay
        .as_deref()
        .map(|address| relay::resolve_session(Some(address), &config))
        .transpose()?;
    let options = Options {
        config_dir: paths::config_dir()?,
        state_dir: state_dir()?,
        device: super::host_identity::device(args.device)?,
        identity_dir: super::host_identity::directory()?,
        name: args.name,
        binary: args
            .codex_binary
            .map(|path| {
                path.into_os_string()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("the Codex binary path must be valid UTF-8"))
            })
            .transpose()?,
        port: args.port,
        proxy: api_proxy::resolve_app_server_proxy(args.proxy.as_deref())?,
        relay: explicit_relay,
    };
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    let (stop_tx, stop_rx) = mpsc::channel();
    let mut task = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut manager = Manager::start(options)?;
        ui::headline(ui::Tone::Ok, "managed computer hosting — Ctrl-C to stop");
        let mut previous_key = None;
        loop {
            let status = manager.write_status()?;
            if status.management.service_key != previous_key {
                if let Some(key) = &status.management.service_key {
                    ui::field("management", key);
                }
                previous_key = status.management.service_key;
            }
            match stop_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {},
            }
        }
        manager.stop()
    });
    tokio::select! {
        result = &mut task => return result.context("managed-host supervisor failed")?,
        result = &mut interrupt => result.context("waiting for Ctrl-C")?,
        _ = async {
            #[cfg(unix)]
            terminate.recv().await;
            #[cfg(not(unix))]
            std::future::pending::<()>().await;
        } => {},
    }
    let _ = stop_tx.send(());
    task.await.context("stopping managed-host supervisor")?
}

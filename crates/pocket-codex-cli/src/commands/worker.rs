//! Hidden foreground worker entrypoints spawned by high-level commands.

use std::time::Duration;

use anyhow::{bail, Result};
use pocket_codex_core::{
    config::{Config, Mode},
    paths,
    state::{PbRole, PbSessionInfo},
};
use pocket_codex_pb::{
    register_background as pb_register, subscribe_background as pb_subscribe, RegisterOptions,
    RelaySession, SubscribeOptions, TunnelStatus,
};

use crate::{
    cli::WorkerCmd,
    commands::{account, api_proxy, worker_health::Reporter},
};

/// Run an internal worker command.
pub async fn run(cmd: WorkerCmd) -> Result<()> {
    match cmd {
        WorkerCmd::PbRegister(args) => {
            // The parent always passes `--relay` and exports the credential, so
            // config is only consulted for parity with other commands; load it
            // best-effort so a broken config.toml can't fail a worker that never
            // needs it.
            let config = Config::load().unwrap_or_default();
            let session =
                crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
            let reporter =
                reporter(PbRole::Register, &args.key, &args.local_addr, &session, args.codec)?;
            let registration = pb_register(&session, RegisterOptions {
                key: args.key,
                local_addr: args.local_addr,
                codec: args.codec,
            })
            .await?;
            let _refresh = keep_account_credential_alive(&session, &config);
            let outcome =
                monitor(&reporter, registration.subscribe(), || registration.diagnostics()).await;
            registration.stop().await?;
            outcome?;
        },
        WorkerCmd::PbSubscribe(args) => {
            let config = Config::load().unwrap_or_default();
            let session =
                crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
            let reporter =
                reporter(PbRole::Subscribe, &args.key, &args.local_addr, &session, false)?;
            let connection = pb_subscribe(&session, SubscribeOptions {
                key: args.key,
                local_addr: args.local_addr,
            })
            .await?;
            let _refresh = keep_account_credential_alive(&session, &config);
            let outcome =
                monitor(&reporter, connection.subscribe(), || connection.diagnostics()).await;
            connection.stop().await?;
            outcome?;
        },
        WorkerCmd::ApiProxy(args) => api_proxy::run(args.listen, args.proxy).await?,
    }
    Ok(())
}

/// Keep this worker's relay credential from lapsing, for as long as it runs.
///
/// A detached worker outlives the command that spawned it — that is the point —
/// so in account mode it holds a credential with a finite life and nothing else
/// to renew it. Expiry does not merely refuse the next request: the relay
/// cancels the credential's lease and tears down every tunnel it opened, so a
/// service hosted from the CLI would go unreachable after its TTL while the
/// worker process sat there looking healthy (and `managed_pb::ensure` would
/// keep reusing it, since the pid is alive).
///
/// Asking the backend for the credential again is what renews it: the backend
/// renews rather than re-mints, which extends the existing lease and returns
/// the same credential string — so this worker's live tunnel keeps working and
/// there is no new value to install anywhere.
///
/// Returns a guard whose drop stops refreshing. `None` in self-host mode, where
/// the operator's own key does not expire.
fn keep_account_credential_alive(session: &RelaySession, config: &Config) -> Option<OwnedTask> {
    if !session.credential.starts_with("pbmt1_") || config.account_mode() != Mode::Account {
        return None;
    }
    let backend = account::backend_base(None, config);
    let initial_config = config.clone();
    Some(OwnedTask(tokio::spawn(async move {
        let mut delay = Duration::from_secs(2);
        let expires_at = loop {
            let mut config = Config::load().unwrap_or_else(|_| initial_config.clone());
            match account::fetch_relay_credential(&mut config, &backend).await {
                Ok(relay) => break relay.expires_at,
                Err(error) => {
                    tracing::warn!(%error, retry_secs = delay.as_secs(), "credential expiry lookup failed; retrying");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(30));
                },
            }
        };
        let mut refresh =
            OwnedTask(pocket_codex_pb::keep_credential_alive(expires_at, move || {
                let backend = backend.clone();
                async move {
                    let mut config = Config::load()?;
                    Ok(account::fetch_relay_credential(&mut config, &backend)
                        .await?
                        .expires_at)
                }
            }));
        let _ = (&mut refresh.0).await;
    })))
}

struct OwnedTask(tokio::task::JoinHandle<()>);
impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn reporter(
    role: PbRole,
    key: &str,
    local_addr: &str,
    session: &RelaySession,
    codec: bool,
) -> Result<Reporter> {
    Reporter::new(&PbSessionInfo {
        role,
        key: key.to_string(),
        local_addr: local_addr.to_string(),
        relay_addr: session.relay_addr.clone(),
        pid: std::process::id(),
        log_file: paths::pb_log_file(role, key)?,
        codec,
        started_at: chrono::Utc::now().to_rfc3339(),
    })
}

async fn monitor(
    reporter: &Reporter,
    mut status: tokio::sync::watch::Receiver<TunnelStatus>,
    diagnostics: impl Fn() -> pocket_codex_pb::TunnelDiagnostics,
) -> Result<()> {
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut snapshot_failed = false;
    loop {
        let current = status.borrow_and_update().clone();
        if let Err(error) = reporter.write(current.clone(), diagnostics()) {
            if !snapshot_failed {
                tracing::warn!(%error, "worker runtime snapshot unavailable");
            }
            snapshot_failed = true;
        } else {
            snapshot_failed = false;
        }
        match current {
            TunnelStatus::Failed(reason) => bail!("network worker failed: {reason}"),
            TunnelStatus::Stopped => return Ok(()),
            _ => {},
        }
        tokio::select! {
            result = &mut shutdown => return result,
            _ = interval.tick() => {},
            result = status.changed() => { if result.is_err() { bail!("network worker stopped reporting status"); } },
        }
    }
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

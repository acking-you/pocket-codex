//! Hidden foreground worker entrypoints spawned by high-level commands.

use anyhow::Result;
use pocket_codex_core::config::{Config, Mode};
use pocket_codex_pb::{
    register as pb_register, subscribe as pb_subscribe, RegisterOptions, RelaySession,
    SubscribeOptions,
};

use crate::{
    cli::WorkerCmd,
    commands::{account, api_proxy},
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
            let registration = pb_register(&session, RegisterOptions {
                key: args.key,
                local_addr: args.local_addr,
                codec: args.codec,
            })
            .await?;
            // Keeps the credential alive under this registration; see
            // [`keep_account_credential_alive`] for why a register worker cannot
            // skip it.
            let _refresh = keep_account_credential_alive(&session, &config).await;
            // A worker's whole job is to hold the tunnel open, so it parks here
            // until it is signalled. Returning would drop the handle and take
            // the registration down with it.
            tokio::signal::ctrl_c().await?;
            registration.stop().await?;
        },
        WorkerCmd::PbSubscribe(args) => {
            let config = Config::load().unwrap_or_default();
            let session =
                crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
            let connection = pb_subscribe(&session, SubscribeOptions {
                key: args.key,
                local_addr: args.local_addr,
            })
            .await?;
            let _refresh = keep_account_credential_alive(&session, &config).await;
            tokio::signal::ctrl_c().await?;
            connection.stop().await?;
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
async fn keep_account_credential_alive(
    session: &RelaySession,
    config: &Config,
) -> Option<tokio::task::JoinHandle<()>> {
    // A permanent (32-byte administrator or operator) key needs no refreshing;
    // only a `pbmt1_` temporary credential does.
    if !session.credential.starts_with("pbmt1_") || config.account_mode() != Mode::Account {
        return None;
    }
    let backend = account::backend_base(None, config);
    // The expiry has to come from the backend: the parent passed the credential
    // through the environment, not its lifetime.
    let expires_at = match account::fetch_relay_credential(&mut config.clone(), &backend).await {
        Ok(relay) => relay.expires_at,
        Err(err) => {
            // Non-fatal: the tunnel is already up and works until the credential
            // lapses. Warn rather than fail, so a transient backend outage does
            // not take down a worker that is currently serving fine.
            tracing::warn!(
                error = %format!("{err:#}"),
                "could not read the relay credential's expiry; this worker will stop serving when \
                 it lapses"
            );
            return None;
        },
    };
    Some(pocket_codex_pb::keep_credential_alive(expires_at, move || {
        let backend = backend.clone();
        async move {
            let mut config = Config::load().unwrap_or_default();
            Ok(account::fetch_relay_credential(&mut config, &backend)
                .await?
                .expires_at)
        }
    }))
}

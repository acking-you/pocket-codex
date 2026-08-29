//! Hidden foreground worker entrypoints spawned by high-level commands.

use anyhow::Result;
use pocket_codex_core::config::Config;
use pocket_codex_pb::{
    register as pb_register, subscribe as pb_subscribe, RegisterOptions, SubscribeOptions,
};

use crate::{cli::WorkerCmd, commands::api_proxy};

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
            tokio::signal::ctrl_c().await?;
            connection.stop().await?;
        },
        WorkerCmd::ApiProxy(args) => api_proxy::run(args.listen, args.proxy).await?,
    }
    Ok(())
}

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
            // The parent always passes `--relay`, so config is only consulted
            // for parity with other commands; load it best-effort so a broken
            // config.toml can't fail a worker that never needs it.
            let config = Config::load().unwrap_or_default();
            let relay =
                crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
            pb_register(RegisterOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: relay,
                codec: args.codec,
            })
            .await;
        },
        WorkerCmd::PbSubscribe(args) => {
            // The parent always passes `--relay`, so config is only consulted
            // for parity with other commands; load it best-effort so a broken
            // config.toml can't fail a worker that never needs it.
            let config = Config::load().unwrap_or_default();
            let relay =
                crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
            pb_subscribe(SubscribeOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: relay,
            })
            .await;
        },
        WorkerCmd::ApiProxy(args) => api_proxy::run(args.listen, args.proxy).await?,
    }
    Ok(())
}

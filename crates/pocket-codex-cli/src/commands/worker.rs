//! Hidden foreground worker entrypoints spawned by high-level commands.

use anyhow::Result;
use pocket_codex_pb::{
    register as pb_register, subscribe as pb_subscribe, RegisterOptions, SubscribeOptions,
};

use crate::{cli::WorkerCmd, commands::api_proxy};

/// Run an internal worker command.
pub async fn run(cmd: WorkerCmd) -> Result<()> {
    match cmd {
        WorkerCmd::PbRegister(args) => {
            pb_register(RegisterOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: args.relay.relay,
                codec: args.codec,
            })
            .await;
        },
        WorkerCmd::PbSubscribe(args) => {
            pb_subscribe(SubscribeOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: args.relay.relay,
            })
            .await;
        },
        WorkerCmd::ApiProxy(args) => api_proxy::run(args.listen, args.proxy).await?,
    }
    Ok(())
}

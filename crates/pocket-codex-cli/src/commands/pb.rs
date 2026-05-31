//! `pocket-codex pb …` subcommand handlers.
//!
//! These are blocking foreground commands by design: starting a relay
//! session is not *daemonisable* in a portable way (the upstream
//! `pb-mapper` helpers run forever), so we let `pocket-codex pb
//! register/subscribe` keep running in the foreground until the user
//! ctrl-C's it. Daemonisation is the user's choice (`tmux`, `nohup`,
//! `systemd-run`, `launchd`).

use std::net::SocketAddr;

use anyhow::{Context, Result};
use pocket_codex_pb::{
    register as pb_register, status as pb_status, subscribe as pb_subscribe, RegisterOptions,
    StatusKind, SubscribeOptions,
};
use tokio::net::lookup_host;

use crate::{
    cli::{PbCmd, PbRegisterArgs, PbStatusArgs, PbStatusKind, PbSubscribeArgs},
    commands::ui,
};

/// Dispatch the `pb` subcommand group.
pub async fn run(cmd: PbCmd) -> Result<()> {
    match cmd {
        PbCmd::Register(args) => register(args).await,
        PbCmd::Subscribe(args) => subscribe(args).await,
        PbCmd::Status(args) => status(args).await,
    }
}

async fn register(args: PbRegisterArgs) -> Result<()> {
    let opts = RegisterOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
        relay_addr: args.relay.relay.clone(),
        codec: args.codec,
    };
    ui::headline(ui::Tone::Action, "pb register");
    ui::field("local", &opts.local_addr);
    ui::field("key", &opts.key);
    ui::field("relay", &opts.relay_addr);
    ui::field("codec", &opts.codec.to_string());
    ui::muted("press Ctrl-C to stop");
    pb_register(opts).await;
    Ok(())
}

async fn subscribe(args: PbSubscribeArgs) -> Result<()> {
    let opts = SubscribeOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
        relay_addr: args.relay.relay.clone(),
    };
    ui::headline(ui::Tone::Action, "pb subscribe");
    ui::field("key", &opts.key);
    ui::field("relay", &opts.relay_addr);
    ui::field("local", &opts.local_addr);
    ui::muted("press Ctrl-C to stop");
    pb_subscribe(opts).await;
    Ok(())
}

async fn status(args: PbStatusArgs) -> Result<()> {
    let kind = match args.kind {
        PbStatusKind::Keys => StatusKind::Keys,
        PbStatusKind::RemoteId => StatusKind::RemoteId,
    };
    let addr = resolve_one(&args.relay.relay).await?;
    pb_status(addr, kind).await;
    Ok(())
}

async fn resolve_one(addr: &str) -> Result<SocketAddr> {
    let mut iter = lookup_host(addr)
        .await
        .with_context(|| format!("resolving relay address `{addr}`"))?;
    iter.next()
        .with_context(|| format!("relay address `{addr}` resolved to no entries"))
}

//! `pocket-codex pb …` subcommand handlers.
//!
//! `register` and `subscribe` are blocking foreground commands by design:
//! daemonising a relay session is not portable, so they hold the tunnel open
//! until the user ctrl-C's it and leave daemonisation to the user's tool of
//! choice (`tmux`, `nohup`, `systemd-run`, `launchd`).

use anyhow::Result;
use pocket_codex_core::config::Config;
use pocket_codex_pb::{
    keys as pb_keys, register as pb_register, subscribe as pb_subscribe, RegisterOptions,
    SubscribeOptions,
};

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
    let config = Config::load()?;
    let session = crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
    let opts = RegisterOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
        codec: args.codec,
    };
    ui::headline(ui::Tone::Action, "pb register");
    ui::field("local", &opts.local_addr);
    ui::field("key", &opts.key);
    ui::field("relay", &session.relay_addr);
    ui::field("codec", &opts.codec.to_string());
    // Resolves once the relay has ACCEPTED the registration, so a refusal is
    // reported here rather than retried silently — and "registered" below is a
    // statement about the relay's state, not just about the request being sent.
    let registration = pb_register(&session, opts).await?;
    ui::muted("registered — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    registration.stop().await?;
    Ok(())
}

async fn subscribe(args: PbSubscribeArgs) -> Result<()> {
    let config = Config::load()?;
    let session = crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
    let opts = SubscribeOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
    };
    ui::headline(ui::Tone::Action, "pb subscribe");
    ui::field("key", &opts.key);
    ui::field("relay", &session.relay_addr);
    ui::field("local", &opts.local_addr);
    // Resolves once the local listener is bound AND the relay confirmed the
    // service, so the address printed below is dialable the moment it appears.
    let connection = pb_subscribe(&session, opts).await?;
    ui::muted("listening — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    connection.stop().await?;
    Ok(())
}

/// Print what the relay knows about this credential's services.
///
/// This used to hand off to upstream's own status printer, which wrote its
/// formatting straight to stdout and had a `remote-id` mode. The SDK exposes
/// the data instead of a rendering, so both kinds print through our own `ui`
/// and look like the rest of the CLI.
async fn status(args: PbStatusArgs) -> Result<()> {
    let config = Config::load()?;
    let session = crate::commands::relay::resolve_session(args.relay.relay.as_deref(), &config)?;
    match args.kind {
        PbStatusKind::Keys => {
            let keys = pb_keys(&session).await?;
            ui::headline(ui::Tone::Ok, "relay services");
            ui::field("relay", &session.relay_addr);
            if keys.is_empty() {
                ui::muted("no services registered for this credential");
            } else {
                for key in keys {
                    ui::field("key", &key);
                }
            }
        },
        PbStatusKind::RemoteId => {
            let remote = pocket_codex_pb::remote_id(&session).await?;
            ui::headline(ui::Tone::Ok, "relay connection");
            ui::field("relay", &session.relay_addr);
            ui::field("remote id", &remote);
        },
    }
    Ok(())
}

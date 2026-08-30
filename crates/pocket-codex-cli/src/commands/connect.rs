//! `pocket-codex connect` high-level client-side orchestration.
//!
//! ```text
//!                       pocket-codex connect …
//!                                  │
//!                                  ▼
//!                       resolve_transport → { session, namespace? }
//!                                  │
//!                                  ▼
//!                       TargetRequest { key?, device?, name }
//!                                  │
//!                  ┌── key/device given? ──┐
//!                  │                        │ no  ── any local default? ─┐
//!                  │ yes                                                  │
//!                  │                                                       no
//!                  ▼                                                       ▼
//!           skip discovery                              discover_services(transport)
//!                  │                                                       │
//!                  └─────────────┬─────────────────────────────────────────┘
//!                                ▼
//!                  choose_target(App, request, config, state, discovered)
//!                                │
//!                                ▼
//!                  managed_pb::ensure(PbWorkerSpec {
//!                    role: PbRole::Subscribe, key, local_addr,
//!                    session, codec: false,
//!                  })
//!                                │
//!                                ▼
//!                  state.record_selected_service(App, device, name)
//!                                │
//!                                ▼
//!                  print "codex remote: codex --remote ws://<local_addr>"
//! ```
//!
//! The discovery guard avoids an unnecessary relay round-trip when the
//! user has already pinned a default through `services default set`
//! or implicitly via a successful prior `connect`.
//!
//! Account and self-host mode run the SAME flow: both subscribe straight to the
//! relay, differing only in the credential and the key's namespace, which
//! [`Transport`] already carries.

use anyhow::Result;
use pocket_codex_core::{
    config::Config,
    service::ServiceKind,
    state::{PbRole, RuntimeState},
};

use crate::{
    cli::ConnectArgs,
    commands::{
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
        transport::{self, Transport},
        ui,
    },
};

/// Run the client-side setup flow.
pub async fn run(args: ConnectArgs) -> Result<()> {
    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;
    connect(
        TargetRequest {
            key: args.key,
            device: args.device,
            name: args.name,
        },
        args.local_addr,
        &config,
        &transport,
        ServiceKind::App,
    )
    .await
}

/// Subscribe to a remote service of `kind` and expose it on `local_addr`.
///
/// Shared with `pocket-codex api connect`, which differs only in the kind it
/// asks for and the summary that gets printed.
pub(crate) async fn connect(
    request: TargetRequest,
    local_addr: String,
    config: &Config,
    transport: &Transport,
    kind: ServiceKind,
) -> Result<()> {
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let state = RuntimeState::load()?;
    let has_local_default =
        config.default_service(kind).is_some() || state.selected_service(kind).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(transport, kind).await?
    } else {
        Vec::new()
    };
    let target = choose_target(kind, request, config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        // An explicit `--key` is taken verbatim; anything derived from a
        // device/name goes through the transport so an account's keys land in its
        // own namespace.
        key: match target.service_id.as_ref() {
            Some(service) => transport.key(service),
            None => target.key.clone(),
        },
        local_addr,
        session: transport.session.clone(),
        codec: false,
    })
    .await?;
    if let Some(service_id) = target.service_id {
        let mut state = RuntimeState::load()?;
        state.record_selected_service(kind, service_id.device, service_id.name);
        state.save()?;
    }
    print_connect_summary(&outcome, kind);
    Ok(())
}

fn print_connect_summary(outcome: &EnsureOutcome, kind: ServiceKind) {
    let session = outcome.render("pb subscribe");
    match kind {
        ServiceKind::Api => {
            ui::headline(ui::Tone::Action, "codex provider config");
            ui::muted("    paste into ~/.codex/config.toml:");
            println!("{}", crate::commands::api::codex_provider_config(&session.local_addr));
        },
        _ => {
            ui::headline(ui::Tone::Action, "codex remote");
            ui::code(&codex_remote_command(&session.local_addr));
        },
    }
}

pub(crate) fn remote_ws_url(local_addr: &str) -> String {
    format!("ws://{local_addr}")
}

pub(crate) fn codex_remote_command(local_addr: &str) -> String {
    format!("codex --remote {}", remote_ws_url(local_addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_remote_command_uses_local_subscriber_listener() {
        assert_eq!(codex_remote_command("127.0.0.1:28080"), "codex --remote ws://127.0.0.1:28080");
    }
}

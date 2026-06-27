//! `pocket-codex connect` high-level client-side orchestration.
//!
//! ```text
//!                       pocket-codex connect …
//!                                  │
//!                                  ▼
//!                       TargetRequest { key?, device?, name }
//!                                  │
//!                  ┌── key/device given? ──┐
//!                  │                        │ no  ── any local default? ─┐
//!                  │ yes                                                  │
//!                  │                                                       no
//!                  ▼                                                       ▼
//!           skip discovery                                  discover_services(relay)
//!                  │                                                       │
//!                  └─────────────┬─────────────────────────────────────────┘
//!                                ▼
//!                  choose_target(App, request, config, state, discovered)
//!                                │
//!                                ▼
//!                  managed_pb::ensure(PbWorkerSpec {
//!                    role: PbRole::Subscribe, key, local_addr,
//!                    relay_addr, codec: false,
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

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use pocket_codex_broker_client::{run_subscribe, Connector, SubscribeConfig, TokenProvider};
use pocket_codex_core::{
    config::Config,
    service::ServiceKind,
    state::{PbRole, RuntimeState},
};
use tokio::net::TcpListener;

use crate::{
    cli::ConnectArgs,
    commands::{
        account,
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
        transport::{self, Transport},
        ui,
    },
};

/// Idle timeout applied to account-mode data bridges.
const ACCOUNT_DATA_IDLE: Duration = Duration::from_secs(1800);

/// Run the client-side setup flow.
pub async fn run(args: ConnectArgs) -> Result<()> {
    let config = Config::load()?;
    match transport::resolve_transport(args.relay.relay.as_deref(), None, &config)? {
        Transport::SelfHost { relay } => connect_self_host(args, &config, relay).await,
        Transport::Account { backend } => connect_account(args, backend).await,
    }
}

async fn connect_self_host(args: ConnectArgs, config: &Config, relay: String) -> Result<()> {
    let request = TargetRequest {
        key: args.key,
        device: args.device,
        name: args.name,
    };
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::App).is_some()
        || state.selected_service(ServiceKind::App).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::App, request, config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: target.key,
        local_addr: args.local_addr,
        relay_addr: relay,
        codec: false,
    })?;
    if let Some(service_id) = target.service_id {
        let mut state = RuntimeState::load()?;
        state.record_selected_service(ServiceKind::App, service_id.device, service_id.name);
        state.save()?;
    }
    print_connect_summary(&outcome);
    Ok(())
}

/// Account-mode client side: subscribe to a relay-exposed app-server through the
/// backend broker, exposing it on a local listener. Runs in the foreground.
async fn connect_account(args: ConnectArgs, backend: String) -> Result<()> {
    let mut config = Config::load()?;
    let (device, name) = account::resolve_target(
        &mut config,
        &backend,
        ServiceKind::App,
        args.device.as_deref(),
        args.name.as_deref(),
    )
    .await?;

    let (host, port) = account::broker_endpoint(&backend)?;
    let connector: Arc<dyn Connector> = Arc::new(account::BrokerTlsConnector::new(host, port)?);
    let tokens: Arc<dyn TokenProvider> =
        Arc::new(account::ConfigTokenProvider::new(backend.clone()));
    let listener = TcpListener::bind(&args.local_addr)
        .await
        .with_context(|| format!("binding local subscriber listener {}", args.local_addr))?;

    ui::headline(ui::Tone::Ok, "account connect");
    ui::field("service", &format!("{device}/app/{name}"));
    ui::field("local", &args.local_addr);
    ui::headline(ui::Tone::Action, "codex remote");
    ui::code(&codex_remote_command(&args.local_addr));
    ui::headline(ui::Tone::Action, "keep this running, Ctrl-C to stop");

    run_subscribe(
        connector,
        tokens,
        SubscribeConfig {
            device,
            kind: ServiceKind::App,
            name,
            idle: ACCOUNT_DATA_IDLE,
        },
        listener,
    )
    .await;
    Ok(())
}


fn print_connect_summary(outcome: &EnsureOutcome) {
    let session = outcome.render("pb subscribe");
    ui::headline(ui::Tone::Action, "codex remote");
    ui::code(&codex_remote_command(&session.local_addr));
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

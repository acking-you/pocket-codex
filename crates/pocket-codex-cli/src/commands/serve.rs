//! `pocket-codex serve` high-level host-side orchestration.
//!
//! ```text
//!                       pocket-codex serve …
//!                                │
//!                                ▼
//!                ServiceId::new(device, App, name).key()
//!                          (or args.key)
//!                                │
//!                                ▼
//!              pocket_codex_codex::spawn(SpawnOptions)
//!                                │
//!                                ▼
//!              websocket_listen_addr("ws://host:port")
//!                                │
//!                                ▼
//!              managed_pb::ensure(PbWorkerSpec {
//!                role:  PbRole::Register,
//!                key,
//!                local_addr,
//!                relay_addr,
//!                codec,
//!              })
//!                                │
//!                                ▼
//!              print_serve_summary + "client setup: pocket-codex
//!              connect --key <key> --relay <relay>"
//! ```
//!
//! `serve` is the host side of an app-server pairing: it owns the
//! `codex app-server` child and the pb-mapper register worker that
//! exposes its WebSocket. Non-WebSocket listen URLs (for example unix
//! sockets) are rejected because pb-mapper needs a relayable TCP
//! endpoint.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use pocket_codex_broker_client::{run_register, Connector, RegisterConfig, TokenProvider};
use pocket_codex_codex::{spawn, ListenSpec, SpawnOptions};
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, ServiceId, ServiceKind},
    state::PbRole,
};

use crate::{
    cli::ServeArgs,
    commands::{
        account, api_proxy,
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        transport::{self, Transport},
        ui,
    },
};

/// Idle timeout applied to account-mode data bridges.
const ACCOUNT_DATA_IDLE: Duration = Duration::from_secs(1800);

/// Run the host-side one-shot setup flow.
pub async fn run(args: ServeArgs) -> Result<()> {
    // Resolve the effective upstream proxy once (explicit flag or env). The
    // spawned app-server reads proxy settings only from its environment, never
    // from codex's config.toml, so we inject it there via SpawnOptions. Only an
    // explicit `--proxy` is validated eagerly (see resolve_app_server_proxy).
    let proxy_requested = args.proxy.is_some();
    let effective_proxy = api_proxy::resolve_app_server_proxy(args.proxy.as_deref())?;

    let config = Config::load()?;
    let transport = transport::resolve_transport(args.relay.relay.as_deref(), None, &config)?;

    let device = args.device.clone().unwrap_or_else(default_device_id);
    let name = args.name.clone();
    let codec = args.codec;
    let explicit_key = args.key.clone();

    let requested_listen = ListenSpec::WebSocket {
        host: args.host,
        port: args.port,
    };
    let report = spawn(SpawnOptions {
        binary: args.codex_binary,
        listen: requested_listen,
        extra_args: args.extra,
        log_file: None,
        proxy: effective_proxy.clone(),
    })?;
    let local_addr = websocket_listen_addr(&report.info.listen).with_context(|| {
        format!("codex listen URL `{}` is not relayable TCP", report.info.listen)
    })?;

    match transport {
        Transport::SelfHost { relay } => {
            let key = explicit_key
                .unwrap_or_else(|| ServiceId::new(&device, ServiceKind::App, &name).key());
            let outcome = managed_pb::ensure(PbWorkerSpec {
                role: PbRole::Register,
                key: key.clone(),
                local_addr,
                relay_addr: relay.clone(),
                codec,
            })?;
            print_serve_summary(
                &report.info,
                &outcome,
                &key,
                &relay,
                effective_proxy.as_deref(),
                proxy_requested,
                report.reused,
            );
            Ok(())
        }
        Transport::Account { backend } => {
            ui::headline(ui::Tone::Ok, "codex app-server");
            ui::field("pid", &report.info.pid.to_string());
            ui::field("listen", &report.info.listen);
            ui::field("log", &report.info.log_file.display().to_string());
            api_proxy::print_proxy_status(
                effective_proxy.as_deref(),
                proxy_requested,
                report.reused,
                api_proxy::SpawnCommand::Serve,
            );
            if explicit_key.is_some() {
                ui::warn("--key is ignored in account mode; the service is namespaced to your account");
            }
            serve_account(&backend, &device, &name, local_addr).await
        }
    }
}

/// Account-mode host side: register the local app-server through the backend
/// broker. Runs in the foreground (holds the control tunnel) until interrupted.
async fn serve_account(backend: &str, device: &str, name: &str, local_addr: String) -> Result<()> {
    let (host, port) = account::broker_endpoint(backend)?;
    let connector: Arc<dyn Connector> = Arc::new(account::BrokerTlsConnector::new(host, port)?);
    let tokens: Arc<dyn TokenProvider> =
        Arc::new(account::ConfigTokenProvider::new(backend.to_string()));
    let local: SocketAddr = local_addr
        .parse()
        .with_context(|| format!("codex listen addr `{local_addr}` is not a socket address"))?;

    ui::headline(ui::Tone::Ok, "account register");
    ui::field("backend", backend);
    ui::field("service", &format!("{device}/app/{name}"));
    ui::headline(ui::Tone::Action, "exposing — keep this running, Ctrl-C to stop");

    run_register(
        connector,
        tokens,
        RegisterConfig {
            device: device.to_string(),
            kind: ServiceKind::App,
            name: name.to_string(),
            client_instance_id: account::client_instance_id(),
            local_addr: local,
            idle: ACCOUNT_DATA_IDLE,
        },
    )
    .await;
    Ok(())
}

fn print_serve_summary(
    codex: &pocket_codex_core::state::CodexProcessInfo,
    pb: &EnsureOutcome,
    key: &str,
    relay: &str,
    effective_proxy: Option<&str>,
    proxy_requested: bool,
    reused: bool,
) {
    ui::headline(ui::Tone::Ok, "codex app-server");
    ui::field("pid", &codex.pid.to_string());
    ui::field("listen", &codex.listen);
    ui::field("log", &codex.log_file.display().to_string());
    api_proxy::print_proxy_status(
        effective_proxy,
        proxy_requested,
        reused,
        api_proxy::SpawnCommand::Serve,
    );
    pb.render("pb register");
    ui::headline(ui::Tone::Action, "client setup");
    ui::code(&format!("pocket-codex connect --key {key} --relay {relay}"));
}

fn websocket_listen_addr(listen: &str) -> Option<String> {
    listen
        .strip_prefix("ws://")
        .filter(|addr| !addr.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_listen_addr_extracts_tcp_addr() {
        assert_eq!(
            websocket_listen_addr("ws://127.0.0.1:18080").as_deref(),
            Some("127.0.0.1:18080")
        );
        assert_eq!(websocket_listen_addr("unix:///tmp/codex.sock"), None);
    }
}

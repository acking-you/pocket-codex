//! `pocket-codex api …` subcommand handlers.
//!
//! ```text
//!                       pocket-codex api …
//!                     ┌─────────┴─────────┐
//!                   serve               connect
//!                     │                   │
//!         ┌───────────┴────┐    ┌─────────┴──────────────┐
//!         ▼                ▼    ▼                        ▼
//!   managed_api      managed_pb  discover_services   managed_pb
//!   ::ensure         ::ensure    + choose_target     ::ensure
//!   (api-proxy)      (Register)  (priority cascade)  (Subscribe)
//!         │                │              │                │
//!         └ ApiProxyInfo   └ PbSessionInfo  ResolvedTarget │
//!                                                          ▼
//!                                                  record_selected
//!                                                  _service +
//!                                                  codex_provider_config
//! ```
//!
//! `serve` wires the local Responses API proxy onto a relay key; the
//! matching `connect` resolves a target service and prints the
//! `[model_providers.pocket-codex-api]` snippet a remote `codex` should
//! use.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use pocket_codex_broker_client::{
    run_register, run_subscribe, Connector, RegisterConfig, SubscribeConfig, TokenProvider,
};
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, ServiceId, ServiceKind},
    state::{PbRole, RuntimeState},
};
use tokio::net::TcpListener;

use crate::{
    cli::{ApiCmd, ApiConnectArgs, ApiServeArgs},
    commands::{
        account, api_proxy,
        managed_api::{self, ApiWorkerSpec, EnsureOutcome as ApiEnsureOutcome},
        managed_pb::{self, EnsureOutcome as PbEnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
        transport::{self, Transport},
        ui,
    },
};

/// Idle timeout applied to account-mode data bridges.
const ACCOUNT_DATA_IDLE: Duration = Duration::from_secs(1800);

/// Dispatch the `api` subcommand group.
pub async fn run(cmd: ApiCmd) -> Result<()> {
    match cmd {
        ApiCmd::Serve(args) => serve(args).await,
        ApiCmd::Connect(args) => connect(args).await,
    }
}

async fn serve(args: ApiServeArgs) -> Result<()> {
    let device = args.device.clone().unwrap_or_else(default_device_id);

    // Resolve the effective upstream proxy once (explicit flag or env) so we
    // can fail fast on a bad scheme, surface it to the user, and record a
    // signature that lets a rerun with a changed proxy restart the worker.
    let effective_proxy = api_proxy::resolve_proxy(args.proxy.as_deref());
    if let Some(raw) = effective_proxy.as_deref() {
        api_proxy::validate_proxy(raw)?;
    }
    let proxy_signature = effective_proxy.as_deref().map(api_proxy::redact_proxy);
    let local_addr = format!("{}:{}", args.host, args.port);

    let config = Config::load()?;
    let transport = transport::resolve_transport(args.relay.relay.as_deref(), None, &config)?;

    let key = args
        .key
        .clone()
        .unwrap_or_else(|| ServiceId::new(&device, ServiceKind::Api, &args.name).key());
    let api_outcome = managed_api::ensure(ApiWorkerSpec {
        key: key.clone(),
        local_addr: local_addr.clone(),
        proxy: args.proxy.clone(),
        proxy_signature,
    })?;

    match transport {
        Transport::SelfHost {
            relay,
        } => {
            let pb_outcome = managed_pb::ensure(PbWorkerSpec {
                role: PbRole::Register,
                key: key.clone(),
                local_addr,
                relay_addr: relay.clone(),
                codec: args.codec,
            })?;
            print_serve_summary(
                &api_outcome,
                &pb_outcome,
                &key,
                &relay,
                effective_proxy.as_deref(),
            );
            Ok(())
        },
        Transport::Account {
            backend,
        } => {
            api_outcome.render();
            print_proxy_status(effective_proxy.as_deref());
            serve_account(&backend, &device, &args.name, local_addr).await
        },
    }
}

/// Account-mode host side: register the local Responses API proxy through the
/// backend broker (foreground; holds the control tunnel until interrupted).
async fn serve_account(backend: &str, device: &str, name: &str, local_addr: String) -> Result<()> {
    let (host, port) = account::broker_endpoint(backend)?;
    let connector: Arc<dyn Connector> = Arc::new(account::BrokerTlsConnector::new(host, port)?);
    let tokens: Arc<dyn TokenProvider> =
        Arc::new(account::ConfigTokenProvider::new(backend.to_string()));
    let local: SocketAddr = local_addr
        .parse()
        .with_context(|| format!("api proxy addr `{local_addr}` is not a socket address"))?;

    ui::headline(ui::Tone::Ok, "account register");
    ui::field("backend", backend);
    ui::field("service", &format!("{device}/api/{name}"));
    ui::headline(ui::Tone::Action, "exposing — keep this running, Ctrl-C to stop");

    run_register(connector, tokens, RegisterConfig {
        device: device.to_string(),
        kind: ServiceKind::Api,
        name: name.to_string(),
        client_instance_id: account::client_instance_id(),
        local_addr: local,
        idle: ACCOUNT_DATA_IDLE,
    })
    .await;
    Ok(())
}

async fn connect(args: ApiConnectArgs) -> Result<()> {
    let config = Config::load()?;
    match transport::resolve_transport(args.relay.relay.as_deref(), None, &config)? {
        Transport::SelfHost {
            relay,
        } => connect_self_host(args, &config, relay).await,
        Transport::Account {
            backend,
        } => connect_account(args, backend).await,
    }
}

async fn connect_self_host(args: ApiConnectArgs, config: &Config, relay: String) -> Result<()> {
    let request = TargetRequest {
        key: args.key,
        device: args.device,
        name: args.name,
    };
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::Api).is_some()
        || state.selected_service(ServiceKind::Api).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::Api, request, config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: target.key,
        local_addr: args.local_addr,
        relay_addr: relay,
        codec: false,
    })?;
    if let Some(service_id) = target.service_id {
        let mut state = RuntimeState::load()?;
        state.record_selected_service(ServiceKind::Api, service_id.device, service_id.name);
        state.save()?;
    }
    print_connect_summary(&outcome);
    Ok(())
}

/// Account-mode client side: subscribe to a relay-exposed API proxy through the
/// backend broker, expose it locally, and print the codex provider config.
async fn connect_account(args: ApiConnectArgs, backend: String) -> Result<()> {
    let mut config = Config::load()?;
    let (device, name) = account::resolve_target(
        &mut config,
        &backend,
        ServiceKind::Api,
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
        .with_context(|| format!("binding local API listener {}", args.local_addr))?;

    ui::headline(ui::Tone::Ok, "account connect (api)");
    ui::field("service", &format!("{device}/api/{name}"));
    ui::headline(ui::Tone::Action, "codex provider config");
    ui::muted("    paste into ~/.codex/config.toml:");
    println!("{}", codex_provider_config(&args.local_addr));
    ui::headline(ui::Tone::Action, "keep this running, Ctrl-C to stop");

    run_subscribe(
        connector,
        tokens,
        SubscribeConfig {
            device,
            kind: ServiceKind::Api,
            name,
            idle: ACCOUNT_DATA_IDLE,
        },
        listener,
    )
    .await;
    Ok(())
}

fn print_proxy_status(effective: Option<&str>) {
    match effective {
        Some(raw) => ui::field("proxy", &api_proxy::redact_proxy(raw)),
        None => ui::warn(
            "no upstream proxy configured. The API proxy reaches chatgpt.com directly and will \
             fail on networks that block it. Pass `--proxy http://host:port` (or \
             `socks5://host:port`), or export HTTPS_PROXY / ALL_PROXY / HTTP_PROXY before running \
             `pocket-codex api serve`.",
        ),
    }
}

fn print_serve_summary(
    api: &ApiEnsureOutcome,
    pb: &PbEnsureOutcome,
    key: &str,
    relay: &str,
    effective_proxy: Option<&str>,
) {
    api.render();
    print_proxy_status(effective_proxy);
    pb.render("pb register");
    ui::headline(ui::Tone::Action, "client setup");
    ui::code(&format!("pocket-codex api connect --key {key} --relay {relay}"));
}

fn print_connect_summary(outcome: &PbEnsureOutcome) {
    let session = outcome.render("pb subscribe");
    ui::headline(ui::Tone::Action, "codex provider config");
    ui::muted("    paste into ~/.codex/config.toml:");
    println!("{}", codex_provider_config(&session.local_addr));
}

pub(crate) fn codex_provider_config(local_addr: &str) -> String {
    format!(
        r#"model_provider = "pocket-codex-api"

[model_providers.pocket-codex-api]
name = "Pocket-Codex API"
base_url = "http://{local_addr}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = true"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_config_points_codex_at_local_responses_base_url() {
        let config = codex_provider_config("127.0.0.1:28180");

        assert!(config.contains(r#"base_url = "http://127.0.0.1:28180/v1""#));
        assert!(config.contains("supports_websockets = true"));
        assert!(config.contains("requires_openai_auth = false"));
    }
}

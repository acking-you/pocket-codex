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

use anyhow::Result;
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, ServiceId, ServiceKind},
    state::PbRole,
};

use crate::{
    cli::{ApiCmd, ApiConnectArgs, ApiServeArgs},
    commands::{
        api_proxy, connect,
        managed_api::{self, ApiWorkerSpec, EnsureOutcome as ApiEnsureOutcome},
        managed_pb::{self, EnsureOutcome as PbEnsureOutcome, PbWorkerSpec},
        service_target::TargetRequest,
        transport::{self, Transport},
        ui,
    },
};

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
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;

    // An explicit `--key` is honoured only in self-host mode: in account mode the
    // relay confines the credential to the account's namespace, so a hand-written
    // key outside it would simply be refused.
    if args.key.is_some() && transport.is_account() {
        ui::warn("--key is ignored in account mode; the service is namespaced to your account");
    }
    let service = ServiceId::new(&device, ServiceKind::Api, &args.name);
    let key = match args.key.clone().filter(|_| !transport.is_account()) {
        Some(key) => key,
        None => transport.key(&service),
    };
    let api_outcome = managed_api::ensure(ApiWorkerSpec {
        key: key.clone(),
        local_addr: local_addr.clone(),
        proxy: args.proxy.clone(),
        proxy_signature,
    })?;
    let pb_outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: key.clone(),
        local_addr,
        session: transport.session.clone(),
        codec: args.codec,
    })
    .await?;
    print_serve_summary(&api_outcome, &pb_outcome, &key, &transport, effective_proxy.as_deref());
    Ok(())
}

async fn connect(args: ApiConnectArgs) -> Result<()> {
    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;
    // Same flow as `pocket-codex connect`, only the kind differs — an API proxy
    // and an app-server are both just a TCP service on the relay.
    connect::connect(
        TargetRequest {
            key: args.key,
            device: args.device,
            name: args.name,
        },
        args.local_addr,
        &config,
        &transport,
        ServiceKind::Api,
    )
    .await
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
    transport: &Transport,
    effective_proxy: Option<&str>,
) {
    api.render();
    print_proxy_status(effective_proxy);
    pb.render("pb register");
    ui::headline(ui::Tone::Action, "client setup");
    ui::code(&client_setup_command("api connect", key, transport));
}

/// The command to print for the other device. In account mode the peer resolves
/// the key from its own session, so telling it `--key`/`--relay` would be
/// wrong: the key is namespaced to the account and the relay is not the peer's
/// to name.
pub(crate) fn client_setup_command(subcommand: &str, key: &str, transport: &Transport) -> String {
    if transport.is_account() {
        format!("pocket-codex {subcommand}")
    } else {
        format!("pocket-codex {subcommand} --key {key} --relay {}", transport.session.relay_addr)
    }
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

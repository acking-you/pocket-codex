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
    state::{PbRole, RuntimeState},
};

use crate::{
    cli::{ApiCmd, ApiConnectArgs, ApiServeArgs},
    commands::{
        api_proxy,
        managed_api::{self, ApiWorkerSpec, EnsureOutcome as ApiEnsureOutcome},
        managed_pb::{self, EnsureOutcome as PbEnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
        ui,
    },
};

/// Dispatch the `api` subcommand group.
pub async fn run(cmd: ApiCmd) -> Result<()> {
    match cmd {
        ApiCmd::Serve(args) => serve(args),
        ApiCmd::Connect(args) => connect(args).await,
    }
}

fn serve(args: ApiServeArgs) -> Result<()> {
    let service_id = ServiceId::new(
        args.device.clone().unwrap_or_else(default_device_id),
        ServiceKind::Api,
        &args.name,
    );
    let key = args.key.clone().unwrap_or_else(|| service_id.key());
    let local_addr = format!("{}:{}", args.host, args.port);

    // Resolve the effective upstream proxy once (explicit flag or env) so we
    // can fail fast on a bad scheme, surface it to the user, and record a
    // signature that lets a rerun with a changed proxy restart the worker.
    let effective_proxy = api_proxy::resolve_proxy(args.proxy.as_deref());
    if let Some(raw) = effective_proxy.as_deref() {
        api_proxy::validate_proxy(raw)?;
    }
    let proxy_signature = effective_proxy.as_deref().map(api_proxy::redact_proxy);

    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;

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
        relay_addr: relay.clone(),
        codec: args.codec,
    })?;
    print_serve_summary(&api_outcome, &pb_outcome, &key, &relay, effective_proxy.as_deref());
    Ok(())
}

async fn connect(args: ApiConnectArgs) -> Result<()> {
    let request = TargetRequest {
        key: args.key,
        device: args.device,
        name: args.name,
    };
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::Api).is_some()
        || state.selected_service(ServiceKind::Api).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::Api, request, &config, &state, &discovered)?;
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

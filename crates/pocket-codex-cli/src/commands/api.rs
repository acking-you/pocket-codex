//! `pocket-codex api …` subcommand handlers.

use anyhow::Result;
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, ServiceId, ServiceKind},
    state::{PbRole, RuntimeState},
};

use crate::{
    cli::{ApiCmd, ApiConnectArgs, ApiServeArgs},
    commands::{
        managed_api::{self, ApiWorkerSpec, EnsureOutcome as ApiEnsureOutcome},
        managed_pb::{self, EnsureOutcome as PbEnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
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
    let api_outcome = managed_api::ensure(ApiWorkerSpec {
        key: key.clone(),
        local_addr: local_addr.clone(),
    })?;
    let pb_outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: key.clone(),
        local_addr,
        relay_addr: args.relay.relay.clone(),
        codec: args.codec,
    })?;
    print_serve_summary(&api_outcome, &pb_outcome, &key, &args.relay.relay);
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
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::Api).is_some()
        || state.selected_service(ServiceKind::Api).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&args.relay.relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::Api, request, &config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: target.key,
        local_addr: args.local_addr,
        relay_addr: args.relay.relay,
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

fn print_serve_summary(api: &ApiEnsureOutcome, pb: &PbEnsureOutcome, key: &str, relay: &str) {
    match api {
        ApiEnsureOutcome::Reused(session) => println!(
            "api proxy reused: pid={} listen={} log={}",
            session.pid,
            session.local_addr,
            session.log_file.display()
        ),
        ApiEnsureOutcome::Replaced {
            stale_pid,
            session,
        } => println!(
            "api proxy replaced stale pid {} with pid={} listen={} log={}",
            stale_pid,
            session.pid,
            session.local_addr,
            session.log_file.display()
        ),
        ApiEnsureOutcome::Spawned(session) => println!(
            "api proxy started: pid={} listen={} log={}",
            session.pid,
            session.local_addr,
            session.log_file.display()
        ),
    }
    match pb {
        PbEnsureOutcome::Reused(session) => println!(
            "pb register reused: pid={} key={} relay={} log={}",
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
        PbEnsureOutcome::Replaced {
            stale_pid,
            session,
        } => println!(
            "pb register replaced stale pid {} with pid={} key={} relay={} log={}",
            stale_pid,
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
        PbEnsureOutcome::Spawned(session) => println!(
            "pb register started: pid={} key={} relay={} log={}",
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
    }
    println!("client setup: pocket-codex api connect --key {key} --relay {relay}");
}

fn print_connect_summary(outcome: &PbEnsureOutcome) {
    let session = match outcome {
        PbEnsureOutcome::Reused(session) => {
            println!(
                "pb subscribe reused: pid={} key={} relay={} log={}",
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
        PbEnsureOutcome::Replaced {
            stale_pid,
            session,
        } => {
            println!(
                "pb subscribe replaced stale pid {} with pid={} key={} relay={} log={}",
                stale_pid,
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
        PbEnsureOutcome::Spawned(session) => {
            println!(
                "pb subscribe started: pid={} key={} relay={} log={}",
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
    };
    println!("{}", codex_provider_config(&session.local_addr));
}

pub(crate) fn codex_provider_config(local_addr: &str) -> String {
    format!(
        r#"Codex config:
model_provider = "pocket-codex-api"

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

//! CLI lifecycle for an attached OpenCode service.

use std::{env, net::SocketAddr};

use anyhow::{Context, Result};
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, sanitize_component, ServiceId, ServiceKind},
    state::{PbRole, RuntimeState},
};
use pocket_codex_host_svc::opencode::{BasicCredentials, Connection, OpenCodeGateway};

use crate::{
    cli::{OpenCodeCmd, OpenCodeConnectArgs, OpenCodeServeArgs, OpenCodeStopArgs},
    commands::{
        managed_pb::{self, PbWorkerSpec, StopFilter, StopOutcome},
        service_target::{choose_target, discover_services, TargetRequest},
        transport, ui,
    },
};

/// Dispatch OpenCode attached-hosting commands.
pub async fn run(command: OpenCodeCmd) -> Result<()> {
    match command {
        OpenCodeCmd::Serve(args) => serve(args).await,
        OpenCodeCmd::Connect(args) => connect(args).await,
        OpenCodeCmd::Status => status(),
        OpenCodeCmd::Stop(args) => stop(args),
    }
}

async fn serve(args: OpenCodeServeArgs) -> Result<()> {
    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;
    let password = match args.password_env.as_deref() {
        Some(name) => Some(env::var(name).with_context(|| {
            format!("reading OpenCode password from environment variable `{name}`")
        })?),
        None => None,
    };
    let credentials = password.map(|value| BasicCredentials::new(args.username, value));
    let client = Connection::connect(&args.url, &args.directory, credentials)
        .await
        .map_err(|error| anyhow::anyhow!("OpenCode compatibility check failed: {error}"))?;
    let address: SocketAddr = args
        .local_addr
        .parse()
        .context("invalid OpenCode gateway address")?;
    let gateway = OpenCodeGateway::new(client);
    let (listener, router) = gateway
        .bind(address)
        .await
        .context("binding OpenCode gateway")?;
    let local_addr = listener.local_addr()?.to_string();
    let device = args.device.unwrap_or_else(default_device_id);
    let key = transport.key(&ServiceId::new(device, ServiceKind::OpenCode, &args.name));
    let registration =
        pocket_codex_pb::register(&transport.session, pocket_codex_pb::RegisterOptions {
            key: key.clone(),
            local_addr: local_addr.clone(),
            codec: args.codec,
        })
        .await?;
    ui::headline(ui::Tone::Ok, "OpenCode attached");
    ui::field("endpoint", &format!("http://{local_addr}"));
    ui::field("key", &key);
    ui::muted(
        "Ctrl-C stops Pocket-Codex hosting only; the existing OpenCode server remains running.",
    );
    let serve_result = axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let stop_result = registration.stop().await;
    serve_result?;
    stop_result?;
    Ok(())
}

async fn connect(args: OpenCodeConnectArgs) -> Result<()> {
    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;
    let state = RuntimeState::load()?;
    let discovered = if args.key.is_none()
        && args.device.is_none()
        && config.default_service(ServiceKind::OpenCode).is_none()
        && state.selected_service(ServiceKind::OpenCode).is_none()
    {
        discover_services(&transport, ServiceKind::OpenCode).await?
    } else {
        Vec::new()
    };
    let target = choose_target(
        ServiceKind::OpenCode,
        TargetRequest {
            key: args.key,
            device: args.device,
            name: args.name,
        },
        &config,
        &state,
        &discovered,
    )?;
    let key = target
        .service_id
        .as_ref()
        .map(|service| transport.key(service))
        .unwrap_or(target.key);
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: key.clone(),
        local_addr: args.local_addr,
        session: transport.session,
        codec: false,
    })
    .await?;
    if let Some(service) = target.service_id {
        let mut state = RuntimeState::load()?;
        state.record_selected_service(ServiceKind::OpenCode, service.device, service.name);
        state.save()?;
    }
    let session = outcome.render("OpenCode subscribe");
    ui::headline(ui::Tone::Action, "OpenCode endpoint");
    ui::field("url", &format!("http://{}", session.local_addr));
    Ok(())
}

fn status() -> Result<()> {
    let state = RuntimeState::load()?;
    let rows: Vec<_> = state
        .pb
        .iter()
        .filter(|session| session.key.contains(":opencode:"))
        .collect();
    if rows.is_empty() {
        ui::muted("OpenCode: no managed hosting or connections");
        return Ok(());
    }
    for session in rows {
        ui::field(session.role.as_str(), &format!("{} -> {}", session.key, session.local_addr));
    }
    Ok(())
}

fn stop(args: OpenCodeStopArgs) -> Result<()> {
    let state = RuntimeState::load()?;
    let suffix = args
        .name
        .map(|name| format!(":opencode:{}", sanitize_component(&name)));
    let keys: Vec<String> = state
        .pb
        .iter()
        .filter(|session| {
            session.key.contains(":opencode:")
                && suffix
                    .as_ref()
                    .is_none_or(|suffix| session.key.ends_with(suffix))
        })
        .map(|session| session.key.clone())
        .collect();
    for key in keys {
        for outcome in managed_pb::stop_matching(StopFilter {
            role: None,
            key: Some(key),
        })? {
            match outcome {
                StopOutcome::Stopped(_) => {
                    ui::headline(ui::Tone::Ok, "OpenCode Pocket-Codex hosting stopped")
                },
                StopOutcome::Stale(_) => {
                    ui::headline(ui::Tone::Muted, "OpenCode stale hosting record cleared")
                },
            }
        }
    }
    ui::muted("The existing OpenCode server was not stopped.");
    Ok(())
}

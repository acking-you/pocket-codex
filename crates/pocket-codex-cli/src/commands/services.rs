//! `pocket-codex services …` subcommand handlers.

use anyhow::{anyhow, Result};
use comfy_table::Cell;
use pocket_codex_core::{config::Config, service::ServiceKind};

use crate::{
    cli::{ServicesCmd, ServicesDefaultCmd, ServicesDefaultSetArgs, ServicesListArgs},
    commands::{
        account,
        service_target::discover_services,
        transport::{self, Transport},
        ui,
    },
};

/// Dispatch the `services` subcommand group.
pub async fn run(cmd: ServicesCmd) -> Result<()> {
    match cmd {
        ServicesCmd::List(args) => list(args).await,
        ServicesCmd::Default(ServicesDefaultCmd::Set(args)) => default_set(args),
    }
}

async fn list(args: ServicesListArgs) -> Result<()> {
    let kind = args.kind.map(ServiceKind::from);
    let mut config = Config::load()?;
    match transport::resolve_transport(args.relay.relay.as_deref(), None, &config)? {
        Transport::SelfHost {
            relay,
        } => list_self_host(&relay, kind).await,
        Transport::Account {
            backend,
        } => list_account(&mut config, &backend, kind).await,
    }
}

async fn list_self_host(relay: &str, kind: Option<ServiceKind>) -> Result<()> {
    let mut services = discover_services(relay).await?;
    services.retain(|id| kind.is_none_or(|kind| id.kind == kind));
    services.sort_by_key(|id| id.key());

    if services.is_empty() {
        ui::muted("no Pocket-Codex services found");
        return Ok(());
    }

    let mut table = ui::new_table(&["KEY", "DEVICE", "KIND", "NAME"]);
    for service in services {
        table.add_row(vec![
            Cell::new(service.key()),
            Cell::new(&service.device),
            ui::kind_cell(service.kind.as_key_segment(), service.kind),
            Cell::new(&service.name),
        ]);
    }
    println!("{table}");
    Ok(())
}

async fn list_account(config: &mut Config, backend: &str, kind: Option<ServiceKind>) -> Result<()> {
    let mut services = account::fetch_services(config, backend).await?;
    services.retain(|s| kind.is_none_or(|kind| s.kind == kind));
    services.sort_by(|a, b| {
        (a.device.as_str(), a.name.as_str()).cmp(&(b.device.as_str(), b.name.as_str()))
    });

    if services.is_empty() {
        ui::muted("no services in your account");
        return Ok(());
    }

    let mut table = ui::new_table(&["DEVICE", "KIND", "NAME"]);
    for service in services {
        table.add_row(vec![
            Cell::new(&service.device),
            ui::kind_cell(service.kind.as_key_segment(), service.kind),
            Cell::new(&service.name),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn default_set(args: ServicesDefaultSetArgs) -> Result<()> {
    let kind = ServiceKind::from(args.kind);
    let mut config = Config::load()?;
    config.set_default_service(kind, &args.device, &args.name);
    config.save()?;
    let target = config
        .default_service(kind)
        .ok_or_else(|| anyhow!("default target missing after setting {kind} service"))?;
    ui::headline(ui::Tone::Ok, &format!("default {kind} service"));
    ui::field("target", &format!("pcx:{}:{}:{}", target.device, kind, target.name));
    Ok(())
}

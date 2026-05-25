//! `pocket-codex services …` subcommand handlers.

use anyhow::{anyhow, Result};
use pocket_codex_core::{config::Config, service::ServiceKind};

use crate::{
    cli::{ServicesCmd, ServicesDefaultCmd, ServicesDefaultSetArgs, ServicesListArgs},
    commands::service_target::discover_services,
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
    let mut services = discover_services(&args.relay.relay).await?;
    services.retain(|id| kind.is_none_or(|kind| id.kind == kind));
    services.sort_by_key(|id| id.key());

    if services.is_empty() {
        println!("no Pocket-Codex services found");
    } else {
        for service in services {
            println!(
                "{} device={} kind={} name={}",
                service.key(),
                service.device,
                service.kind,
                service.name
            );
        }
    }
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
    println!("default {kind} service: pcx:{}:{}:{}", target.device, kind, target.name);
    Ok(())
}

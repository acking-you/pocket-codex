//! Subscribe (consumer) side: one data tunnel per local connection.

use std::{sync::Arc, time::Duration};

use pocket_codex_account_proto::{
    bridge::bridge,
    broker::{BrokerHello, BrokerRole, TunnelPurpose},
};
use pocket_codex_core::service::ServiceKind;
use tokio::net::{TcpListener, TcpStream};

use crate::{conn, BrokerError, Connector, TokenProvider};

/// What service a subscribe session consumes.
#[derive(Clone)]
pub struct SubscribeConfig {
    /// Device id segment.
    pub device: String,
    /// Service kind.
    pub kind: ServiceKind,
    /// Instance name segment.
    pub name: String,
    /// Idle timeout applied to each data bridge.
    pub idle: Duration,
}

/// Accept local connections on `listener`; for each, open a subscribe data
/// tunnel to the backend and bridge it. Intended to be `tokio::spawn`ed.
pub async fn run_subscribe(
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    cfg: SubscribeConfig,
    listener: TcpListener,
) {
    let cfg = Arc::new(cfg);
    loop {
        let (local, _) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "subscribe accept failed");
                continue;
            }
        };
        let connector = connector.clone();
        let tokens = tokens.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            if let Err(e) = one(connector, tokens, cfg, local).await {
                tracing::warn!(error = %e, "subscribe tunnel failed");
            }
        });
    }
}

async fn one(
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    cfg: Arc<SubscribeConfig>,
    local: TcpStream,
) -> Result<(), BrokerError> {
    let token = tokens.token().await?;
    let hello = BrokerHello {
        token,
        role: BrokerRole::Subscribe,
        purpose: TunnelPurpose::SubscribeData,
        device: cfg.device.clone(),
        kind: cfg.kind,
        name: cfg.name.clone(),
        client_instance_id: None,
        generation: None,
        stream_id: None,
    };
    let tunnel = conn::open_tunnel(connector.as_ref(), &hello).await?;
    bridge(tunnel, local, cfg.idle).await?;
    Ok(())
}

//! Relay service discovery: resolve `host:port` and list `pcx:*` keys.
use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};
use pocket_codex_core::service::ServiceId;
use tokio::net::lookup_host;

/// One discovered Pocket-Codex service on the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredService {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
    /// Full `pcx:<device>:<kind>:<name>` key.
    pub key: String,
}

/// Resolve a `host:port` relay string to one `SocketAddr`.
pub async fn resolve_relay(relay: &str) -> Result<SocketAddr> {
    lookup_host(relay)
        .await
        .with_context(|| format!("resolving relay `{relay}`"))?
        .next()
        .ok_or_else(|| anyhow!("relay `{relay}` resolved to no addresses"))
}

/// List Pocket-Codex services registered on the relay (bare keys filtered).
pub async fn discover(relay: &str) -> Result<Vec<DiscoveredService>> {
    let addr = resolve_relay(relay).await?;
    let keys = pocket_codex_pb::keys(addr)
        .await
        .context("querying relay keys")?;
    Ok(keys
        .into_iter()
        .filter_map(|k| {
            ServiceId::parse_key(&k).map(|id| DiscoveredService {
                device: id.device,
                kind: id.kind.as_key_segment().to_string(),
                name: id.name,
                key: k,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_relay_rejects_garbage() {
        // No port → resolution fails fast, not a hang.
        assert!(resolve_relay("not-a-host-without-port").await.is_err());
    }
}

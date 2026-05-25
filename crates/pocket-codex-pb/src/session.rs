//! Thin async wrappers around the upstream `pb-mapper` library.
//!
//! The upstream API takes generic `LocalStream`/`LocalListener` type
//! parameters and `ToSocketAddrs` for both sides; we lock those down to
//! TCP and `String` addresses because that's the only combination
//! Pocket-Codex actually needs (codex app-server speaks WebSocket over
//! TCP, never UDP). Should we ever need UDP we can extend this module
//! with a parallel set of helpers.

use std::{net::SocketAddr, sync::Arc};

use anyhow::{anyhow, Context, Result};
use pb_mapper::{
    common::{
        config::StatusOp,
        message::command::{PbConnStatusReq, PbConnStatusResp, PbServiceConnStatus},
    },
    local::{
        client::{handle_status_cli, run_client_side_cli, status::get_status},
        server::run_server_side_cli,
    },
};
use tokio::net::TcpStream;
use uni_stream::stream::{TcpListenerProvider, TcpStreamProvider};

/// Options for registering a local TCP service with a remote relay.
///
/// Mirrors `pb-mapper-server-cli tcp-server`.
#[derive(Debug, Clone)]
pub struct RegisterOptions {
    /// Service key under which the relay should index the registration.
    pub key: String,
    /// `host:port` of the local service to expose.
    pub local_addr: String,
    /// `host:port` of the upstream relay (`pb-mapper-server`).
    pub relay_addr: String,
    /// Enable AES-256-GCM end-to-end encryption (matches `--codec`).
    pub codec: bool,
}

/// Options for subscribing to a remote service from a client device.
///
/// Mirrors `pb-mapper-client-cli tcp-server`.
#[derive(Debug, Clone)]
pub struct SubscribeOptions {
    /// Service key the client wants to attach to.
    pub key: String,
    /// `host:port` of the local listener the client should expose.
    pub local_addr: String,
    /// `host:port` of the upstream relay.
    pub relay_addr: String,
}

/// Register a local TCP service with the relay.
///
/// This future runs forever (or until the upstream pb-mapper session
/// fails) and is intended to be `tokio::spawn`-ed by the caller.
pub async fn register(opts: RegisterOptions) {
    let local: Arc<str> = Arc::from(opts.local_addr.as_str());
    let remote: Arc<str> = Arc::from(opts.relay_addr.as_str());
    run_server_side_cli::<TcpStreamProvider, _>(
        local.as_ref(),
        remote.as_ref(),
        Arc::from(opts.key.as_str()),
        opts.codec,
        false, // is_datagram
    )
    .await;
}

/// Subscribe to a remote service and expose it on a local TCP port.
///
/// This future runs forever (or until the upstream pb-mapper session
/// fails) and is intended to be `tokio::spawn`-ed by the caller.
pub async fn subscribe(opts: SubscribeOptions) {
    let local: Arc<str> = Arc::from(opts.local_addr.as_str());
    let remote: Arc<str> = Arc::from(opts.relay_addr.as_str());
    run_client_side_cli::<TcpListenerProvider, _>(
        local.as_ref(),
        remote.as_ref(),
        Arc::from(opts.key.as_str()),
    )
    .await;
}

/// What kind of relay status query to issue.
#[derive(Debug, Clone, Copy)]
pub enum StatusKind {
    /// List active subscription / connection ids.
    RemoteId,
    /// List registered service keys.
    Keys,
}

/// Pretty-print the relay's view of registered services / clients to
/// stdout. Mirrors `pb-mapper-{server,client}-cli status …`.
///
/// `relay_addr` is taken as a pre-resolved [`SocketAddr`] because the
/// upstream helper requires `Copy + Send + 'static` address types.
pub async fn status(relay_addr: SocketAddr, kind: StatusKind) {
    let op = match kind {
        StatusKind::RemoteId => StatusOp::RemoteId,
        StatusKind::Keys => StatusOp::Keys,
    };
    handle_status_cli(op, relay_addr).await;
}

/// Query registered service keys from a relay.
pub async fn keys(relay_addr: SocketAddr) -> Result<Vec<String>> {
    match query_status(relay_addr, PbConnStatusReq::Keys).await? {
        PbConnStatusResp::Keys(keys) => Ok(keys),
        other => Err(anyhow!("relay returned unexpected keys status: {other:?}")),
    }
}

/// Query connection health for one registered service key.
pub async fn service_connections(
    relay_addr: SocketAddr,
    key: impl Into<String>,
) -> Result<Vec<PbServiceConnStatus>> {
    let key = key.into();
    match query_status(relay_addr, PbConnStatusReq::Service {
        key: key.clone(),
    })
    .await?
    {
        PbConnStatusResp::Service {
            connections, ..
        } => Ok(connections),
        other => Err(anyhow!("relay returned unexpected service status for `{key}`: {other:?}")),
    }
}

async fn query_status(relay_addr: SocketAddr, req: PbConnStatusReq) -> Result<PbConnStatusResp> {
    let mut stream = TcpStream::connect(relay_addr)
        .await
        .with_context(|| format!("connecting to pb-mapper relay {relay_addr}"))?;
    get_status(&mut stream, req)
        .await
        .map_err(|err| anyhow!("querying pb-mapper relay status: {err}"))
}

//! Meta service of an ACP host (TRD §4.2.11): the provider-neutral routes
//! plus `/history/v1` backed by the hub transcript.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use tokio::net::TcpListener;

use super::{
    history::{AcpHistorySource, AcpSessionDirs},
    hub::AcpHub,
};
use crate::{
    generic_app, history_sync,
    store::{ConfigStore, HostStore},
};

/// Serve the ACP meta service on an already-bound loopback `listener`.
pub async fn serve_meta(
    listener: TcpListener,
    store: Arc<ConfigStore>,
    host: Arc<HostStore>,
    uploads_dir: PathBuf,
    hub: Arc<AcpHub>,
) -> anyhow::Result<()> {
    let local: SocketAddr = listener.local_addr()?;
    if !local.ip().is_loopback() {
        anyhow::bail!("the ACP meta service only listens on loopback");
    }
    let app = generic_app(store, host, uploads_dir, Arc::new(AcpSessionDirs(hub.clone())))
        .merge(history_sync::router(Arc::new(AcpHistorySource::new(hub))));
    axum::serve(listener, app)
        .await
        .context("running ACP meta service")
}

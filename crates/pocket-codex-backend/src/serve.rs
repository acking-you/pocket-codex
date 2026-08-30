//! The accept loop: wrap each connection in the configured TLS layer and hand
//! it to the HTTP API (hyper).

use std::time::Duration;

use axum::Router;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
};

use crate::tls::TlsKind;

/// A wrapped (TLS or plain) connection ready to serve.
trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}

/// How long a TLS handshake may take before the connection is abandoned. Each
/// accepted connection is its own task, so an unbounded handshake lets a client
/// that opens a socket but never completes (or slowly drips) the handshake pin
/// a task + file descriptor indefinitely — a slowloris-style
/// resource-exhaustion vector on a public listener. Bound it.
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Wrap one accepted TCP connection, or `None` if it was an ACME challenge or
/// the handshake failed/stalled.
async fn accept_tls(tls: &TlsKind, tcp: TcpStream) -> Option<Box<dyn Stream>> {
    match tls {
        TlsKind::Plain => Some(Box::new(tcp)),
        TlsKind::Static(acceptor) => {
            match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
                Ok(Ok(stream)) => Some(Box::new(stream)),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "tls handshake failed");
                    None
                },
                Err(_) => {
                    tracing::warn!("tls handshake timed out");
                    None
                },
            }
        },
    }
}

/// Serve the HTTP API over the TLS layer, forever.
pub async fn serve_http(listener: TcpListener, router: Router, tls: TlsKind) {
    loop {
        let (tcp, _) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "http accept failed");
                continue;
            },
        };
        let tls = tls.clone();
        let router = router.clone();
        tokio::spawn(async move {
            if let Some(stream) = accept_tls(&tls, tcp).await {
                let io = TokioIo::new(stream);
                let service = TowerToHyperService::new(router);
                if let Err(e) = Builder::new(TokioExecutor::new())
                    .serve_connection_with_upgrades(io, service)
                    .await
                {
                    tracing::debug!(error = %e, "http connection ended");
                }
            }
        });
    }
}

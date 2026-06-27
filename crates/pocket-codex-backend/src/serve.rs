//! Accept loops that wrap each connection in the configured TLS layer and feed
//! it to the HTTP API (hyper) or the broker.

use axum::Router;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use pocket_codex_broker_server::BrokerServer;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
};

use crate::tls::TlsKind;

/// A wrapped (TLS or plain) connection ready to serve.
trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}

/// Wrap one accepted TCP connection, or `None` if it was an ACME challenge or
/// the handshake failed.
async fn accept_tls(tls: &TlsKind, tcp: TcpStream) -> Option<Box<dyn Stream>> {
    match tls {
        TlsKind::Plain => Some(Box::new(tcp)),
        TlsKind::Static(acceptor) => match acceptor.accept(tcp).await {
            Ok(stream) => Some(Box::new(stream)),
            Err(e) => {
                tracing::warn!(error = %e, "tls handshake failed");
                None
            },
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

/// Serve the broker over the TLS layer, forever.
pub async fn serve_broker(listener: TcpListener, broker: BrokerServer, tls: TlsKind) {
    loop {
        let (tcp, _) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "broker accept failed");
                continue;
            },
        };
        let tls = tls.clone();
        let broker = broker.clone();
        tokio::spawn(async move {
            if let Some(stream) = accept_tls(&tls, tcp).await {
                broker.handle_connection(stream).await;
            }
        });
    }
}

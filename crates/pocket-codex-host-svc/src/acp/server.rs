//! WebSocket transport of the hub (TRD §4.2.6): `GET /acp` carries one
//! JSON-RPC message per text frame between a controller and a
//! [`HubConnection`](super::HubConnection); `GET /healthz` answers 200.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use pocket_codex_core::acp::rpc;
use tokio::net::TcpListener;
use tracing::debug;

use super::hub::AcpHub;

/// Frame and message size limit, the same as `AppClient`.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
/// Close code used when a controller does not keep up ("try again later").
pub const CLOSE_TRY_AGAIN_LATER: u16 = 1013;

/// Serve the hub on an already-bound loopback `listener` until dropped.
pub async fn serve_ws(listener: TcpListener, hub: Arc<AcpHub>) -> anyhow::Result<()> {
    let local: SocketAddr = listener.local_addr()?;
    if !local.ip().is_loopback() {
        anyhow::bail!("the ACP hub only listens on loopback");
    }
    let app = Router::new()
        .route("/acp", get(upgrade))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .with_state(hub);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn upgrade(State(hub): State<Arc<AcpHub>>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(hub, socket))
}

async fn connection(hub: Arc<AcpHub>, socket: WebSocket) {
    let (conn, mut outbound) = hub.open_connection();
    let closed = conn.closed();
    let (mut sink, mut stream) = socket.split();
    let reader_conn = conn.clone();
    let mut reader = tokio::spawn(async move {
        while let Some(Ok(message)) = stream.next().await {
            let bytes = match message {
                Message::Text(text) => text.as_bytes().to_vec(),
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => continue,
            };
            match rpc::decode(&bytes) {
                Ok(message) => reader_conn.handle(message),
                Err(e) => debug!("ignoring an invalid controller frame: {}", e.message),
            }
        }
    });
    let mut writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                message = outbound.recv() => match message {
                    Some(message) => {
                        let text = rpc::encode(&message);
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            return;
                        }
                    },
                    None => return,
                },
                _ = closed.cancelled() => {
                    let frame = CloseFrame {
                        code: CLOSE_TRY_AGAIN_LATER,
                        reason: "controller is not keeping up".into(),
                    };
                    let _ = sink.send(Message::Close(Some(frame))).await;
                    return;
                },
            }
        }
    });
    tokio::select! {
        _ = &mut reader => writer.abort(),
        _ = &mut writer => reader.abort(),
    }
    conn.close();
}

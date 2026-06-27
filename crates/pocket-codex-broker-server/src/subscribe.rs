//! Subscribe (consumer) handling: one pb-mapper subscribe per data tunnel,
//! bridged through a loopback listener.

use std::{net::SocketAddr, time::Duration};

use pocket_codex_account_proto::{bridge::bridge, broker::BrokerAck, frame::write_frame, params};
use pocket_codex_pb::{subscribe, SubscribeOptions};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, Instant},
};
use tokio_util::sync::CancellationToken;

use crate::{BrokerServer, BrokerServerError, Result, ServerStream};

impl BrokerServer {
    /// Serve one subscribe data tunnel: stand up a pb-mapper subscribe on a
    /// loopback listener, dial it, and bridge it to the client tunnel.
    pub(crate) async fn handle_subscribe_data(
        &self,
        mut data: Box<dyn ServerStream>,
        relay_key: String,
    ) -> Result<()> {
        // Reserve a free loopback port, then let pb-mapper rebind it. (pb-mapper
        // owns the listener, so we connect with a short readiness retry.)
        let probe = TcpListener::bind("127.0.0.1:0").await?;
        let sub_addr = probe.local_addr()?;
        drop(probe);

        let cancel = CancellationToken::new();
        let pb = tokio::spawn({
            let relay_addr = self.inner.relay_addr.clone();
            let key = relay_key.clone();
            let local_addr = sub_addr.to_string();
            let cancel = cancel.clone();
            async move {
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = subscribe(SubscribeOptions { key, local_addr, relay_addr }) => {}
                }
            }
        });

        let local = match connect_with_retry(sub_addr, params::STREAM_DIAL_TIMEOUT).await {
            Ok(local) => local,
            Err(e) => {
                let _ =
                    write_frame(&mut data, &BrokerAck::err("relay subscribe unavailable")).await;
                cancel.cancel();
                pb.abort();
                return Err(e);
            },
        };

        write_frame(&mut data, &BrokerAck::ok(relay_key)).await?;
        let result = bridge(data, local, self.inner.data_idle).await;
        cancel.cancel();
        pb.abort();
        result.map_err(BrokerServerError::from)
    }
}

/// Connect to `addr`, retrying briefly while pb-mapper's listener comes up.
async fn connect_with_retry(addr: SocketAddr, budget: Duration) -> Result<TcpStream> {
    let deadline = Instant::now() + budget;
    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                if Instant::now() >= deadline {
                    return Err(e.into());
                }
                sleep(Duration::from_millis(25)).await;
            },
        }
    }
}

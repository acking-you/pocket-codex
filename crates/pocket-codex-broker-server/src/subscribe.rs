//! Subscribe (consumer) handling: one pb-mapper subscribe per data tunnel,
//! bridged through a loopback listener.

use pocket_codex_account_proto::{bridge::bridge, broker::BrokerAck, frame::write_frame};
use pocket_codex_pb::{subscribe, SubscribeOptions};
use tokio::net::{TcpListener, TcpStream};

use crate::{BrokerServer, BrokerServerError, Result, ServerStream};

impl BrokerServer {
    /// Serve one subscribe data tunnel: stand up a pb-mapper subscribe on a
    /// loopback listener, dial it, and bridge it to the client tunnel.
    pub(crate) async fn handle_subscribe_data(
        &self,
        mut data: Box<dyn ServerStream>,
        relay_key: String,
    ) -> Result<()> {
        // Reserve a free loopback port, then let pb-mapper rebind it. Racy in
        // principle — another process could take the port in between — but it is
        // how you ask the OS for an unused port, and the alternative is guessing.
        let probe = TcpListener::bind("127.0.0.1:0").await?;
        let sub_addr = probe.local_addr()?;
        drop(probe);

        // The SDK resolves this once the local listener is bound AND the relay
        // has confirmed the service, so the dial below cannot lose a race with
        // pb-mapper's own bind. This used to be a spawn plus a 25 ms poll loop
        // against a 10 s budget, because the old API offered no readiness signal.
        let connection = match subscribe(
            &self.relay_session(),
            SubscribeOptions {
                key: relay_key.clone(),
                local_addr: sub_addr.to_string(),
            },
        )
        .await
        {
            Ok(connection) => connection,
            Err(e) => {
                let _ =
                    write_frame(&mut data, &BrokerAck::err("relay subscribe unavailable")).await;
                return Err(BrokerServerError::Relay(format!("{e:#}")));
            },
        };

        let local = match TcpStream::connect(sub_addr).await {
            Ok(local) => local,
            Err(e) => {
                let _ =
                    write_frame(&mut data, &BrokerAck::err("relay subscribe unavailable")).await;
                // Stop rather than drop: dropping aborts the tunnel worker
                // mid-teardown, which leaves the relay holding the registration
                // until its own lease sweep notices.
                let _ = connection.stop().await;
                return Err(e.into());
            },
        };

        write_frame(&mut data, &BrokerAck::ok(relay_key)).await?;
        let result = bridge(data, local, self.inner.data_idle).await;
        let _ = connection.stop().await;
        result.map_err(BrokerServerError::from)
    }
}

//! Shared tunnel-handshake helpers.

use pocket_codex_account_proto::{
    broker::{BrokerAck, BrokerHello},
    frame::{read_frame, write_frame},
    params,
};
use tokio::time::timeout;

use crate::{BrokerError, BrokerStream, Connector};

/// Write the `hello` and read the backend's `ack` on an already-connected
/// stream, returning the ack on success or a [`BrokerError::Rejected`] on a
/// negative ack.
pub(crate) async fn handshake<S: BrokerStream>(
    stream: &mut S,
    hello: &BrokerHello,
) -> Result<BrokerAck, BrokerError> {
    timeout(params::CONTROL_IO_TIMEOUT, write_frame(stream, hello))
        .await
        .map_err(|_| BrokerError::Timeout("write hello"))??;
    let ack: BrokerAck = timeout(params::CONTROL_IO_TIMEOUT, read_frame(stream))
        .await
        .map_err(|_| BrokerError::Timeout("read ack"))??;
    if ack.ok {
        Ok(ack)
    } else {
        Err(BrokerError::Rejected(ack.error.unwrap_or_else(|| "rejected".to_string())))
    }
}

/// Connect a fresh tunnel and complete its Hello/Ack handshake.
pub(crate) async fn open_tunnel(
    connector: &dyn Connector,
    hello: &BrokerHello,
) -> Result<Box<dyn BrokerStream>, BrokerError> {
    let mut stream = connector.connect().await?;
    handshake(&mut stream, hello).await?;
    Ok(stream)
}

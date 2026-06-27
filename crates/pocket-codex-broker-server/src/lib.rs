//! Server side of the Pocket-Codex broker tunnel.
//!
//! The backend accepts authenticated client tunnels (TLS in production, plain
//! TCP in tests) and bridges them to a **loopback** pb-mapper relay that holds
//! the real `MSG_HEADER_KEY`. Clients never see that key and can never name
//! another account's services: the backend derives the relay key from the
//! verified token as `pcxu:<user_id>:<device>:<kind>:<name>`.
//!
//! The pb-mapper work is reused unchanged via [`pocket_codex_pb`]; this crate
//! is the seam between pb-mapper's address-based `register`/`subscribe` and the
//! per-stream client tunnels:
//!
//! - **register** ([`TunnelPurpose::RegisterControl`]): bind a loopback seam
//!   listener and run [`pocket_codex_pb::register`] (real key) pointed at it.
//!   Each subscriber the relay sends shows up as an accept on the seam → the
//!   backend emits a [`BrokerControl::NewStream`] and rendezvous the answering
//!   [`TunnelPurpose::RegisterData`] tunnel with that accepted connection.
//! - **subscribe** ([`TunnelPurpose::SubscribeData`]): run
//!   [`pocket_codex_pb::subscribe`] on a loopback listener and dial it,
//!   bridging the result to the client tunnel.
//!
//! The process-global `MSG_HEADER_KEY` must be set (via
//! [`pocket_codex_pb::set_msg_header_key`]) before any connection is handled.

#![forbid(unsafe_code)]

mod register;
mod subscribe;

use std::{collections::HashMap, sync::Arc, time::Duration};

use pocket_codex_account_proto::{
    broker::{BrokerAck, BrokerHello, BrokerRole, TunnelPurpose},
    frame::{read_frame, write_frame},
    key::NamespacedServiceId,
    params,
};
use pocket_codex_core::service::ServiceId;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{oneshot, Mutex},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

/// A connected client tunnel: TLS in production, plain TCP in tests.
pub trait ServerStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> ServerStream for T {}

/// Verifies a client session token, yielding the internal user id to namespace
/// on. Kept synchronous: JWT verification needs no I/O on the hot path.
pub trait TokenVerifier: Send + Sync + 'static {
    /// Return the internal user id for a valid token, or `None` to reject.
    fn verify(&self, token: &str) -> Option<String>;
}

/// Errors from handling a broker connection.
#[derive(Debug, thiserror::Error)]
pub enum BrokerServerError {
    /// Stream I/O failed.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// Framing a handshake/control message failed.
    #[error("frame: {0}")]
    Frame(#[from] pocket_codex_account_proto::frame::FrameError),
    /// The session token was missing or invalid.
    #[error("unauthorized")]
    Unauthorized,
    /// The hello frame was malformed for its purpose.
    #[error("bad hello: {0}")]
    BadHello(&'static str),
    /// A data tunnel referenced a register session that does not exist.
    #[error("no such register session")]
    NoSession,
    /// A data tunnel referenced a superseded session generation.
    #[error("stale generation")]
    StaleGeneration,
    /// A data tunnel referenced an unknown/expired stream id.
    #[error("no pending stream")]
    NoPendingStream,
    /// A timed operation exceeded its deadline.
    #[error("timed out: {0}")]
    Timeout(&'static str),
}

type Result<T> = std::result::Result<T, BrokerServerError>;

/// One live register session, shared between its control task and the
/// out-of-band [`TunnelPurpose::RegisterData`] tunnels that rendezvous with it.
struct RegisterSession {
    /// Epoch assigned at install; bumped on every takeover so a stale data
    /// tunnel from a retired session is fenced off.
    generation: u64,
    /// Stable per-process id of the owning client (diagnostics / takeover).
    #[allow(dead_code, reason = "retained for diagnostics and future policy")]
    client_instance_id: String,
    /// Cancels the whole session (pb register task + seam accept + control
    /// loop).
    cancel: CancellationToken,
    /// Per-stream rendezvous: a seam accept inserts a slot, the answering data
    /// tunnel removes it and sends itself through.
    pending: Mutex<HashMap<u64, oneshot::Sender<Box<dyn ServerStream>>>>,
}

struct Inner {
    verifier: Arc<dyn TokenVerifier>,
    relay_addr: String,
    data_idle: Duration,
    registers: Mutex<HashMap<String, Arc<RegisterSession>>>,
}

/// The broker server. Cheap to [`Clone`] (an `Arc` handle) so the backend can
/// share it across its accept loop.
#[derive(Clone)]
pub struct BrokerServer {
    inner: Arc<Inner>,
}

impl BrokerServer {
    /// Build a broker server that bridges to the pb-mapper relay at
    /// `relay_addr` (expected loopback). `data_idle` bounds an idle data
    /// bridge.
    pub fn new(
        verifier: Arc<dyn TokenVerifier>,
        relay_addr: impl Into<String>,
        data_idle: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                verifier,
                relay_addr: relay_addr.into(),
                data_idle,
                registers: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Handle one accepted client tunnel to completion. Errors are logged, not
    /// returned, so this can be `tokio::spawn`ed directly per connection.
    pub async fn handle_connection<S: ServerStream>(&self, stream: S) {
        let stream: Box<dyn ServerStream> = Box::new(stream);
        if let Err(e) = self.dispatch(stream).await {
            tracing::warn!(error = %e, "broker connection ended with error");
        }
    }

    async fn dispatch(&self, mut stream: Box<dyn ServerStream>) -> Result<()> {
        let hello: BrokerHello = timeout(params::CONTROL_IO_TIMEOUT, read_frame(&mut stream))
            .await
            .map_err(|_| BrokerServerError::Timeout("read hello"))??;
        let Some(user_id) = self.inner.verifier.verify(&hello.token) else {
            let _ = write_frame(&mut stream, &BrokerAck::err("unauthorized")).await;
            return Err(BrokerServerError::Unauthorized);
        };
        let service = ServiceId::new(&hello.device, hello.kind, &hello.name);
        let relay_key = NamespacedServiceId::new(&user_id, service).key();
        match (hello.role, hello.purpose) {
            (BrokerRole::Register, TunnelPurpose::RegisterControl) => {
                self.handle_register_control(stream, hello, relay_key).await
            },
            (BrokerRole::Register, TunnelPurpose::RegisterData) => {
                self.handle_register_data(stream, hello, relay_key).await
            },
            (BrokerRole::Subscribe, TunnelPurpose::SubscribeData) => {
                self.handle_subscribe_data(stream, relay_key).await
            },
            _ => {
                let _ = write_frame(&mut stream, &BrokerAck::err("invalid role/purpose")).await;
                Err(BrokerServerError::BadHello("invalid role/purpose"))
            },
        }
    }

    /// Rendezvous a [`TunnelPurpose::RegisterData`] tunnel with the loopback
    /// accept that emitted its `NewStream`.
    async fn handle_register_data(
        &self,
        mut data: Box<dyn ServerStream>,
        hello: BrokerHello,
        relay_key: String,
    ) -> Result<()> {
        let generation = hello
            .generation
            .ok_or(BrokerServerError::BadHello("missing generation"))?;
        let stream_id = hello
            .stream_id
            .ok_or(BrokerServerError::BadHello("missing stream_id"))?;
        let session = self
            .inner
            .registers
            .lock()
            .await
            .get(&relay_key)
            .cloned()
            .ok_or(BrokerServerError::NoSession)?;
        if session.generation != generation {
            let _ = write_frame(&mut data, &BrokerAck::err("stale generation")).await;
            return Err(BrokerServerError::StaleGeneration);
        }
        let tx = session
            .pending
            .lock()
            .await
            .remove(&stream_id)
            .ok_or(BrokerServerError::NoPendingStream)?;
        write_frame(&mut data, &BrokerAck::ok(relay_key)).await?;
        // Past the ack the tunnel is raw; hand it to the waiting seam bridge.
        let _ = tx.send(data);
        Ok(())
    }
}

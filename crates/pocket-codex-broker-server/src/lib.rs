//! Server side of the Pocket-Codex broker tunnel.
//!
//! The backend accepts authenticated client tunnels (TLS in production, plain
//! TCP in tests) and bridges them to a **loopback** pb-mapper relay, presenting
//! the relay's administrator credential. Clients hold no relay credential at all
//! and can never name another account's services: the backend derives the relay
//! key from the verified token as `pcxu:<user_id>:<device>:<kind>:<name>`.
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
//! The relay credential is supplied to [`BrokerServer::new`] as part of a
//! [`pocket_codex_pb::RelaySession`]. It used to be process-global state a caller
//! had to install before the first connection; making it a constructor argument
//! means a broker cannot exist without one.

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
    /// A register control hello named a key that another live instance
    /// already owns (first-wins: the incumbent keeps the key; the newcomer is
    /// refused instead of evicting it — see `handle_register_control`).
    #[error("key conflict: {0}")]
    KeyConflict(String),
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
    /// The relay refused a register or subscribe.
    ///
    /// Distinct from [`Self::Io`] on purpose: the relay answering "no" — an
    /// over-quota service, a revoked credential — is a decision to surface, not
    /// a transport fault to retry through. Conflating the two is what let a
    /// permanent refusal drive an unbacked-off reconnect loop.
    #[error("relay refused: {0}")]
    Relay(String),
}

type Result<T> = std::result::Result<T, BrokerServerError>;

/// One live register session, shared between its control task and the
/// out-of-band [`TunnelPurpose::RegisterData`] tunnels that rendezvous with it.
struct RegisterSession {
    /// Epoch assigned at install; bumped on every takeover so a stale data
    /// tunnel from a retired session is fenced off.
    generation: u64,
    /// Stable per-process id of the owning client: a reconnecting hello with
    /// the same id takes over this session; any other id is refused
    /// (first-wins — see `handle_register_control`).
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
    /// How this process authenticates to the relay. Held here rather than set
    /// process-globally (which is what the pre-0.5 client required), so the
    /// credential is a property of this broker and never ambient state some
    /// other component could depend on having been installed.
    relay: pocket_codex_pb::RelaySession,
    data_idle: Duration,
    registers: Mutex<HashMap<String, Arc<RegisterSession>>>,
    /// Last time a key-conflict rejection was logged at WARN, per relay key —
    /// a stuck legacy client retrying in a loop must not flood the journal
    /// (the 2026-07-07 outage began as a register-storm log flood). Sync
    /// mutex: the critical section is a map lookup, no awaits.
    conflict_log: std::sync::Mutex<HashMap<String, std::time::Instant>>,
}

/// The broker server. Cheap to [`Clone`] (an `Arc` handle) so the backend can
/// share it across its accept loop.
#[derive(Clone)]
pub struct BrokerServer {
    inner: Arc<Inner>,
}

impl BrokerServer {
    /// Build a broker server that bridges to the pb-mapper relay `relay`
    /// addresses (expected loopback). `data_idle` bounds an idle data bridge.
    ///
    /// `relay` carries the credential as well as the address. The broker is the
    /// component that holds the relay's ADMINISTRATOR key: it registers and
    /// subscribes on behalf of clients that hold no relay credential at all,
    /// which is what keeps per-account isolation a property of the broker rather
    /// than of client good behaviour.
    pub fn new(
        verifier: Arc<dyn TokenVerifier>,
        relay: pocket_codex_pb::RelaySession,
        data_idle: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                verifier,
                relay,
                data_idle,
                registers: Mutex::new(HashMap::new()),
                conflict_log: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    /// This broker's relay session, for the register/subscribe legs.
    pub(crate) fn relay_session(&self) -> pocket_codex_pb::RelaySession {
        self.inner.relay.clone()
    }

    /// Force-deregister a relay key: cancel the live register session holding
    /// it (if any) and evict it. Cancelling tears down that session's
    /// loopback pb-register, so the relay drops the key at once. Returns
    /// `true` when a session was holding the key. Best-effort — a client
    /// whose `run_register` is still alive will reconnect and re-register
    /// shortly after.
    pub async fn deregister_key(&self, relay_key: &str) -> bool {
        let removed = self.inner.registers.lock().await.remove(relay_key);
        match removed {
            Some(session) => {
                session.cancel.cancel();
                true
            },
            None => false,
        }
    }

    /// Handle one accepted client tunnel to completion. Errors are logged, not
    /// returned, so this can be `tokio::spawn`ed directly per connection.
    pub async fn handle_connection<S: ServerStream>(&self, stream: S) {
        let stream: Box<dyn ServerStream> = Box::new(stream);
        if let Err(e) = self.dispatch(stream).await {
            // Key conflicts already emit their own rate-limited WARN at the
            // rejection site; a per-attempt WARN here would let one stuck
            // retry-looping client flood the journal.
            if matches!(e, BrokerServerError::KeyConflict(_)) {
                tracing::debug!(error = %e, "broker connection rejected");
            } else {
                tracing::warn!(error = %e, "broker connection ended with error");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct RejectAll;
    impl TokenVerifier for RejectAll {
        fn verify(&self, _token: &str) -> Option<String> {
            None
        }
    }

    /// A stream that never yields a read and errors every write — models a
    /// client that vanished (RST) between its register hello and the server's
    /// ok-ack, so the ack `write_frame` fails.
    struct FailWrite;
    impl tokio::io::AsyncRead for FailWrite {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Pending
        }
    }
    impl tokio::io::AsyncWrite for FailWrite {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "peer gone",
            )))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Regression: if the ok-ack write fails right after the session is
    /// installed, the teardown must still evict it. Otherwise the dead session
    /// lingers in the map (its lease never expires — it never entered the
    /// control loop) and, under first-wins, PERMANENTLY refuses the key.
    #[tokio::test]
    async fn ack_write_failure_does_not_leak_the_register_session() {
        let broker = BrokerServer::new(
            Arc::new(RejectAll),
            pocket_codex_pb::RelaySession::for_test("127.0.0.1:1"),
            Duration::from_secs(1),
        );
        let key = "pcxu:u:dev:app:default".to_string();
        let hello = BrokerHello {
            token: "t".to_string(),
            role: BrokerRole::Register,
            purpose: TunnelPurpose::RegisterControl,
            device: "dev".to_string(),
            kind: pocket_codex_core::service::ServiceKind::App,
            name: "default".to_string(),
            client_instance_id: Some("inst-one".to_string()),
            generation: None,
            stream_id: None,
        };
        let r = broker
            .handle_register_control(Box::new(FailWrite), hello, key.clone())
            .await;
        assert!(r.is_err(), "a failed ack write must surface as an error");
        assert!(
            broker.inner.registers.lock().await.get(&key).is_none(),
            "a failed ack write must NOT leave the session parked in the map"
        );
    }

    #[tokio::test]
    async fn deregister_key_cancels_and_evicts_the_session() {
        let broker = BrokerServer::new(
            Arc::new(RejectAll),
            pocket_codex_pb::RelaySession::for_test("127.0.0.1:1"),
            Duration::from_secs(1),
        );
        let cancel = CancellationToken::new();
        let key = "pcxu:u:dev:app:default";
        broker.inner.registers.lock().await.insert(
            key.to_string(),
            Arc::new(RegisterSession {
                generation: 0,
                client_instance_id: "test".to_string(),
                cancel: cancel.clone(),
                pending: Mutex::new(HashMap::new()),
            }),
        );

        assert!(!cancel.is_cancelled());
        // Deregistering cancels the session (tearing down its pb-register) and
        // evicts the map slot.
        assert!(broker.deregister_key(key).await);
        assert!(cancel.is_cancelled());
        assert!(broker.inner.registers.lock().await.get(key).is_none());
        // A second call (no live session) is a no-op.
        assert!(!broker.deregister_key(key).await);
    }
}

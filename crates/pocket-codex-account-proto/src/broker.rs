//! The broker tunnel handshake.
//!
//! A client opens a TLS connection to the backend's broker port, sends one
//! [`BrokerHello`] frame (a session token + the service it wants), and reads a
//! [`BrokerAck`]. On success the connection becomes a raw bidirectional tunnel
//! that the backend bridges to the relay; on failure the backend closes it.

use pocket_codex_core::service::ServiceKind;
use serde::{Deserialize, Serialize};

/// Which side of the relay the client wants the backend to take on its behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerRole {
    /// Publish a local service (the CLI exposing its codex app-server).
    Register,
    /// Consume a published service (the app/CLI operating an app-server).
    Subscribe,
}

/// What a broker tunnel is for. Sent inside [`BrokerHello`] so the backend
/// knows whether the tunnel stays a JSON control channel or goes raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelPurpose {
    /// Long-lived control tunnel for a register session: stays JSON-framed and
    /// carries [`BrokerControl`] frames both ways (the controller that pulls a
    /// fresh data tunnel for each new subscriber).
    RegisterControl,
    /// A per-stream data tunnel a publisher opens in answer to a
    /// [`BrokerControl::NewStream`]; goes raw after the ack.
    RegisterData,
    /// A subscribe-side data tunnel (one per consumer connection); goes raw
    /// after the ack.
    SubscribeData,
}

/// The first frame a client sends on any broker tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerHello {
    /// Backend session token (JWT). The backend derives the user namespace from
    /// it; the client never names another account's prefix.
    pub token: String,
    /// Register (publisher) or subscribe (consumer).
    pub role: BrokerRole,
    /// What this tunnel is for.
    pub purpose: TunnelPurpose,
    /// Device id segment of the target service.
    pub device: String,
    /// Service kind (app-server vs API proxy).
    pub kind: ServiceKind,
    /// Instance name segment of the target service.
    pub name: String,
    /// `RegisterControl` only: a stable per-process id (e.g. `pid-unixms`) so a
    /// fresh control session deterministically takes over (retires) a prior
    /// session for the same key, instead of racing it within the lease window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_instance_id: Option<String>,
    /// `RegisterData` only: the session generation of the answered
    /// [`BrokerControl::NewStream`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// `RegisterData` only: the stream id of the answered
    /// [`BrokerControl::NewStream`]; pairs this tunnel to its loopback accept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<u64>,
}

/// The backend's reply to a [`BrokerHello`], sent before the tunnel goes raw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerAck {
    /// Whether the handshake was authorized and the relay leg is up.
    pub ok: bool,
    /// The resolved `pcxu:<user>:<device>:<kind>:<name>` key (diagnostics
    /// only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_key: Option<String>,
    /// Human-readable reason when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BrokerAck {
    /// A success ack carrying the resolved relay key.
    pub fn ok(relay_key: impl Into<String>) -> Self {
        Self {
            ok: true,
            relay_key: Some(relay_key.into()),
            error: None,
        }
    }

    /// A failure ack carrying a reason.
    pub fn err(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            relay_key: None,
            error: Some(reason.into()),
        }
    }
}

/// Control frames exchanged on a [`TunnelPurpose::RegisterControl`] tunnel, in
/// both directions, length-prefixed JSON (see [`crate::frame`]).
///
/// Modeled one-to-one on pb-mapper's relay↔publisher control messages so the
/// broker inherits the same heartbeat / lease / generation discipline. The
/// `generation` is the per-register-session epoch the backend assigns; a stale
/// generation (after a reconnect) is how late/duplicate signals are fenced off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum BrokerControl {
    /// backend → client: a subscriber arrived; open a `RegisterData` tunnel
    /// echoing `generation` + `stream_id`. (pb-mapper `LocalServer::Stream`.)
    NewStream {
        /// Session generation that emitted this signal.
        generation: u64,
        /// Correlation id pairing the answering data tunnel to a loopback
        /// accept.
        stream_id: u64,
    },
    /// backend → client: heartbeat reply. (pb-mapper `PongV2`.)
    Pong {
        /// Echoes the ping sequence.
        seq: u64,
    },
    /// backend → client: this registration is being retired (the relay dropped
    /// or replaced it); tear down and reconnect. (pb-mapper
    /// `LocalServer::Retire`.)
    Retire {
        /// Human-readable reason.
        reason: String,
    },
    /// client → backend: liveness ping. (pb-mapper `PingV2`.)
    Ping {
        /// Monotonic (wrapping) sequence number.
        seq: u64,
    },
    /// client → backend: acknowledges a `NewStream` and renews the lease; the
    /// client then dials the matching `RegisterData` tunnel. (pb-mapper
    /// `StreamAck`.)
    StreamAck {
        /// Generation of the acknowledged `NewStream`.
        generation: u64,
        /// Stream id of the acknowledged `NewStream`.
        stream_id: u64,
    },
}

//! Agent Client Protocol (ACP) v1 wire types shared by the host-side hub and
//! the controller-side bridge engine.
//!
//! The types are a local, hand-written subset of the official schema
//! (`schema-v1.23.0`): the official crates enable `serde_json/preserve_order`,
//! which would change the key order `history_sync::json_digest` relies on. The
//! contract test `tests/acp_schema.rs` checks these types against the pinned
//! schema fixture.
//!
//! ```text
//!   rpc        JSON-RPC frames, error codes, `[acp.<code>]` error prefix
//!   types      requests / responses / capabilities / content blocks
//!   update     `session/update` payloads
//!   transcript hub-side folding of updates into bounded items
//!   pcx        `_pcx/*` extension methods and shared result types
//! ```

/// ACP protocol version implemented by Pocket-Codex.
pub const PROTOCOL_VERSION: u16 = 1;

/// `_pcx/*` extension methods and the result types shared by hub and bridge.
pub mod pcx;
/// JSON-RPC 2.0 framing.
pub mod rpc;
/// Bounded transcript materialized from ACP session updates.
pub mod transcript;
/// ACP v1 request, response and capability types.
pub mod types;
/// `session/update` notification payloads.
pub mod update;

pub use rpc::{RequestId, RpcError, RpcMessage};
pub use transcript::{Applied, HubItem, Transcript, TurnInfo};
pub use types::*;
pub use update::{MessageChunk, SessionNotification, SessionUpdate};

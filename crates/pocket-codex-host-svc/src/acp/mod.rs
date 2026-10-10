//! Generic Agent Client Protocol (ACP v1) support.
//!
//! The host runs any ACP-compatible agent as an owned subprocess, configured
//! as an executable plus argument vector ([`spec`]), and is its only ACP
//! client: strict JSON-RPC 2.0 over stdio ([`jsonrpc`], [`peer`]), version
//! and capability negotiation ([`schema`]), and the session, turn and
//! permission lifecycle ([`host`], [`state`]). Controllers never speak ACP;
//! they use the versioned Pocket-Codex gateway ([`api`], [`client`]) over
//! loopback or a relay tunnel. Both sides fold updates with [`fold`]; a
//! controller keeps its live folds aligned with the host's through
//! [`replica`]. Session authority is recorded per agent source ([`store`]).
//!
//! Optional features are used only when the agent advertises them:
//! `session/load` (replay), `session/resume` (no replay), `session/list`,
//! `session/close`, image prompts, select configuration options and legacy
//! modes. The client advertises no file system, terminal, elicitation,
//! boolean-option or other extension capability, and answers such requests
//! with "method not found" without touching any file.

pub mod api;
pub mod client;
pub mod fold;
pub mod host;
pub mod jsonrpc;
pub mod peer;
pub mod process;
pub mod replica;
pub mod schema;
pub mod spec;
pub mod state;
pub mod store;

pub use client::{ClientError, GatewayClient};
pub use host::{AgentHost, HostError, HostOptions, Identity};
pub use replica::Replica;
pub use spec::{AgentSpec, Preset, SpecError, PRESETS};
pub use store::{SessionStore, SourceId};

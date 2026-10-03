//! Host-side ACP hub (TRD §4.2): the only ACP client of one agent process,
//! multiplexing its sessions to many controllers over `_pcx`-extended ACP.
//!
//! Desktop only; the module is not compiled for Android or iOS.

mod auth;
mod defaults;
mod error;
mod fs;
mod history;
mod hub;
mod inbound;
pub mod install;
mod launch;
mod meta;
mod ops;
mod peer;
mod pending;
mod process;
mod server;
mod session;
#[cfg(any(test, feature = "acp-testing"))]
pub mod testing;

pub use auth::{terminal_launch, GatewayAuth, TerminalLaunch, TerminalLauncher};
pub use error::AcpError;
pub use history::{AcpHistorySource, AcpSessionDirs, PROVIDER as HISTORY_PROVIDER};
pub use hub::{AcpHub, ConnId, HubConnection, HubInfo, HubOptions, LaunchProvider};
pub use launch::{AgentConnector, AgentIo, ChildHandle, LaunchSpec, ProcessConnector};
pub use meta::serve_meta;
pub use peer::{Inbound, PeerExit, MAX_LINE_BYTES};
pub use server::{serve_ws, CLOSE_TRY_AGAIN_LATER, MAX_MESSAGE_BYTES};

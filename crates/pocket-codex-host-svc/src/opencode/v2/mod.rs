//! Native OpenCode 2.0.18 protocol, separate from the v1 wire contract.

mod client;
mod contract;
mod forms;
mod protocol;
mod sse;

pub use client::V2Client;
pub use protocol::{
    Form, Location, Message, MessagePage, Permission, PromptAcceptance, ServerInfo, Session,
};
pub use sse::{Event, EventStream};

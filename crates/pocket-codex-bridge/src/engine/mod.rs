//! Non-bridged engine: pure logic + the tokio runtime/registry. Kept
//! separate from `api/` so it is unit-testable without flutter_rust_bridge.
pub mod account;
pub mod acp;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod acp_manage;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod acp_terminal;
pub mod app_events;
pub mod app_session;
pub mod config;
pub mod discovery;
pub mod logging;
pub mod meta;
pub mod opencode;
pub mod runtime;
pub mod serve;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod serve_acp;
pub mod serve_opencode;
pub mod session_cache;
pub mod session_sync;
pub mod sessions;
pub mod transport;

#[cfg(any(target_os = "android", target_os = "ios"))]
mod acp_desktop_stub;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use acp_desktop_stub::{acp_manage, serve_acp};

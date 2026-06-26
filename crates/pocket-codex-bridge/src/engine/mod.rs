//! Non-bridged engine: pure logic + the tokio runtime/registry. Kept
//! separate from `api/` so it is unit-testable without flutter_rust_bridge.
pub mod account;
pub mod app_session;
pub mod config;
pub mod discovery;
pub mod runtime;
pub mod sessions;

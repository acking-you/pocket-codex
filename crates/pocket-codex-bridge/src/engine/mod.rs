//! Non-bridged engine: pure logic + the tokio runtime/registry. Kept
//! separate from `api/` so it is unit-testable without flutter_rust_bridge.
pub mod account;
pub mod acp;
pub mod app_session;
pub mod config;
pub mod discovery;
pub mod hosting;
pub mod logging;
pub mod meta;
pub mod runtime;
pub mod serve;
pub mod serve_acp;
pub mod session_cache;
pub mod session_engine;
pub mod session_sync;
pub mod sessions;
pub mod transport;

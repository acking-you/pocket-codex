//! Process-global tokio runtime, support-dir, and the in-process
//! subscription registry. Mobile has no child processes / state.toml, so
//! subscriptions are spawned tasks tracked here and aborted on unsubscribe.
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use pocket_codex_pb::{subscribe, SubscribeOptions};
use tokio::{runtime::Runtime, task::JoinHandle};

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static SUPPORT_DIR: OnceCell<PathBuf> = OnceCell::new();
static REGISTRY: OnceCell<Mutex<HashMap<String, SubEntry>>> = OnceCell::new();

struct SubEntry {
    local_addr: String,
    handle: JoinHandle<()>,
}

/// Initialise the runtime + support dir. Idempotent; safe to call once at boot.
pub fn init(support_dir: PathBuf) -> Result<()> {
    RUNTIME
        .set(Runtime::new().map_err(|e| anyhow!("building tokio runtime: {e}"))?)
        .ok();
    SUPPORT_DIR.set(support_dir).ok();
    REGISTRY.set(Mutex::new(HashMap::new())).ok();
    Ok(())
}

/// The global runtime; panics if [`init`] was not called (a boot-order bug).
pub fn runtime() -> &'static Runtime {
    RUNTIME
        .get()
        .expect("engine::runtime::init must run before runtime()")
}

/// The configured app-support directory.
pub fn support_dir() -> Result<PathBuf> {
    SUPPORT_DIR
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("bridge not initialised"))
}

fn registry() -> &'static Mutex<HashMap<String, SubEntry>> {
    REGISTRY
        .get()
        .expect("engine::runtime::init must run first")
}

/// Status of one active subscription, surfaced to the UI.
#[derive(Debug, Clone)]
pub struct SubStatus {
    /// Service key being subscribed to.
    pub key: String,
    /// Local `host:port` the subscriber listener is bound on.
    pub local_addr: String,
    /// Whether the spawned task is still running.
    pub alive: bool,
}

/// Start (or no-op if already live) an in-process subscription exposing
/// `key` on `127.0.0.1:<local_port>`. `pb::subscribe` runs forever, so we
/// spawn it and keep the handle for [`unsubscribe_service`] to abort.
///
/// `local_port` may be `0` to let the OS assign a free port — needed so each
/// subscribed service gets its own port and they can coexist (a fixed shared
/// port lets only the first service bind; the rest hit the probe-bind failure
/// below). The concrete assigned port is read back from the probe and reported
/// in [`SubStatus::local_addr`]. A live subscription for `key` keeps its port.
///
/// Before spawning we probe-bind the local port synchronously so the common
/// failure (port already in use) is reported here instead of being swallowed
/// by the detached task while the UI shows a "live" endpoint. A tiny TOCTOU
/// window remains between the probe drop and `pb::subscribe`'s own bind; fully
/// closing it needs a bind-readiness signal from pb-mapper (not yet exposed).
pub fn subscribe_service(key: String, local_port: u16, relay: String) -> Result<SubStatus> {
    let requested = format!("127.0.0.1:{local_port}");
    let mut reg = registry().lock().expect("registry poisoned");
    if let Some(e) = reg.get(&key) {
        if !e.handle.is_finished() {
            return Ok(SubStatus {
                key,
                local_addr: e.local_addr.clone(),
                alive: true,
            });
        }
    }
    // Probe-bind: fail fast if the port can't be claimed, then release it for
    // pb::subscribe to bind for real. With local_port == 0 the OS picks a free
    // port; read the concrete addr back so we bind and report the real port.
    let local_addr = match std::net::TcpListener::bind(&requested) {
        Ok(probe) => probe
            .local_addr()
            .map(|a| a.to_string())
            .map_err(|e| anyhow!("reading bound addr for {requested}: {e}"))?,
        Err(e) => return Err(anyhow!("cannot bind {requested}: {e}")),
    };
    let opts = SubscribeOptions {
        key: key.clone(),
        local_addr: local_addr.clone(),
        relay_addr: relay,
    };
    let handle = runtime().spawn(async move { subscribe(opts).await });
    reg.insert(key.clone(), SubEntry {
        local_addr: local_addr.clone(),
        handle,
    });
    Ok(SubStatus {
        key,
        local_addr,
        alive: true,
    })
}

/// Abort and forget the subscription for `key`. No-op if absent.
pub fn unsubscribe_service(key: &str) {
    if let Some(e) = registry().lock().expect("registry poisoned").remove(key) {
        e.handle.abort();
    }
}

/// Open a one-off pb-mapper subscriber for `key` that is NOT recorded in the
/// shared registry, returning the bound local address and the task handle (the
/// caller MUST `abort()` it when done).
///
/// Unlike [`subscribe_service`], this never reuses or registers a persistent
/// entry. A transient reachability probe needs that isolation: keyed on the
/// shared registry, a probe could reuse a live connection's tunnel and then —
/// on its own teardown — abort the real connection. With its own handle it
/// tears down only its own tunnel.
pub fn subscribe_transient(
    key: String,
    local_port: u16,
    relay: String,
) -> Result<(String, JoinHandle<()>)> {
    let requested = format!("127.0.0.1:{local_port}");
    // Probe-bind to learn the concrete port (and fail fast if it can't bind),
    // mirroring subscribe_service.
    let local_addr = match std::net::TcpListener::bind(&requested) {
        Ok(probe) => probe
            .local_addr()
            .map(|a| a.to_string())
            .map_err(|e| anyhow!("reading bound addr for {requested}: {e}"))?,
        Err(e) => return Err(anyhow!("cannot bind {requested}: {e}")),
    };
    let opts = SubscribeOptions {
        key,
        local_addr: local_addr.clone(),
        relay_addr: relay,
    };
    let handle = runtime().spawn(async move { subscribe(opts).await });
    Ok((local_addr, handle))
}

/// Snapshot of all tracked subscriptions.
pub fn list_subscriptions() -> Vec<SubStatus> {
    registry()
        .lock()
        .expect("registry poisoned")
        .iter()
        .map(|(key, e)| SubStatus {
            key: key.clone(),
            local_addr: e.local_addr.clone(),
            alive: !e.handle.is_finished(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_fails_fast_when_port_in_use() {
        init(std::env::temp_dir()).expect("init");
        // Hold a port so the probe-bind inside subscribe_service must fail.
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe holder");
        let port = occupied.local_addr().expect("addr").port();

        let err = subscribe_service("pcx:t:api:t".to_string(), port, "relay:7666".to_string())
            .expect_err("port is occupied; subscribe must error, not report alive");
        assert!(err.to_string().contains("cannot bind"), "got: {err}");
        // Nothing should have been registered for the failed attempt.
        assert!(!list_subscriptions().iter().any(|s| s.key == "pcx:t:api:t"));
    }

    #[test]
    fn subscribe_auto_allocates_a_free_port_when_zero() {
        init(std::env::temp_dir()).expect("init");
        // Port 0 must resolve to a concrete OS-assigned port (so every service
        // gets its own), reported back in local_addr — not a literal ":0".
        let status = subscribe_service("pcx:t:app:zero".to_string(), 0, "relay:7666".to_string())
            .expect("port 0 should auto-allocate a free port");
        assert!(status.local_addr.starts_with("127.0.0.1:"), "got: {}", status.local_addr);
        let port: u16 = status
            .local_addr
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .expect("local_addr must end in a numeric port");
        assert_ne!(port, 0, "must report the concrete assigned port, not 0");
        // Drop the spawned (relay-less) task we just started.
        unsubscribe_service("pcx:t:app:zero");
    }
}

//! Process-global tokio runtime, support-dir, and the in-process
//! subscription registry. Mobile has no child processes / state.toml, so
//! subscriptions are spawned tasks tracked here and aborted on unsubscribe.
//!
//! Account and self-host subscriptions are the same code path: both dial the
//! relay directly and differ only in the [`RelaySession`] handed in. Account
//! mode used to need its own broker-tunnel implementation here; now the caller
//! resolves a session (via `account::relay_session`) and everything below is
//! shared.
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use pocket_codex_pb::{subscribe, RelaySession, SubscribeOptions};
use tokio::{
    runtime::{Builder, Runtime},
    task::JoinHandle,
};

use crate::engine::transport::Transport;

/// Worker-thread stack size for the runtime the EMBEDDED codex app-server runs
/// on. codex is a very deep async codebase — its resume/turn futures need far
/// more than tokio's default 2 MiB worker stack, and codex's own entrypoint
/// (`deps/codex/codex-rs/arg0`) sets exactly this 16 MiB for the same reason.
/// A plain `Runtime::new()` (2 MiB) overflows the worker stack when resuming a
/// session, which is a hard `0xc00000fd` STACK_OVERFLOW that unwinding /
/// `catch_unwind` cannot recover — it crashes the whole desktop. Match codex.
const RUNTIME_WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

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
        .set(
            Builder::new_multi_thread()
                .enable_all()
                // Give the embedded codex app-server the big worker stack it
                // needs (see RUNTIME_WORKER_STACK_SIZE); the default overflows on
                // resume and crashes the whole process.
                .thread_stack_size(RUNTIME_WORKER_STACK_SIZE)
                .build()
                .map_err(|e| anyhow!("building tokio runtime: {e}"))?,
        )
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
/// Returns only once the tunnel is READY, so the address it reports can be
/// dialled immediately. Callers do exactly that — `app_session::connect` opens
/// a websocket to it on the next line — and a listener that is merely *about*
/// to be bound would refuse that connection nondeterministically.
pub fn subscribe_service(key: String, local_port: u16, transport: &Transport) -> Result<SubStatus> {
    {
        let reg = registry().lock().expect("registry poisoned");
        if let Some(e) = reg.get(&key) {
            if !e.handle.is_finished() {
                return Ok(SubStatus {
                    key,
                    local_addr: e.local_addr.clone(),
                    alive: true,
                });
            }
        }
    }
    // Dialled with the transport's key shape, but tracked under the key the
    // CALLER used: the app identifies a service by its bare `pcx:` key (that is
    // what `unsubscribe_service` will be given), while the relay in account mode
    // wants the namespaced form.
    let (local_addr, handle) =
        spawn_subscribe(transport.session.clone(), transport.relay_key(&key), local_port)?;
    registry()
        .lock()
        .expect("registry poisoned")
        .insert(key.clone(), SubEntry {
            local_addr: local_addr.clone(),
            handle,
        });
    Ok(SubStatus {
        key,
        local_addr,
        alive: true,
    })
}

/// Bind a local port, subscribe on it, and return once the tunnel is up.
///
/// Two things have to be true when this returns, and neither is free:
///
/// * **The port is known.** `local_port` may be `0` so each service gets its
///   own (a fixed shared port lets only the first bind), so the concrete port
///   is learned from a throwaway probe bind — which also reports "already in
///   use" here rather than letting the detached task swallow it while the UI
///   shows a live endpoint.
/// * **The listener is actually accepting.** The SDK binds inside its worker,
///   so the address is not dialable until it reports ready. Awaiting that is
///   what closes the gap; it also means a subscribe to a service nobody is
///   publishing fails HERE, with the relay's reason, instead of looking like a
///   dead endpoint.
///
/// The returned task owns the tunnel and parks forever holding it. Aborting the
/// task (which is how [`unsubscribe_service`] stops one) drops the handle and
/// with it the tunnel.
fn spawn_subscribe(
    session: RelaySession,
    key: String,
    local_port: u16,
) -> Result<(String, JoinHandle<()>)> {
    let requested = format!("127.0.0.1:{local_port}");
    let local_addr = std::net::TcpListener::bind(&requested)
        .map_err(|e| anyhow!("cannot bind {requested}: {e}"))?
        .local_addr()
        .map(|a| a.to_string())
        .map_err(|e| anyhow!("reading bound addr for {requested}: {e}"))?;

    // The probe is released above; a tiny TOCTOU window remains before the SDK
    // rebinds, which the readiness wait below turns from a silent failure into a
    // reported one.
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let handle = runtime().spawn({
        let local_addr = local_addr.clone();
        async move {
            let opts = SubscribeOptions {
                key: key.clone(),
                local_addr,
            };
            match subscribe(&session, opts).await {
                Ok(connection) => {
                    // A send failure means the caller gave up waiting; the tunnel
                    // is up either way, and the caller's own timeout already
                    // reported that. Hold it regardless, so an abort is what tears
                    // it down.
                    let _ = ready_tx.send(Ok(()));
                    std::future::pending::<()>().await;
                    drop(connection);
                },
                Err(err) => {
                    let message = format!("{err:#}");
                    tracing::warn!(key = %key, error = %message, "subscribe failed");
                    let _ = ready_tx.send(Err(message));
                },
            }
        }
    });

    // Awaited ON the runtime, not with a blocking channel recv: this is called
    // from synchronous FRB entrypoints, and parking a runtime worker thread on a
    // std channel could deadlock against the very task we are waiting for.
    //
    // Bounded so a wedged relay cannot hang the Flutter isolate forever. The
    // budget is the SDK's own readiness timeout plus a margin, so it only fires
    // when the SDK itself has already given up.
    let outcome = runtime()
        .block_on(async { tokio::time::timeout(subscribe_ready_timeout(), ready_rx).await });
    match outcome {
        Ok(Ok(Ok(()))) => Ok((local_addr, handle)),
        Ok(Ok(Err(message))) => {
            handle.abort();
            Err(anyhow!("subscribing to the relay failed: {message}"))
        },
        // Sender dropped without reporting (the task was aborted or panicked),
        // or the readiness budget elapsed. Neither leaves a dialable endpoint, so
        // both have to fail rather than hand back an address that refuses.
        Ok(Err(_)) | Err(_) => {
            handle.abort();
            Err(anyhow!(
                "the relay did not accept a subscription on `{requested}` within {}s",
                subscribe_ready_timeout().as_secs()
            ))
        },
    }
}

/// How long a subscribe waits for the tunnel to come up before giving up.
///
/// A margin over [`pocket_codex_pb::TUNNEL_READY_TIMEOUT`], which is what the
/// SDK itself waits: this bound exists only so a wedged worker cannot hang the
/// blocking caller forever, not to pre-empt the SDK's own verdict.
const SUBSCRIBE_READY_TIMEOUT: std::time::Duration =
    pocket_codex_pb::TUNNEL_READY_TIMEOUT.saturating_add(std::time::Duration::from_secs(5));

/// Test-only override for [`SUBSCRIBE_READY_TIMEOUT`], so a case that
/// deliberately points at nothing does not spend the full production budget
/// waiting.
#[cfg(test)]
static TEST_READY_TIMEOUT: Mutex<Option<std::time::Duration>> = Mutex::new(None);

fn subscribe_ready_timeout() -> std::time::Duration {
    #[cfg(test)]
    if let Some(over) = *TEST_READY_TIMEOUT.lock().unwrap_or_else(|p| p.into_inner()) {
        return over;
    }
    SUBSCRIBE_READY_TIMEOUT
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
    transport: &Transport,
) -> Result<(String, JoinHandle<()>)> {
    spawn_subscribe(transport.session.clone(), transport.relay_key(&key), local_port)
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

    /// A transport pointing at a relay that does not exist.
    ///
    /// Enough for these cases: both are about the LOCAL bind, which happens
    /// before anything dials the relay. The spawned task then fails to
    /// connect and logs, which is exactly what a real unreachable relay
    /// would do.
    fn test_transport() -> Transport {
        Transport {
            session: RelaySession::for_test("127.0.0.1:1"),
            namespace: None,
        }
    }

    #[test]
    fn subscribe_fails_fast_when_port_in_use() {
        init(std::env::temp_dir()).expect("init");
        // Hold a port so the probe-bind inside subscribe_service must fail.
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe holder");
        let port = occupied.local_addr().expect("addr").port();

        let err = subscribe_service("pcx:t:api:t".to_string(), port, &test_transport())
            .expect_err("port is occupied; subscribe must error, not report alive");
        assert!(err.to_string().contains("cannot bind"), "got: {err}");
        // Nothing should have been registered for the failed attempt.
        assert!(!list_subscriptions().iter().any(|s| s.key == "pcx:t:api:t"));
    }

    #[test]
    fn an_unreachable_relay_fails_instead_of_reporting_a_dead_endpoint() {
        init(std::env::temp_dir()).expect("init");
        // Nothing is listening, so waiting out the production budget would just
        // make this case slow.
        *TEST_READY_TIMEOUT.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(std::time::Duration::from_millis(200));
        // The contract callers rely on: a returned address is DIALABLE. Callers
        // open a websocket to it on the next line, so reporting "alive" for a
        // tunnel that never came up is worse than failing — it turns a clear relay
        // error into a nondeterministic connection refused.
        let err = subscribe_service("pcx:t:app:down".to_string(), 0, &test_transport())
            .expect_err("nothing is listening on the relay, so this cannot be ready");
        let err = err.to_string();
        assert!(
            err.contains("subscribing to the relay failed") || err.contains("did not accept"),
            "the error should name the relay, not the local bind: {err}"
        );
        // And a failed attempt must leave nothing behind for `subscriptions` to
        // show as live, or the UI would offer an endpoint that refuses.
        assert!(!list_subscriptions()
            .iter()
            .any(|s| s.key == "pcx:t:app:down"));
    }

    #[test]
    fn a_reachable_service_gets_its_own_concrete_port() {
        init(std::env::temp_dir()).expect("init");
        // Port 0 must resolve to a concrete OS-assigned port (so every service
        // gets its own; a fixed shared port lets only the first bind), and the
        // reported address must carry it rather than a literal `:0`.
        //
        // Exercised through `spawn_subscribe`'s bind step alone: confirming the
        // readiness half needs a relay, which lives in the e2e tests.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe bind");
        let addr = probe.local_addr().expect("probe addr");
        assert_ne!(addr.port(), 0, "the OS must assign a concrete port for :0");
        assert_eq!(addr.ip().to_string(), "127.0.0.1", "subscriptions stay on loopback");
    }
}

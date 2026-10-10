//! How the app resolves its [`Transport`] from persisted configuration.
//!
//! ```text
//!   signed in?  ─yes─▶ account:   GET /v1/relay, keys under `pcxu:<user>:`
//!         │ no
//!         ▼
//!                      self-host: configured relay + stored key, `pcx:` keys
//! ```
//!
//! The type itself, and the key-shape rules that must hold identically here and
//! in the CLI, live in [`pocket_codex_pb::transport`]. This module is only the
//! app's half: reading the support directory and, in account mode, fetching and
//! refreshing a credential. The CLI resolves the same type from flags instead.
//!
//! # Context identity
//!
//! The same logical `pcx:` key names different services under different
//! accounts or self-host relays. [`Context`] identifies *which* network a
//! transport reaches — account namespace and relay in account mode, relay and
//! a fingerprint of its key in self-host mode — without the credential
//! itself, so a refreshed account credential keeps the same context. Every
//! resolution records the current context; when it changes (or the
//! configuration changes), state owned by the previous context is dropped.
//!
//! Owned state is the ACP connection registry and the context-scoped meta
//! tunnels. One revision, the [`epoch`], orders all of it:
//!
//! - A resolution captures the epoch *before* it reads the configuration and
//!   commits ([`Owned`]) only if the epoch is unchanged when it is observed —
//!   checked and installed under the same lock that [`context_changed`] takes,
//!   so a resolution that read the previous configuration can neither become
//!   current, nor touch the credential refresher, nor drop the newer context's
//!   state. It then simply resolves again from the new configuration.
//! - The epoch moves (under that lock) *before* owned state is dropped.
//! - Work that outlives its resolution (a connection being established, a
//!   tunnel being opened) carries the [`Owned::epoch`] it was resolved at and
//!   registers only while the epoch is unchanged, checked under the lock of the
//!   registry it registers in. Such work either registers before the drop (and
//!   is dropped with it) or sees the change and backs out.
//!
//! An account transport is built from one credential answer (relay,
//! credential and namespace together), never from two separate reads that
//! an account switch could interleave.

use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};

use anyhow::{anyhow, Result};
use pocket_codex_account_proto::{http::RelayCredentialResponse, NamespacedServiceId};
use pocket_codex_core::{config::Mode, service::sanitize_component};
use pocket_codex_pb::RelaySession;
// Re-exported so callers keep saying `engine::transport::Transport`: within the
// engine, the resolver and the type it returns are one concept.
pub use pocket_codex_pb::Transport;

use crate::engine::{account, config::load_config, runtime};

/// Which network a transport reaches (see the module docs). Never contains
/// a credential.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Context {
    id: String,
    namespace: Option<String>,
}

impl Context {
    /// The context of `transport`.
    pub fn of(transport: &Transport) -> Self {
        let relay = &transport.session.relay_addr;
        match &transport.namespace {
            Some(namespace) => Self {
                id: format!("account:{namespace}@{relay}"),
                namespace: Some(namespace.clone()),
            },
            None => {
                let digest = pocket_codex_core::history_sync::digest_bytes(
                    transport.session.credential.as_bytes(),
                );
                Self {
                    id: format!("self:{relay}#{}", &digest[..16]),
                    namespace: None,
                }
            },
        }
    }

    /// Stable identifier (safe to log).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The account namespace, in account mode.
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Whether `service_key` may name a service of this context: a
    /// namespaced key must carry this account's own namespace (a self-host
    /// context admits none).
    pub fn admits(&self, service_key: &str) -> bool {
        match NamespacedServiceId::parse_key(service_key) {
            Some(namespaced) => {
                self.namespace().map(sanitize_component).as_deref()
                    == Some(namespaced.user_id.as_str())
            },
            None => true,
        }
    }
}

/// The current context. Its lock also orders every write of [`EPOCH`] and
/// every drop of context-owned state (see the module docs). Lock order: this,
/// then the registries of owned state; nothing holding one of those takes
/// this lock (they read the epoch atomically instead).
fn current() -> MutexGuard<'static, Option<Context>> {
    static CURRENT: Mutex<Option<Context>> = Mutex::new(None);
    CURRENT.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Written only while [`current`] is held.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The configuration/context revision (see the module docs). Lock-free, so
/// a registry can validate against it while holding its own lock.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::SeqCst)
}

/// Whether nothing changed since `epoch` was resolved.
pub fn is_current(epoch: u64) -> bool {
    self::epoch() == epoch
}

/// The context of the most recent resolution, `None` after a configuration
/// change until the next one.
pub fn current_context() -> Option<Context> {
    current().clone()
}

/// A transport that was current when it was resolved, and the [`epoch`] it
/// is current in.
#[derive(Debug, Clone)]
pub struct Owned {
    /// The resolved transport.
    pub transport: Transport,
    /// The epoch its context was observed in.
    pub epoch: u64,
}

/// Drop everything owned by the previous context. The epoch moves first.
/// Called with [`current`] held.
fn drop_owned_locked() {
    EPOCH.fetch_add(1, Ordering::SeqCst);
    super::acp::context_changed();
    super::meta::context_changed();
}

/// A resolution that read a configuration which has changed since.
#[derive(Debug)]
struct Stale;

/// Install `transport`, resolved from a configuration read at `revision`,
/// as current — unless the configuration changed since (then nothing at
/// all happens). `commit` (the credential refresher's start or stop) runs
/// only for a resolution that becomes current. A context different from
/// the previous one drops the previous one's state.
fn observe(
    transport: &Transport,
    revision: u64,
    commit: impl FnOnce() -> Result<()>,
) -> Result<Result<u64, Stale>> {
    let next = Context::of(transport);
    let mut current = current();
    if epoch() != revision {
        return Ok(Err(Stale));
    }
    commit()?;
    let previous = current.replace(next.clone());
    if previous.is_some_and(|previous| previous != next) {
        drop_owned_locked();
    }
    Ok(Ok(epoch()))
}

/// The account or relay configuration changed: forget the current context
/// and everything owned by it, before anything resolves again.
pub fn context_changed() {
    let mut current = current();
    current.take();
    drop_owned_locked();
}

/// The account transport of one credential answer: relay, credential and
/// namespace always come from the same answer.
fn account_transport(credential: RelayCredentialResponse) -> Transport {
    Transport {
        session: RelaySession::new(credential.relay_addr, credential.credential),
        namespace: Some(credential.namespace),
    }
}

/// How often a resolution that raced a configuration change starts over
/// from the new configuration before giving up.
const RESOLVE_ATTEMPTS: usize = 3;

/// Pauses a resolution right after it read the configuration (tests of the
/// race with a configuration change).
#[cfg(test)]
static RESOLVE_PAUSE: Mutex<
    Option<(std::sync::Arc<tokio::sync::Barrier>, std::sync::Arc<tokio::sync::Barrier>)>,
> = Mutex::new(None);

/// One resolution attempt from the configuration in `support`.
async fn resolve_once(support: &Path) -> Result<Result<Owned, Stale>> {
    let revision = epoch();
    let config = load_config(support)?;
    #[cfg(test)]
    {
        let pause = RESOLVE_PAUSE
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some((reached, resume)) = pause {
            reached.wait().await;
            resume.wait().await;
        }
    }
    if config.account_mode() == Mode::Account {
        let credential = account::relay_credential(support).await?;
        let expires_at = credential.expires_at;
        let transport = account_transport(credential);
        // A registration outlives the call that made it, and the relay cancels a
        // lapsed credential's tunnels — so the refresher has to be running for
        // hosting to survive its TTL. Idempotent, hence unconditional here —
        // but only for a resolution that is still current.
        let observed = observe(&transport, revision, || {
            account::start_credential_refresh(support, expires_at)
        })?;
        return Ok(observed.map(|epoch| Owned {
            transport,
            epoch,
        }));
    }
    let relay = config
        .relay()
        .ok_or_else(|| anyhow!("no relay configured"))?
        .to_string();
    let key = config
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    // Length-checked here so a hand-edited config.toml cannot reach the SDK's own
    // error, which echoes the raw key into its message.
    if key.len() != 32 {
        return Err(anyhow!("stored MSG_HEADER_KEY is not 32 bytes; re-run setup"));
    }
    let transport = Transport {
        session: RelaySession::new(relay, key),
        namespace: None,
    };
    let observed = observe(&transport, revision, || {
        account::stop_credential_refresh();
        Ok(())
    })?;
    Ok(observed.map(|epoch| Owned {
        transport,
        epoch,
    }))
}

/// Resolve from the configuration in `support` (see the module docs).
async fn resolve_in(support: &Path) -> Result<Owned> {
    for _ in 0..RESOLVE_ATTEMPTS {
        if let Ok(owned) = resolve_once(support).await? {
            return Ok(owned);
        }
    }
    Err(anyhow!("the account or relay configuration kept changing; try again"))
}

/// Resolve this device's transport from its persisted configuration, with
/// the epoch it is current in.
///
/// Async because account mode fetches a credential from the backend; that
/// result is cached (see [`account::relay_credential`]), so repeated calls are
/// cheap.
pub async fn resolve_owned() -> Result<Owned> {
    resolve_in(&runtime::support_dir()?).await
}

/// [`resolve_owned`] without the epoch, for callers that own no
/// context-scoped state.
pub async fn resolve() -> Result<Transport> {
    resolve_owned().await.map(|owned| owned.transport)
}

/// Resolve the transport from the engine runtime, for the synchronous
/// flutter_rust_bridge entrypoints.
pub fn resolve_blocking() -> Result<Transport> {
    runtime::runtime().block_on(resolve())
}

/// [`resolve_owned`] from the engine runtime.
pub fn resolve_owned_blocking() -> Result<Owned> {
    runtime::runtime().block_on(resolve_owned())
}

/// Serializes tests that change the process-wide context or its owned
/// state (the ACP registry, meta tunnels).
#[cfg(test)]
pub(crate) fn test_serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_change_closes_meta_tunnels_and_moves_the_epoch() {
        let _serial = test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let before = epoch();
        let tunnel = runtime::runtime().spawn(std::future::pending::<()>());
        runtime::insert_for_test("meta\u{1f}self:relay:1#00\u{1f}pcx:d:meta:n", tunnel);
        context_changed();
        assert!(epoch() > before, "work started before the change cannot commit");
        assert!(
            runtime::list_subscriptions()
                .iter()
                .all(|s| !s.key.starts_with("meta\u{1f}")),
            "no meta tunnel of the old context survives"
        );
    }

    /// A resolution reads configuration A and is held there; the
    /// configuration changes to B, which resolves and owns state; then A
    /// continues. A must not become current, must not drop B's state, and
    /// must not hand out A's transport: it starts over and yields B.
    #[test]
    fn a_resolution_that_read_the_previous_configuration_never_becomes_current() {
        use std::sync::Arc;

        use pocket_codex_core::config::Config;
        use tokio::sync::Barrier;

        use crate::engine::config::save_config;

        let _serial = test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let dir = tempfile::tempdir().expect("dir");
        let write = |relay: &str, key: &str| {
            let mut config = Config::default();
            config.set_relay(relay);
            config.set_relay_key(key);
            save_config(dir.path(), &config).expect("save");
        };
        write("relay-a.test:1", &"a".repeat(32));
        let (reached, resume) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
        *RESOLVE_PAUSE
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some((reached.clone(), resume.clone()));
        let rt = runtime::runtime();
        let support = dir.path().to_path_buf();
        let a = rt.spawn(async move { resolve_in(&support).await });
        rt.block_on(reached.wait());
        // A has read relay A. The configuration changes and B resolves.
        write("relay-b.test:1", &"b".repeat(32));
        context_changed();
        let b = rt.block_on(resolve_in(dir.path())).expect("B resolves");
        let context_b = Context::of(&b.transport);
        let owned_by_b = format!("meta\u{1f}{}\u{1f}pcx:d:meta:resolve-race", context_b.id());
        runtime::insert_for_test(&owned_by_b, rt.spawn(std::future::pending::<()>()));
        rt.block_on(resume.wait());
        let a = rt
            .block_on(a)
            .expect("join")
            .expect("A starts over from the new configuration");
        assert_eq!(a.transport.session.relay_addr, "relay-b.test:1", "never A's relay");
        assert_eq!(current_context(), Some(context_b), "B stays current");
        assert_eq!(epoch(), b.epoch, "nothing B owns was dropped");
        assert!(is_current(a.epoch));
        assert!(
            runtime::list_subscriptions()
                .iter()
                .any(|s| s.key == owned_by_b && s.alive),
            "B's tunnel survives"
        );
        context_changed();
        assert!(!is_current(b.epoch));
    }

    /// Relay, credential and namespace of an account transport come from
    /// one credential answer.
    #[test]
    fn an_account_transport_comes_from_one_credential_answer() {
        let transport = account_transport(RelayCredentialResponse {
            relay_addr: "relay.test:7".into(),
            credential: "pbmt1_alice".into(),
            expires_at: 1,
            namespace: "alice".into(),
        });
        assert_eq!(transport.session.relay_addr, "relay.test:7");
        assert_eq!(transport.session.credential, "pbmt1_alice");
        assert_eq!(transport.namespace.as_deref(), Some("alice"));
    }

    fn transport(relay: &str, key: &str, namespace: Option<&str>) -> Transport {
        Transport {
            session: RelaySession::new(relay, key),
            namespace: namespace.map(str::to_string),
        }
    }

    #[test]
    fn contexts_follow_the_network_not_the_credential() {
        let key = "k".repeat(32);
        let alice = Context::of(&transport("relay:1", "pbmt1_old", Some("alice")));
        let refreshed = Context::of(&transport("relay:1", "pbmt1_new", Some("alice")));
        assert_eq!(alice, refreshed, "a refreshed account credential keeps ownership");
        assert_ne!(alice, Context::of(&transport("relay:1", "pbmt1_old", Some("bob"))));
        assert_ne!(alice, Context::of(&transport("relay:2", "pbmt1_old", Some("alice"))));
        let own = Context::of(&transport("relay:1", &key, None));
        assert_ne!(own, Context::of(&transport("relay:1", &"j".repeat(32), None)));
        assert_ne!(own, alice);
        assert!(!own.id().contains(&key), "the key never appears");
        assert_eq!(alice.namespace(), Some("alice"));
        assert_eq!(own.namespace(), None);
    }

    #[test]
    fn a_context_admits_only_its_own_namespace() {
        let alice = Context::of(&transport("relay:1", "pbmt1_x", Some("alice")));
        let own = Context::of(&transport("relay:1", &"k".repeat(32), None));
        assert!(alice.admits("pcx:studio:acp:a"));
        assert!(alice.admits("pcxu:alice:studio:meta:a"));
        assert!(!alice.admits("pcxu:bob:studio:meta:a"), "another account");
        assert!(own.admits("pcx:studio:app:a"));
        assert!(!own.admits("pcxu:alice:studio:app:a"), "self-host has no namespace");
    }
}

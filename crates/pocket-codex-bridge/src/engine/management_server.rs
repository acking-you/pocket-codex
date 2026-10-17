//! Lifetime of the device-level listener and its one relay registration.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};

use anyhow::{bail, Context, Result};
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_pb::{publish_pending, Published, RegisterOptions};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::engine::{
    host_profiles::{
        self,
        files::{write_private, Committed, FileLock},
    },
    host_services, runtime, transport,
};

static AUTHORITY: Mutex<()> = Mutex::new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);
static PERMITTED: AtomicBool = AtomicBool::new(false);
static STOPPING: AtomicBool = AtomicBool::new(false);
static LOOP: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static STATUS: Mutex<Option<LocalStatus>> = Mutex::new(None);

/// Local management setting and current publication state (no credentials).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    /// Whether this device supports hosting.
    pub supported: bool,
    /// This computer permits management from its already-authorized devices.
    pub enabled: bool,
    /// The registration is currently live.
    pub registered: bool,
    /// Device-level key, present after its listener starts.
    pub service_key: Option<String>,
    /// Reachable listener only while enabled; never contains credentials.
    pub listen_addr: Option<String>,
    /// Safe explanation when management could not be started.
    pub problem: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Preference {
    enabled: Option<bool>,
}

fn supported() -> bool {
    !cfg!(any(target_os = "ios", target_os = "android"))
}

fn preference() -> Result<Preference> {
    let path = host_profiles::root()?.join("management.json");
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("reading device management setting"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preference::default()),
        Err(e) => Err(e).context("reading device management setting"),
    }
}

fn update(enabled: bool, only_unset: bool) -> Result<()> {
    if !supported() {
        bail!("this device cannot host services");
    }
    let root = host_profiles::root()?;
    let _lock = FileLock::acquire(&root)?;
    if only_unset && preference()?.enabled.is_some() {
        return Ok(());
    }
    let value = Preference {
        enabled: Some(enabled),
    };
    if write_private(&root.join("management.json"), &serde_json::to_vec(&value)?)?
        != Committed::Durable
    {
        bail!("device management setting could not be durably saved");
    }
    Ok(())
}

/// Start the supervisor once. Mobile never opens a management listener.
pub fn initialize() {
    if !supported() || is_stopping() {
        return;
    }
    let mut task = LOOP.lock().unwrap_or_else(|p| p.into_inner());
    if task.is_some() {
        return;
    }
    *task = Some(runtime::runtime().spawn(supervise()));
}

/// First successful local hosting enables management unless explicitly
/// disabled.
pub fn enable_after_hosting() {
    if let Err(error) = update(true, true) {
        tracing::warn!(%error, "could not save device management preference");
    }
    initialize();
}

/// Explicit local setting. No remote HTTP route can change this grant.
pub fn set_enabled(enabled: bool) -> Result<LocalStatus> {
    let authority = AUTHORITY.lock().unwrap_or_else(|p| p.into_inner());
    update(enabled, false)?;
    GENERATION.fetch_add(1, Ordering::SeqCst);
    PERMITTED.store(enabled, Ordering::SeqCst);
    if let Some(task) = LOOP.lock().unwrap_or_else(|p| p.into_inner()).take() {
        task.abort();
    }
    if !enabled {
        *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    drop(authority);
    initialize();
    local_status()
}

/// Current local state; callers can display an enabled-but-offline
/// registration.
pub fn local_status() -> Result<LocalStatus> {
    let preference = if supported() { preference() } else { Ok(Preference::default()) };
    let previous = STATUS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Ok(status_from(preference, previous))
}

fn status_from(preference: Result<Preference>, previous: Option<LocalStatus>) -> LocalStatus {
    let enabled = supported() && preference.as_ref().is_ok_and(|p| p.enabled == Some(true));
    let mut status = previous.unwrap_or(LocalStatus {
        supported: supported(),
        enabled,
        registered: false,
        service_key: None,
        listen_addr: None,
        problem: None,
    });
    status.enabled = enabled;
    if !enabled {
        status.registered = false;
        status.listen_addr = None;
    }
    if preference.is_err() {
        status.problem = Some(
            "Device management settings could not be read. Choose whether to enable management \
             again on this computer."
                .into(),
        );
    }
    status
}

#[cfg(test)]
#[test]
fn unreadable_setting_keeps_the_local_repair_control_available() {
    let status = status_from(Err(anyhow::anyhow!("private filesystem detail")), None);
    assert_eq!(status.supported, supported());
    assert!(!status.enabled);
    assert!(!status.registered);
    assert!(status.problem.is_some());
    assert!(!serde_json::to_string(&status)
        .expect("JSON")
        .contains("private filesystem detail"));
}

pub(super) fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

pub(super) fn accepts(generation: u64) -> bool {
    generation == self::generation()
        && PERMITTED.load(Ordering::SeqCst)
        && !is_stopping()
        && generation == self::generation()
}

pub(super) fn is_stopping() -> bool {
    STOPPING.load(Ordering::SeqCst)
}

/// Fence requests and withdraw this listener on app quit, without touching
/// engines.
pub fn stop() {
    let _authority = AUTHORITY.lock().unwrap_or_else(|p| p.into_inner());
    GENERATION.fetch_add(1, Ordering::SeqCst);
    STOPPING.store(true, Ordering::SeqCst);
    PERMITTED.store(false, Ordering::SeqCst);
    if let Some(task) = LOOP.lock().unwrap_or_else(|p| p.into_inner()).take() {
        task.abort();
    }
    *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

struct Serving {
    epoch: u64,
    task: JoinHandle<()>,
    published: Option<Published>,
    status: LocalStatus,
}
impl Drop for Serving {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn launch(generation: u64) -> Result<Serving> {
    let owned = tokio::task::spawn_blocking(transport::resolve_owned_blocking).await??;
    if !transport::is_current(owned.epoch) || !accepts(generation) {
        bail!("management context changed");
    }
    let (listener, addr) = host_services::bind_loopback("device management")?;
    let key = ServiceId::new(runtime::device_id(), ServiceKind::Host, "pocket").key();
    let app = super::router(owned.epoch, generation);
    let task = runtime::runtime().spawn(host_services::supervise(
        "device management",
        addr,
        listener,
        move |listener| {
            let app = app.clone();
            async move {
                axum::serve(listener, app)
                    .await
                    .context("serving device management")
            }
        },
    ));
    // The guard owns the listener even if publication is cancelled midway.
    let mut serving = Serving {
        epoch: owned.epoch,
        task,
        published: None,
        status: LocalStatus {
            supported: true,
            enabled: true,
            registered: false,
            service_key: Some(key.clone()),
            listen_addr: Some(addr.to_string()),
            problem: None,
        },
    };
    let (published, _) = publish_pending(&owned.transport.session, RegisterOptions {
        key: owned.transport.relay_key(&key),
        local_addr: addr.to_string(),
        codec: true,
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "device management could not register; check for another app using this device name"
        )
    })?;
    serving.published = Some(published);
    if !transport::is_current(owned.epoch) || !accepts(generation) {
        bail!("management context changed");
    }
    Ok(serving)
}

async fn supervise() {
    let mut serving: Option<Serving> = None;
    loop {
        if is_stopping() {
            return;
        }
        let mut generation = generation();
        let enabled = tokio::task::spawn_blocking(preference)
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|pref| pref.enabled == Some(true));
        {
            let _authority = AUTHORITY.lock().unwrap_or_else(|p| p.into_inner());
            if generation != self::generation() || is_stopping() {
                return;
            }
            if PERMITTED.load(Ordering::SeqCst) != enabled {
                generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            }
            PERMITTED.store(enabled, Ordering::SeqCst);
        }
        if !enabled
            || serving
                .as_ref()
                .is_some_and(|s| !transport::is_current(s.epoch))
        {
            serving.take();
            *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        if enabled && serving.is_none() {
            match launch(generation).await {
                Ok(ready) => serving = Some(ready),
                Err(_) => {
                    *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = Some(LocalStatus {
                        supported: true,
                        enabled: true,
                        registered: false,
                        service_key: None,
                        listen_addr: None,
                        problem: Some(
                            "Device management is unavailable. Check the relay and whether \
                             another app uses this device name."
                                .into(),
                        ),
                    });
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                },
            }
        }
        if let Some(current) = &mut serving {
            current.status.registered = host_services::is_published(&current.published);
            *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = Some(current.status.clone());
            if host_services::needs_publishing(&current.published) {
                serving.take();
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

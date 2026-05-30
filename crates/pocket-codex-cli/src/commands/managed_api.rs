//! Background Responses API proxy worker supervision.
//!
//! ```text
//!                       managed_api::ensure(spec)
//!                                  │
//!                                  ▼
//!                       state.find_api(&spec.key)
//!                                  │
//!                ┌─────────────────┼─────────────────┐
//!                ▼                 ▼                 ▼
//!         Some + alive      Some + dead          None
//!                │                 │                 │
//!                ▼                 ▼                 ▼
//!         EnsureOutcome      remove_api +      spawn_worker
//!         ::Reused           spawn_worker      ::Spawned
//!                            ::Replaced
//!                            (stale_pid kept)
//!
//!   spawn_worker:
//!     argv = [self_exe, "__worker", "api-proxy", "--listen", local_addr]
//!     stdout/stderr → paths::api_proxy_log_file(key)
//!     state.upsert_api(ApiProxyInfo { key, local_addr, pid, log_file, started_at })
//! ```
//!
//! Workers are detached children of the CLI invocation; the parent
//! drops the [`std::process::Child`] handle and the OS keeps the worker
//! running until [`stop_all`] sends `SIGTERM`. Liveness is decided by
//! [`pocket_codex_core::process::pid_alive`], so a worker that crashed
//! between invocations is transparently replaced.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use chrono::Utc;
use pocket_codex_core::{
    paths,
    process::{pid_alive, send_sigterm},
    state::{ApiProxyInfo, RuntimeState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiWorkerSpec {
    pub key: String,
    pub local_addr: String,
    /// Explicit upstream proxy forwarded to the worker via argv. `None`
    /// lets the worker fall back to proxy environment variables.
    pub proxy: Option<String>,
    /// Redacted signature of the *effective* upstream proxy (explicit flag
    /// or resolved env). Persisted in state so a rerun with a changed proxy
    /// restarts the live worker instead of silently reusing the old one.
    pub proxy_signature: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum EnsureOutcome {
    Reused(ApiProxyInfo),
    Replaced { stale_pid: u32, session: ApiProxyInfo },
    Spawned(ApiProxyInfo),
}

#[derive(Debug, Clone)]
pub(crate) enum StopOutcome {
    Stopped(ApiProxyInfo),
    Stale(ApiProxyInfo),
}

pub(crate) fn ensure(spec: ApiWorkerSpec) -> Result<EnsureOutcome> {
    ensure_with_exe(spec, std::env::current_exe().context("locating current executable")?)
}

fn ensure_with_exe(spec: ApiWorkerSpec, exe: PathBuf) -> Result<EnsureOutcome> {
    let mut state = RuntimeState::load()?;
    if let Some(existing) = state.find_api(&spec.key).cloned() {
        let alive = pid_alive(existing.pid);
        // Reuse only when the live worker's upstream proxy still matches the
        // requested one; otherwise the new --proxy / env value would be
        // silently ignored (the worker keeps its original proxy).
        if alive && existing.proxy == spec.proxy_signature {
            return Ok(EnsureOutcome::Reused(existing));
        }
        if alive {
            send_sigterm(existing.pid);
            wait_for_exit(existing.pid);
        }
        state.remove_api(&spec.key);
        let session = spawn_worker(&spec, exe)?;
        state.upsert_api(session.clone());
        state.save()?;
        return Ok(EnsureOutcome::Replaced {
            stale_pid: existing.pid,
            session,
        });
    }

    let session = spawn_worker(&spec, exe)?;
    state.upsert_api(session.clone());
    state.save()?;
    Ok(EnsureOutcome::Spawned(session))
}

/// Wait briefly for a signalled worker to exit so its replacement can rebind
/// the same listen port. Polls up to ~3s; returns early once the pid is gone.
fn wait_for_exit(pid: u32) {
    for _ in 0..30 {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn spawn_worker(spec: &ApiWorkerSpec, exe: PathBuf) -> Result<ApiProxyInfo> {
    let log_file = paths::api_proxy_log_file(&spec.key)?;
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let log_handle_dup = log_handle.try_clone()?;

    let child = Command::new(exe)
        .args(api_worker_args(spec))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_handle))
        .stderr(Stdio::from(log_handle_dup))
        .spawn()
        .context("spawning API proxy worker")?;
    let pid = child.id();
    drop(child);

    Ok(ApiProxyInfo {
        key: spec.key.clone(),
        local_addr: spec.local_addr.clone(),
        proxy: spec.proxy_signature.clone(),
        pid,
        log_file,
        started_at: Utc::now().to_rfc3339(),
    })
}

pub(crate) fn api_worker_args(spec: &ApiWorkerSpec) -> Vec<String> {
    let mut args = vec![
        "__worker".to_string(),
        "api-proxy".to_string(),
        "--listen".to_string(),
        spec.local_addr.clone(),
    ];
    if let Some(proxy) = &spec.proxy {
        args.push("--proxy".to_string());
        args.push(proxy.clone());
    }
    args
}

pub(crate) fn stop_all() -> Result<Vec<StopOutcome>> {
    let mut state = RuntimeState::load()?;
    let mut outcomes = Vec::new();
    for session in std::mem::take(&mut state.api) {
        if pid_alive(session.pid) {
            send_sigterm(session.pid);
            outcomes.push(StopOutcome::Stopped(session));
        } else {
            outcomes.push(StopOutcome::Stale(session));
        }
    }
    state.save()?;
    Ok(outcomes)
}

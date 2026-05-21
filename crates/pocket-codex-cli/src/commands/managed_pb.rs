//! Background pb-mapper worker supervision used by high-level commands.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use chrono::Utc;
use pocket_codex_core::{
    paths,
    process::{pid_alive, send_sigterm},
    state::{PbRole, PbSessionInfo, RuntimeState},
};

/// A pb-mapper worker process Pocket-Codex should supervise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PbWorkerSpec {
    /// Register or subscribe.
    pub role: PbRole,
    /// Service key.
    pub key: String,
    /// Local `host:port` used by this worker.
    pub local_addr: String,
    /// Relay `host:port`.
    pub relay_addr: String,
    /// Whether register mode should request pb-mapper encryption.
    pub codec: bool,
}

/// Outcome of ensuring a worker is present.
#[derive(Debug, Clone)]
pub(crate) enum EnsureOutcome {
    /// An existing live worker was reused.
    Reused(PbSessionInfo),
    /// A stale worker record was replaced.
    Replaced {
        /// Stale PID that was replaced.
        stale_pid: u32,
        /// Newly spawned session info.
        session: PbSessionInfo,
    },
    /// A new worker was spawned.
    Spawned(PbSessionInfo),
}

/// Outcome of stopping one recorded worker.
#[derive(Debug, Clone)]
pub(crate) enum StopOutcome {
    /// The process existed and was signalled.
    Stopped(PbSessionInfo),
    /// The state entry existed but the process was already gone.
    Stale(PbSessionInfo),
}

#[derive(Debug, Clone)]
pub(crate) struct StopFilter {
    pub role: Option<PbRole>,
    pub key: Option<String>,
}

/// Start or reuse the worker described by `spec`.
pub(crate) fn ensure(spec: PbWorkerSpec) -> Result<EnsureOutcome> {
    ensure_with_exe(spec, std::env::current_exe().context("locating current executable")?)
}

fn ensure_with_exe(spec: PbWorkerSpec, exe: PathBuf) -> Result<EnsureOutcome> {
    let mut state = RuntimeState::load()?;
    if let Some(existing) = state.find_pb(spec.role, &spec.key).cloned() {
        if pid_alive(existing.pid) {
            return Ok(EnsureOutcome::Reused(existing));
        }
        state.remove_pb(spec.role, &spec.key);
        let session = spawn_worker(&spec, exe)?;
        state.upsert_pb(session.clone());
        state.save()?;
        return Ok(EnsureOutcome::Replaced {
            stale_pid: existing.pid,
            session,
        });
    }

    let session = spawn_worker(&spec, exe)?;
    state.upsert_pb(session.clone());
    state.save()?;
    Ok(EnsureOutcome::Spawned(session))
}

fn spawn_worker(spec: &PbWorkerSpec, exe: PathBuf) -> Result<PbSessionInfo> {
    let log_file = paths::pb_log_file(spec.role, &spec.key)?;
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let log_handle_dup = log_handle.try_clone()?;

    let child = Command::new(exe)
        .args(pb_worker_args(spec))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_handle))
        .stderr(Stdio::from(log_handle_dup))
        .spawn()
        .with_context(|| format!("spawning pb-mapper {:?} worker", spec.role))?;
    let pid = child.id();
    drop(child);

    Ok(PbSessionInfo {
        role: spec.role,
        key: spec.key.clone(),
        local_addr: spec.local_addr.clone(),
        relay_addr: spec.relay_addr.clone(),
        pid,
        log_file,
        codec: spec.codec,
        started_at: Utc::now().to_rfc3339(),
    })
}

/// Build argv for a hidden pb worker.
pub(crate) fn pb_worker_args(spec: &PbWorkerSpec) -> Vec<String> {
    let subcommand = match spec.role {
        PbRole::Register => "pb-register",
        PbRole::Subscribe => "pb-subscribe",
    };
    let mut args = vec![
        "__worker".to_string(),
        subcommand.to_string(),
        "--key".to_string(),
        spec.key.clone(),
        "--local-addr".to_string(),
        spec.local_addr.clone(),
        "--relay".to_string(),
        spec.relay_addr.clone(),
    ];
    if spec.role == PbRole::Register && spec.codec {
        args.push("--codec".to_string());
    }
    args
}

/// Stop pb-mapper sessions matching `filter` and remove their state records.
pub(crate) fn stop_matching(filter: StopFilter) -> Result<Vec<StopOutcome>> {
    let mut state = RuntimeState::load()?;
    let mut kept = Vec::new();
    let mut outcomes = Vec::new();

    for session in std::mem::take(&mut state.pb) {
        if matches_filter(&session, &filter) {
            if pid_alive(session.pid) {
                send_sigterm(session.pid);
                outcomes.push(StopOutcome::Stopped(session));
            } else {
                outcomes.push(StopOutcome::Stale(session));
            }
        } else {
            kept.push(session);
        }
    }

    state.pb = kept;
    state.save()?;
    Ok(outcomes)
}

fn matches_filter(session: &PbSessionInfo, filter: &StopFilter) -> bool {
    filter.role.is_none_or(|role| role == session.role)
        && filter.key.as_ref().is_none_or(|key| key == &session.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pb_worker_args_include_codec_only_for_register() {
        let register = PbWorkerSpec {
            role: PbRole::Register,
            key: "codex".into(),
            local_addr: "127.0.0.1:18080".into(),
            relay_addr: "relay.example:7666".into(),
            codec: true,
        };
        let subscribe = PbWorkerSpec {
            role: PbRole::Subscribe,
            key: "codex".into(),
            local_addr: "127.0.0.1:28080".into(),
            relay_addr: "relay.example:7666".into(),
            codec: true,
        };

        assert_eq!(pb_worker_args(&register), vec![
            "__worker",
            "pb-register",
            "--key",
            "codex",
            "--local-addr",
            "127.0.0.1:18080",
            "--relay",
            "relay.example:7666",
            "--codec"
        ]);
        assert_eq!(pb_worker_args(&subscribe), vec![
            "__worker",
            "pb-subscribe",
            "--key",
            "codex",
            "--local-addr",
            "127.0.0.1:28080",
            "--relay",
            "relay.example:7666"
        ]);
    }
}

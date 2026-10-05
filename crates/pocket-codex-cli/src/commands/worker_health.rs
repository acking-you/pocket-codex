//! Private, bounded runtime snapshots for independently managed network
//! workers.

use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use pocket_codex_core::{
    process::{pb_worker_identity, pb_worker_start_time},
    state::PbSessionInfo,
};
use pocket_codex_pb::{TunnelDiagnostics, TunnelStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WorkerHealth {
    pub pid: u32,
    pub process_started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_identity: Option<String>,
    pub sdk_version: String,
    pub status: String,
    pub diagnostics: serde_json::Value,
}

const MAX_TRANSITIONS: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Transition {
    pid: u32,
    process_identity: String,
    timestamp_ms: u64,
    elapsed_ms: u64,
    status: String,
    reason: Option<String>,
}

#[derive(Default)]
struct Transitions {
    events: VecDeque<Transition>,
    dirty: bool,
}

impl Transitions {
    fn record(&mut self, event: Transition) {
        if self.events.back().is_some_and(|last| {
            last.process_identity == event.process_identity
                && last.status == event.status
                && last.reason == event.reason
        }) {
            return;
        }
        self.events.push_back(event);
        while self.events.len() > MAX_TRANSITIONS {
            self.events.pop_front();
        }
        self.dirty = true;
    }
}

pub(crate) struct Reporter {
    path: PathBuf,
    started: u64,
    identity: String,
    events_path: PathBuf,
    elapsed: Instant,
    transitions: Mutex<Transitions>,
}

fn health_path(session: &PbSessionInfo) -> PathBuf {
    session
        .log_file
        .with_extension(format!("{}.health.json", session.pid))
}

impl Reporter {
    pub fn new(session: &PbSessionInfo) -> Result<Self> {
        Ok(Self {
            path: health_path(session),
            events_path: events_path(session),
            elapsed: Instant::now(),
            transitions: Mutex::new(Transitions {
                events: read_events(session),
                dirty: false,
            }),
            started: pb_worker_start_time(session).context("identifying this network worker")?,
            identity: pb_worker_identity(session).context("identifying this network worker")?,
        })
    }

    pub fn write(&self, status: TunnelStatus, diagnostics: TunnelDiagnostics) -> Result<()> {
        let reason = match &status {
            TunnelStatus::Failed(reason) => Some(reason.chars().take(256).collect()),
            TunnelStatus::Retrying => diagnostics
                .last_failure
                .map(|failure| failure.as_str().to_owned()),
            _ => None,
        };
        let health = WorkerHealth {
            pid: std::process::id(),
            process_started_at: self.started,
            process_identity: Some(self.identity.clone()),
            sdk_version: diagnostics.sdk_version.to_string(),
            status: match status {
                TunnelStatus::Starting => "starting",
                TunnelStatus::Connected => "connected",
                TunnelStatus::Retrying => "retrying",
                TunnelStatus::Stopped => "stopped",
                TunnelStatus::Failed(_) => "failed",
            }
            .to_string(),
            diagnostics: serde_json::to_value(diagnostics)?,
        };
        write_json(&self.path, &health)?;
        let mut transitions = self
            .transitions
            .lock()
            .map_err(|_| anyhow::anyhow!("worker transition history poisoned"))?;
        transitions.record(Transition {
            pid: health.pid,
            process_identity: self.identity.clone(),
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            elapsed_ms: self.elapsed.elapsed().as_millis() as u64,
            status: health.status,
            reason,
        });
        if transitions.dirty {
            write_json(&self.events_path, &transitions.events)?;
            transitions.dirty = false;
        }
        Ok(())
    }
}

fn events_path(session: &PbSessionInfo) -> PathBuf {
    session.log_file.with_extension("events.json")
}

pub(crate) fn read_events(session: &PbSessionInfo) -> VecDeque<Transition> {
    let read = || -> Option<VecDeque<Transition>> {
        let file = fs::File::open(events_path(session)).ok()?;
        if file.metadata().ok()?.len() > 65_536 {
            return None;
        }
        let mut events: VecDeque<Transition> = serde_json::from_reader(file.take(65_537)).ok()?;
        while events.len() > MAX_TRANSITIONS {
            events.pop_front();
        }
        Some(events)
    };
    read().unwrap_or_default()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}

impl Drop for Reporter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("tmp"));
    }
}

pub(crate) fn read(session: &PbSessionInfo) -> Option<WorkerHealth> {
    let mut file = fs::File::open(health_path(session)).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() > 65_536
        || metadata.modified().ok()?.elapsed().ok()? > Duration::from_secs(30)
    {
        return None;
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(65_537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65_536 {
        return None;
    }
    let health: WorkerHealth = serde_json::from_slice(&bytes).ok()?;
    let same_process = match &health.process_identity {
        Some(identity) => pb_worker_identity(session).as_ref() == Some(identity),
        None => pb_worker_start_time(session) == Some(health.process_started_at),
    };
    (health.pid == session.pid && same_process).then_some(health)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_transitions_across_processes_without_heartbeat_noise() {
        let mut history = Transitions::default();
        for i in 0..100 {
            let event = Transition {
                pid: 1,
                process_identity: "first".into(),
                timestamp_ms: i,
                elapsed_ms: i,
                status: if i % 2 == 0 { "connected" } else { "retrying" }.into(),
                reason: None,
            };
            history.record(event.clone());
            history.record(event);
        }
        assert_eq!(history.events.len(), MAX_TRANSITIONS);
        assert_eq!(history.events.front().expect("test fixture").timestamp_ms, 68);
        let mut replacement = history.events.back().expect("test fixture").clone();
        replacement.process_identity = "replacement".into();
        history.record(replacement);
        assert_eq!(
            history
                .events
                .back()
                .expect("test fixture")
                .process_identity,
            "replacement"
        );
    }
}

//! Spawn / inspect / stop a `codex app-server` child process.
//!
//! Pocket-Codex aims to behave like a small process supervisor: it
//! resolves the user's `codex` binary, starts it with the requested
//! `--listen` URL (websocket or unix socket), captures its stdout and
//! stderr to a log file, and persists enough metadata in
//! [`pocket_codex_core::state::RuntimeState`] for a follow-up CLI
//! invocation to attach instead of re-spawning.
//!
//! Daemonisation is intentionally minimal: we use [`std::process`] to
//! spawn and rely on the kernel to keep the child alive after the CLI
//! exits (the parent process drops the [`Child`] handle without
//! waiting). This keeps things portable and avoids depending on
//! platform-specific `daemon(3)` calls.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use chrono::Utc;
use pocket_codex_core::{
    paths,
    state::{CodexProcessInfo, RuntimeState},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::{debug, info, warn};

use crate::protocol::Message as _ProtocolMessage;

/// `codex app-server`'s `--listen` URL choices we explicitly support.
#[derive(Debug, Clone)]
pub enum ListenSpec {
    /// `unix://<path>` — UDS socket.
    UnixSocket(PathBuf),
    /// `ws://<host>:<port>` — WebSocket listener.
    WebSocket {
        /// Host the app-server should bind to.
        host: String,
        /// Port the app-server should bind to.
        port: u16,
    },
}

impl ListenSpec {
    /// Format this listen spec as the URL string accepted by
    /// `codex app-server --listen`.
    pub fn to_listen_url(&self) -> String {
        match self {
            Self::UnixSocket(path) => format!("unix://{}", path.display()),
            Self::WebSocket {
                host,
                port,
            } => format!("ws://{host}:{port}"),
        }
    }

    /// Local TCP `host:port` an external relay should connect to in
    /// order to reach this app-server. Returns `None` for unix socket
    /// transports (which can't be relayed by pb-mapper directly).
    pub fn as_socket_addr(&self) -> Option<String> {
        match self {
            Self::WebSocket {
                host,
                port,
            } => Some(format!("{host}:{port}")),
            Self::UnixSocket(_) => None,
        }
    }
}

/// Options describing how to spawn the codex app-server.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Optional explicit binary path. When `None`, `codex` is looked
    /// up on `$PATH`.
    pub binary: Option<PathBuf>,

    /// `--listen` URL.
    pub listen: ListenSpec,

    /// Extra arguments passed verbatim after `app-server`.
    pub extra_args: Vec<String>,

    /// Log file path. When `None` the default location from
    /// [`paths::codex_log_file`] is used.
    pub log_file: Option<PathBuf>,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            binary: None,
            listen: ListenSpec::WebSocket {
                host: "127.0.0.1".into(),
                port: 18080,
            },
            extra_args: Vec::new(),
            log_file: None,
        }
    }
}

/// Result of [`spawn`].
#[derive(Debug, Clone)]
pub struct SpawnReport {
    /// Process info recorded in [`RuntimeState::codex`].
    pub info: CodexProcessInfo,
}

/// Spawn `codex app-server`, persist the resulting state and return a
/// report describing what was started.
///
/// If the previous run is still alive (PID matches and process exists)
/// the existing process is returned untouched.
pub fn spawn(opts: SpawnOptions) -> pocket_codex_core::Result<SpawnReport> {
    let mut state = RuntimeState::load()?;

    if let Some(existing) = state.codex.clone() {
        if pid_alive(existing.pid) {
            info!(pid = existing.pid, listen = %existing.listen, "codex already running");
            return Ok(SpawnReport {
                info: existing,
            });
        } else {
            warn!(stale_pid = existing.pid, "previous codex process is gone, restarting");
        }
    }

    let binary = match opts.binary.as_ref() {
        Some(path) => path.clone(),
        None => which::which("codex").map_err(|e| {
            pocket_codex_core::Error::Config(format!(
                "could not locate `codex` on $PATH ({e}); install codex or pass --codex-binary"
            ))
        })?,
    };

    let log_file = opts
        .log_file
        .clone()
        .map(Ok)
        .unwrap_or_else(paths::codex_log_file)?;
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let log_handle_dup = log_handle.try_clone()?;

    let listen_url = opts.listen.to_listen_url();
    debug!(?binary, %listen_url, ?log_file, "spawning codex app-server");

    let child = Command::new(&binary)
        .arg("app-server")
        .arg("--listen")
        .arg(&listen_url)
        .args(&opts.extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_handle))
        .stderr(Stdio::from(log_handle_dup))
        .spawn()
        .map_err(|e| {
            pocket_codex_core::Error::Config(format!("failed to spawn `{}`: {e}", binary.display()))
        })?;

    let pid = child.id();
    // Drop the Child handle so the kernel keeps the process alive
    // after this CLI exits; we track it by pid from now on.
    drop(child);

    let info = CodexProcessInfo {
        pid,
        listen: listen_url,
        log_file,
        started_at: Utc::now().to_rfc3339(),
    };
    state.codex = Some(info.clone());
    state.save()?;

    info!(pid, listen = %info.listen, "codex app-server spawned");
    Ok(SpawnReport {
        info,
    })
}

/// Snapshot of the current supervised process.
#[derive(Debug, Clone)]
pub struct StatusReport {
    /// Recorded info, if any.
    pub recorded: Option<CodexProcessInfo>,
    /// Whether the recorded pid is still alive.
    pub alive: bool,
}

/// Inspect the persisted state and report whether the supervised
/// process is still running.
pub fn status() -> pocket_codex_core::Result<StatusReport> {
    let state = RuntimeState::load()?;
    let alive = state.codex.as_ref().is_some_and(|c| pid_alive(c.pid));
    Ok(StatusReport {
        recorded: state.codex,
        alive,
    })
}

/// Outcome of [`stop`].
#[derive(Debug, Clone)]
pub enum StopOutcome {
    /// Nothing to do — there was no recorded process.
    NoRecord,
    /// Previous process was already gone; state was cleaned up.
    StaleRecord {
        /// Pid we cleaned up.
        pid: u32,
    },
    /// Successfully sent the signal and removed the recorded state.
    Stopped {
        /// Pid we signalled.
        pid: u32,
    },
}

/// Send `SIGTERM` to the supervised process and clear the recorded
/// state.
pub fn stop() -> pocket_codex_core::Result<StopOutcome> {
    let mut state = RuntimeState::load()?;
    let Some(info) = state.codex.take() else {
        return Ok(StopOutcome::NoRecord);
    };

    let outcome = if pid_alive(info.pid) {
        send_sigterm(info.pid);
        StopOutcome::Stopped {
            pid: info.pid,
        }
    } else {
        StopOutcome::StaleRecord {
            pid: info.pid,
        }
    };

    state.save()?;
    Ok(outcome)
}

/// Check whether the given pid is still alive.
fn pid_alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::new(),
    );
    sys.process(Pid::from_u32(pid)).is_some()
}

/// Best-effort SIGTERM. Errors are logged but not surfaced because the
/// caller's recorded state is wiped in either case.
fn send_sigterm(pid: u32) {
    use nix::{
        sys::signal::{kill, Signal},
        unistd::Pid as NixPid,
    };
    let nix_pid = NixPid::from_raw(pid as i32);
    if let Err(e) = kill(nix_pid, Signal::SIGTERM) {
        warn!(pid, error = %e, "failed to SIGTERM codex process");
    }
}

// Compile-time assurance the protocol module is reachable from this
// crate (used by integration tests downstream).
#[allow(
    dead_code,
    reason = "anchors the protocol module so future refactors don't quietly drop it"
)]
fn _proto_anchor(_msg: _ProtocolMessage) {}

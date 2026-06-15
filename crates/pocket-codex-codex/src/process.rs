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
    process::{pid_alive, send_sigterm},
    state::{CodexProcessInfo, RuntimeState},
};
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

    /// Upstream proxy injected into the child's environment so codex's
    /// outbound HTTP (codex_apps MCP, model calls, plugin sync) and the
    /// model WebSocket can reach chatgpt.com. When `Some`, the proxy is
    /// written to `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` (and their
    /// lowercase variants) for the child only. When `None`, the child
    /// inherits the parent's environment untouched. The value is taken
    /// verbatim; validation is the caller's responsibility.
    pub proxy: Option<String>,
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
            proxy: None,
        }
    }
}

/// Result of [`spawn`].
#[derive(Debug, Clone)]
pub struct SpawnReport {
    /// Process info recorded in [`RuntimeState::codex`].
    pub info: CodexProcessInfo,

    /// Whether [`spawn`] reused an already-running process instead of
    /// starting a new one. When `true`, any spawn-time options (such as
    /// [`SpawnOptions::proxy`]) had no effect on the live process.
    pub reused: bool,
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
                reused: true,
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

    let child = build_command(&binary, &listen_url, &opts.extra_args, opts.proxy.as_deref())
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
        reused: false,
    })
}

/// Build the `codex app-server` [`Command`] without spawning it or
/// touching stdio / on-disk state.
///
/// When `proxy` is `Some`, the proxy URL is injected into the child's
/// environment under `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` (and
/// their lowercase variants) so codex's reqwest client and WebSocket
/// tunnel both pick it up — neither reads codex's own `config.toml` for
/// proxy settings, they only honour the process environment. As a
/// defensive default we also seed `NO_PROXY` with the loopback hosts so
/// the proxy never swallows codex's local app-server traffic, but only
/// when the parent did not already provide one (we never override an
/// inherited value). When `proxy` is `None` the child inherits the
/// parent environment unchanged.
fn build_command(
    binary: &std::path::Path,
    listen_url: &str,
    extra_args: &[String],
    proxy: Option<&str>,
) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("app-server")
        .arg("--listen")
        .arg(listen_url)
        .args(extra_args);

    if let Some(raw) = proxy {
        // Set both cases so platforms/libraries that read either form agree.
        // On Windows env keys are case-insensitive, so the two writes simply
        // collapse to one entry with the same value — harmless.
        for key in
            ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        {
            command.env(key, raw);
        }
        let inherits_no_proxy =
            std::env::var_os("NO_PROXY").is_some() || std::env::var_os("no_proxy").is_some();
        if !inherits_no_proxy {
            command.env("NO_PROXY", "localhost,127.0.0.1,::1");
            command.env("no_proxy", "localhost,127.0.0.1,::1");
        }
    }

    command
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

// Compile-time assurance the protocol module is reachable from this
// crate (used by integration tests downstream).
#[allow(
    dead_code,
    reason = "anchors the protocol module so future refactors don't quietly drop it"
)]
fn _proto_anchor(_msg: _ProtocolMessage) {}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString, path::Path};

    use super::*;

    /// Collect the explicitly-set environment overrides on a `Command`.
    /// `get_envs` yields only what we set via `.env(..)`, not the
    /// inherited parent environment, so this isolates our injection.
    fn explicit_envs(command: &Command) -> HashMap<OsString, Option<OsString>> {
        command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
            .collect()
    }

    #[test]
    fn build_command_injects_proxy_env_when_set() {
        let command = build_command(
            Path::new("codex"),
            "ws://127.0.0.1:18080",
            &[],
            Some("http://127.0.0.1:11111"),
        );
        let envs = explicit_envs(&command);

        // Look up case-insensitively: Windows env keys are case-insensitive,
        // so the upper/lowercase writes collapse to one entry there, while on
        // unix both survive. Either way each logical var must carry the value.
        let lookup = |name: &str| -> Option<Option<OsString>> {
            envs.iter().find_map(|(k, v)| {
                k.to_str()
                    .is_some_and(|k| k.eq_ignore_ascii_case(name))
                    .then(|| v.clone())
            })
        };
        for key in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
            assert_eq!(
                lookup(key),
                Some(Some(OsString::from("http://127.0.0.1:11111"))),
                "expected {key} to carry the proxy value"
            );
        }
    }

    #[test]
    fn build_command_leaves_env_untouched_without_proxy() {
        let command = build_command(Path::new("codex"), "ws://127.0.0.1:18080", &[], None);

        assert!(
            explicit_envs(&command).is_empty(),
            "no env should be set when proxy is None; child inherits the parent's"
        );
    }
}

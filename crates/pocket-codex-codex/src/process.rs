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
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use pocket_codex_core::{
    paths,
    process::{find_codex_app_server, pid_alive, send_sigterm, tcp_port_open},
    state::{CodexProcessInfo, RuntimeState},
};
use tracing::{debug, info, warn};

use crate::protocol::Message as _ProtocolMessage;

/// How long [`spawn`] waits for a freshly-launched app-server to start
/// listening before giving up and recording the directly-spawned PID. codex
/// usually binds in well under a second; the generous ceiling covers a cold
/// start without hanging an interactive `serve` indefinitely.
const SPAWN_RESOLVE_TIMEOUT: Duration = Duration::from_secs(15);

/// Parse the `host`/`port` out of a `ws://host:port` listen URL. Returns
/// `None` for non-websocket transports (e.g. `unix://`), which have no TCP
/// port to probe and therefore fall back to PID-based tracking.
fn ws_host_port(listen_url: &str) -> Option<(String, u16)> {
    let authority = listen_url.strip_prefix("ws://")?.split('/').next()?;
    let (host, port) = authority.rsplit_once(':')?;
    Some((host.to_string(), port.parse().ok()?))
}

/// After a spawn, poll until the listen port is served, then return the PID of
/// the process that actually owns it (the native app-server, even when it sits
/// behind an npm/node shim). Falls back to `spawn_pid` if the port never comes
/// up within [`SPAWN_RESOLVE_TIMEOUT`], so status honestly reports a server
/// that failed to start rather than silently mislabelling one.
fn resolve_listener_pid(host: &str, port: u16, listen_url: &str, spawn_pid: u32) -> u32 {
    let deadline = Instant::now() + SPAWN_RESOLVE_TIMEOUT;
    loop {
        if tcp_port_open(host, port) {
            return find_codex_app_server(listen_url).unwrap_or(spawn_pid);
        }
        if Instant::now() >= deadline {
            return spawn_pid;
        }
        thread::sleep(Duration::from_millis(150));
    }
}

/// Resolve the `codex` binary: the `explicit` path if it exists, otherwise
/// `codex` on `$PATH`. Returns `None` when neither resolves, so a caller (e.g.
/// the UI) can prompt the user to point at a binary before spawning. This is
/// the same resolution [`spawn`] performs internally, exposed so the path can
/// be shown / validated without starting a process.
pub fn locate_binary(explicit: Option<&str>) -> Option<PathBuf> {
    match explicit.map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => {
            let p = PathBuf::from(path);
            p.exists().then_some(p)
        },
        None => which::which("codex").ok(),
    }
}

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

    /// Byte offset in [`CodexProcessInfo::log_file`] just before this spawn — the
    /// point from which this run's log lines begin (the file is opened in append
    /// mode and shared across runs). A log tailer starts here to show this
    /// process's output without replaying earlier runs.
    pub log_offset: u64,
}

/// Spawn `codex app-server`, persist the resulting state and return a
/// report describing what was started.
///
/// If the previous run is still alive (PID matches and process exists)
/// the existing process is returned untouched.
pub fn spawn(opts: SpawnOptions) -> pocket_codex_core::Result<SpawnReport> {
    let mut state = RuntimeState::load()?;
    let listen_url = opts.listen.to_listen_url();
    let endpoint = ws_host_port(&listen_url);

    let log_file = opts
        .log_file
        .clone()
        .map(Ok)
        .unwrap_or_else(paths::codex_log_file)?;
    // The byte offset where this run's log lines will begin (the file is opened
    // in append mode and shared across runs). A tailer starts here to show only
    // this process's output, not earlier runs'.
    let log_offset = std::fs::metadata(&log_file).map(|m| m.len()).unwrap_or(0);

    // Reuse an app-server that is already serving the listen port. For a
    // websocket transport "is it up" is decided by the *port*, not a recorded
    // PID: when `codex` is an npm/node shim the PID we spawned is the shim
    // (which exits) while the native binary keeps the socket. Adopting the
    // live listener — ours from a prior run, or any other codex already bound
    // there — also makes `serve` idempotent instead of launching a second
    // codex that can't bind the port and dies, leaving the "stale codex /
    // working endpoint" split that this whole function exists to avoid.
    match &endpoint {
        Some((host, port)) if tcp_port_open(host, *port) => {
            // Adopt ONLY when the listener is genuinely a `codex … app-server`
            // bound to this URL. If something else holds the port,
            // find_codex_app_server returns None: refuse rather than register a
            // foreign service as our app-server (which `serve` would publish on
            // the relay and `status` would report alive).
            let Some(pid) = find_codex_app_server(&listen_url) else {
                return Err(pocket_codex_core::Error::Config(format!(
                    "{host}:{port} is already in use by a non-codex process; free the port or \
                     serve on a different --port"
                )));
            };
            // Keep the original start time if it's the same process we already
            // tracked; otherwise stamp now (best-effort for an adopted one).
            let started_at = state
                .codex
                .as_ref()
                .filter(|c| c.pid == pid)
                .map(|c| c.started_at.clone())
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            let info = CodexProcessInfo {
                pid,
                listen: listen_url,
                log_file,
                started_at,
            };
            state.codex = Some(info.clone());
            state.save()?;
            info!(pid, %host, port, "adopting codex app-server already on the listen port");
            return Ok(SpawnReport {
                info,
                reused: true,
                log_offset,
            });
        },
        // Unix-socket transport has no TCP port to probe — keep PID-based
        // reuse (the shim problem is websocket-specific).
        None => {
            if let Some(existing) = state.codex.clone() {
                if pid_alive(existing.pid) {
                    info!(pid = existing.pid, listen = %existing.listen, "codex already running");
                    return Ok(SpawnReport {
                        info: existing,
                        reused: true,
                        log_offset,
                    });
                }
                warn!(stale_pid = existing.pid, "previous codex process is gone, restarting");
            }
        },
        // Websocket port is free → fall through and spawn a fresh app-server.
        Some(_) => {
            if let Some(existing) = state.codex.as_ref() {
                warn!(stale_pid = existing.pid, "recorded codex is no longer serving, restarting");
            }
        },
    }

    let binary = match opts.binary.as_ref() {
        Some(path) => path.clone(),
        None => which::which("codex").map_err(|e| {
            pocket_codex_core::Error::Config(format!(
                "could not locate `codex` on $PATH ({e}); install codex or pass --codex-binary"
            ))
        })?,
    };

    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let log_handle_dup = log_handle.try_clone()?;

    debug!(?binary, %listen_url, ?log_file, "spawning codex app-server");

    let child = build_command(&binary, &listen_url, &opts.extra_args, opts.proxy.as_deref())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_handle))
        .stderr(Stdio::from(log_handle_dup))
        .spawn()
        .map_err(|e| {
            pocket_codex_core::Error::Config(format!("failed to spawn `{}`: {e}", binary.display()))
        })?;

    let spawn_pid = child.id();
    // Drop the Child handle so the kernel keeps the process alive after this
    // CLI exits; we track it by pid from now on.
    drop(child);

    // The PID we just spawned may be a throwaway shim. Wait for the listener to
    // come up and record the PID that actually owns it, so status/stop target
    // the real app-server rather than a wrapper that has already exited.
    let pid = match &endpoint {
        Some((host, port)) => resolve_listener_pid(host, *port, &listen_url, spawn_pid),
        None => spawn_pid,
    };

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
        log_offset,
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
    // The bundled binary IS the app-server (`codex-app-server --listen …`),
    // whereas an external `codex` exposes it as a subcommand (`codex app-server
    // --listen …`). Distinguish by file name so either can be spawned uniformly.
    let is_standalone = binary
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.starts_with("codex-app-server"));
    if !is_standalone {
        command.arg("app-server");
    }
    command.arg("--listen").arg(listen_url).args(extra_args);

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

    // Windows: `codex` is typically an npm shim (codex.cmd → node), and a
    // windowless GUI parent (the Flutter desktop app) launching a console child
    // makes Windows allocate a visible black console window that stays open for
    // the child's lifetime — and closing it would kill codex. CREATE_NO_WINDOW
    // suppresses that console while leaving the child fully functional; its
    // stdout/stderr are already redirected to the log file by the caller, so no
    // output is lost. `creation_flags` is a safe call (no `unsafe` needed), so
    // the crate's `#![forbid(unsafe_code)]` still holds. This is the single
    // command-construction site, so every spawn path (in-app host, watchdog
    // respawn, CLI serve) inherits the fix.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
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
    // "Alive" is whether a real codex app-server is serving the listen URL —
    // not merely whether the recorded PID exists (it may be a shim that exited
    // while the native binary keeps the socket) nor whether *anything* holds
    // the port (a foreign process must not read as our app-server). For
    // websocket transports require a `codex … app-server` bound to the URL;
    // fall back to the PID for unix sockets.
    let alive = state.codex.as_ref().is_some_and(|c| {
        if ws_host_port(&c.listen).is_some() {
            find_codex_app_server(&c.listen).is_some()
        } else {
            pid_alive(c.pid)
        }
    });
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

    // Target the process that actually owns the listen port — that's the real
    // app-server, which differs from the recorded PID when codex was launched
    // through a shim (killing the recorded shim PID would never free the port).
    // Fall back to the recorded PID for unix sockets or when the port is idle.
    let target = ws_host_port(&info.listen)
        .filter(|(host, port)| tcp_port_open(host, *port))
        .and_then(|_| find_codex_app_server(&info.listen))
        .unwrap_or(info.pid);

    let outcome = if pid_alive(target) {
        send_sigterm(target);
        StopOutcome::Stopped {
            pid: target,
        }
    } else {
        StopOutcome::StaleRecord {
            pid: target,
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
    fn locate_binary_prefers_an_existing_explicit_path() {
        // An explicit path that exists is used verbatim (here, the test binary).
        let exe = std::env::current_exe().expect("current exe");
        assert_eq!(locate_binary(Some(exe.to_str().expect("utf8 path"))), Some(exe.clone()));
        // A missing explicit path resolves to nothing, so the UI can prompt.
        assert_eq!(locate_binary(Some("/definitely/not/here/codex-xyz")), None);
        // A blank override is treated as "not given" → PATH lookup (which may or
        // may not find codex on the test host); assert only that it doesn't panic.
        let _ = locate_binary(Some("   "));
    }

    #[test]
    fn build_command_omits_subcommand_for_bundled_app_server() {
        let args = |bin: &str| -> Vec<String> {
            build_command(Path::new(bin), "ws://127.0.0.1:1", &[], None)
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        // External `codex` exposes app-server as a subcommand.
        assert_eq!(args("codex"), ["app-server", "--listen", "ws://127.0.0.1:1"]);
        // The bundled standalone binary IS the app-server — no subcommand.
        assert_eq!(args("codex-app-server"), ["--listen", "ws://127.0.0.1:1"]);
        assert_eq!(args("codex-app-server.exe"), ["--listen", "ws://127.0.0.1:1"]);
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

    #[test]
    fn ws_host_port_parses_websocket_urls_only() {
        assert_eq!(ws_host_port("ws://127.0.0.1:18080"), Some(("127.0.0.1".to_string(), 18080)));
        // Trailing path is ignored (authority only).
        assert_eq!(ws_host_port("ws://0.0.0.0:9000/foo"), Some(("0.0.0.0".to_string(), 9000)));
        // Unix sockets and missing/garbage ports have no TCP endpoint.
        assert_eq!(ws_host_port("unix:///tmp/codex.sock"), None);
        assert_eq!(ws_host_port("ws://127.0.0.1"), None);
        assert_eq!(ws_host_port("ws://127.0.0.1:notaport"), None);
    }
}

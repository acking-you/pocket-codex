//! Small process helpers shared by Pocket-Codex supervisors.
//!
//! The CLI tracks long-running `codex app-server` and pb-mapper worker
//! processes by PID in `state.toml`. These helpers keep the platform
//! checks and best-effort termination behaviour consistent across both
//! supervisors.

use std::{net::TcpStream, time::Duration};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tracing::warn;

/// Check whether a process id currently exists on this host.
pub fn pid_alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::new(),
    );
    sys.process(Pid::from_u32(pid)).is_some()
}

/// Whether something is currently accepting TCP connections on `host:port`.
///
/// This is the source of truth for "is the codex app-server actually up",
/// which is more reliable than a recorded PID: when `codex` is launched
/// through an npm/node shim (`codex.ps1 → node → codex.exe`) the PID we
/// spawned is the shim — it exits while the native binary keeps the socket.
/// Unspecified hosts (`0.0.0.0` / `::`) are probed over loopback.
pub fn tcp_port_open(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    let host = match host {
        "0.0.0.0" | "::" | "" => "127.0.0.1",
        other => other,
    };
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    let timeout = Duration::from_millis(400);
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
}

/// Does this process look like a native `codex app-server` launched with
/// `listen_url`? Matched by executable stem (`codex`) so the `node` /
/// `powershell` wrappers of an npm install are skipped: they carry the same
/// `app-server --listen …` arguments but are not the process holding the
/// socket. Split out from [`find_codex_app_server`] so the matching rule is
/// unit-testable without real processes.
fn matches_codex_app_server(exe_stem: &str, cmd: &[String], listen_url: &str) -> bool {
    exe_stem.eq_ignore_ascii_case("codex")
        && cmd.iter().any(|a| a == "app-server")
        && cmd.iter().any(|a| a.contains(listen_url))
}

/// PID of the native `codex app-server` process serving `listen_url`, if one
/// is running. This is what keeps status/stop/`serve` correct when `codex`
/// resolves to an npm/node shim: [`std::process::Command`] only sees the
/// shim's PID (which exits), while the native binary several layers down keeps
/// the listener. Returns the first match (only one process can hold the port).
pub fn find_codex_app_server(listen_url: &str) -> Option<u32> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    sys.processes().values().find_map(|p| {
        let exe = p.exe().map(std::path::Path::to_path_buf);
        let stem = exe
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new(p.name()))
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let cmd: Vec<String> = p
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        matches_codex_app_server(&stem, &cmd, listen_url).then(|| p.pid().as_u32())
    })
}

/// Send `SIGTERM` to a process id, logging but not surfacing errors.
#[cfg(unix)]
pub fn send_sigterm(pid: u32) {
    use nix::{
        sys::signal::{kill, Signal},
        unistd::Pid as NixPid,
    };

    let nix_pid = NixPid::from_raw(pid as i32);
    if let Err(e) = kill(nix_pid, Signal::SIGTERM) {
        warn!(pid, error = %e, "failed to SIGTERM process");
    }
}

/// Send `SIGTERM` to a process id, logging but not surfacing errors.
///
/// Windows has no SIGTERM; best-effort termination goes through
/// sysinfo's `Process::kill` (TerminateProcess) instead.
#[cfg(not(unix))]
pub fn send_sigterm(pid: u32) {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::new(),
    );
    match sys.process(Pid::from_u32(pid)) {
        Some(process) => {
            if !process.kill() {
                warn!(pid, "failed to terminate process");
            }
        },
        None => warn!(pid, "process not found; nothing to terminate"),
    }
}

/// Forcefully terminate a process id, returning whether the kill signal
/// was delivered.
///
/// Unlike [`send_sigterm`], this is an immediate, non-graceful kill
/// (`SIGKILL` on unix, `TerminateProcess` on Windows, both via sysinfo's
/// [`sysinfo::Process::kill`]). It is the primitive behind a *force
/// takeover*, where Pocket-Codex evicts another codex process that is
/// holding a session's rollout file open so it can resume that session
/// itself. Returns `false` when the process is already gone or the kill
/// could not be delivered; callers treat that as best-effort (the
/// takeover proceeds regardless).
pub fn force_kill(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::new(),
    );
    match sys.process(Pid::from_u32(pid)) {
        Some(process) => process.kill(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn tcp_port_open_sees_a_live_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        assert!(tcp_port_open("127.0.0.1", port), "bound port should read open");
        // Unspecified host is probed over loopback, so it sees the same listener.
        assert!(tcp_port_open("0.0.0.0", port));
    }

    #[test]
    fn tcp_port_open_false_when_nothing_listens() {
        // Bind then drop to learn a port nobody is listening on.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr").port()
        };
        assert!(!tcp_port_open("127.0.0.1", port));
    }

    #[test]
    fn matches_native_codex_app_server_only() {
        let url = "ws://127.0.0.1:18080";
        let cmd = |argv: &[&str]| argv.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Native binary: stem `codex`, carries `app-server` + our listen url.
        assert!(matches_codex_app_server(
            "codex",
            &cmd(&["codex", "app-server", "--listen", url]),
            url,
        ));
        // The npm/node shim carries identical args but is named `node` — skip it.
        assert!(!matches_codex_app_server(
            "node",
            &cmd(&["node", "codex.js", "app-server", "--listen", url]),
            url,
        ));
        // Right binary, different port → not ours.
        assert!(!matches_codex_app_server(
            "codex",
            &cmd(&["codex", "app-server", "--listen", "ws://127.0.0.1:9999"]),
            url,
        ));
        // Right binary + url but not an app-server invocation.
        assert!(
            !matches_codex_app_server("codex", &cmd(&["codex", "exec", "--listen", url]), url,)
        );
    }
}

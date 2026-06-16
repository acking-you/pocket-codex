//! Small process helpers shared by Pocket-Codex supervisors.
//!
//! The CLI tracks long-running `codex app-server` and pb-mapper worker
//! processes by PID in `state.toml`. These helpers keep the platform
//! checks and best-effort termination behaviour consistent across both
//! supervisors.

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
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

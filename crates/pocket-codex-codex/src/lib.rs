//! Process management and protocol bridge for the upstream
//! `codex app-server`.
//!
//! Pocket-Codex spawns the user's existing `codex` binary in
//! `app-server` mode and exposes a typed handle that the CLI / UI
//! layers can use to:
//!
//! * locate the `codex` binary on `$PATH`,
//! * spawn `codex app-server --listen <url>` as a detached child and stream its
//!   stdout/stderr to a log file,
//! * inspect supervised processes (alive? same pid?) and gracefully stop them.
//!
//! JSON-RPC 2.0 envelopes that the app-server speaks are defined in
//! [`protocol`]; this crate stays transport-agnostic so callers can
//! decide whether to talk over stdio, a unix socket, or a websocket.

#![forbid(unsafe_code)]
/// Legacy built-in version marker. No Codex runtime is bundled with the app.
pub const EMBEDDED_CODEX_COMMIT: &str = "unavailable";

/// JSON-RPC 2.0 envelopes used by the codex app-server.
pub mod protocol;

/// Async WebSocket JSON-RPC client for a `codex app-server`.
pub mod client;

/// Spawn / inspect / stop the supervised `codex app-server` process.
pub mod process;

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
mod shell_environment;

/// Verify a freshly-spawned app-server actually serves (`/readyz`), so
/// launch commands can fail fast with the real error instead of printing
/// success for a child that died on boot.
pub mod readiness;

/// Read codex session rollout files from `CODEX_HOME` and classify their
/// most-recent-turn state.
pub mod rollout;

/// Bootstrap `CODEX_HOME` (custom provider / non-degraded prompt) from the
/// onboarding UI.
pub mod setup;

/// Detect whether a session's rollout is currently held open by a live
/// process, and enumerate codex app-server processes.
pub mod liveness;

/// Combine transcript + liveness into a resume-safety verdict and
/// implement force takeover of a held-open session.
pub mod takeover;

pub use process::{
    locate_binary, spawn, status, stop, ListenSpec, SpawnOptions, SpawnReport, StatusReport,
    StopOutcome,
};

/// The `PATH` to search for, and hand to, other external tool executables a
/// GUI host launches (ACP agents): on macOS, the inherited `PATH` followed by
/// directories only the login shell adds — the same lazily resolved value
/// Codex children receive; elsewhere `None`, meaning inherit unchanged.
///
/// It is applied to the child's environment only; this process's own
/// environment is never modified.
pub fn external_tool_path() -> Option<std::ffi::OsString> {
    #[cfg(target_os = "macos")]
    {
        shell_environment::child_path()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
pub use readiness::{
    spawn_ready, verify_ready, wait_for_readyz, SpawnReadyError, StartupFailure, READY_TIMEOUT,
};

/// Whether the child process `pid` of this process has exited, **without
/// reaping it**: its exit status stays collectable, so its pid (and the
/// process-group id it leads) cannot be reused until the owner reaps it.
/// That lets an owner notice the exit and still signal the group safely
/// before reaping.
///
/// `Some(false)` while it runs, `Some(true)` once it exited, `None` when
/// unknown (not a child of this process, already reaped, or no Unix).
pub fn child_exited_unreaped(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        use rustix::process::{waitid, Pid, WaitId, WaitIdOptions};
        let pid = Pid::from_raw(i32::try_from(pid).ok()?)?;
        loop {
            match waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ) {
                Ok(Some(_)) => return Some(true),
                Ok(None) => return Some(false),
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return None,
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

#[cfg(all(test, unix))]
mod child_exit_tests {
    #[test]
    fn exit_is_seen_without_reaping_the_child() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .expect("spawn");
        let pid = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while super::child_exited_unreaped(pid) == Some(false)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(super::child_exited_unreaped(pid), Some(true));
        // Still collectable: nothing reaped it.
        assert_eq!(child.wait().expect("wait").code(), Some(7));
    }
}

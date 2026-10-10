//! The owned agent subprocess and its cleanup.
//!
//! The agent starts in a new process group whose id is its own pid. The
//! leader is never reaped until the whole group has been signalled: while it
//! remains an unreaped child its pid (and therefore the group id) cannot be
//! reused, so the delayed group signals can never reach an unrelated process.
//! Only after `SIGTERM`, a short grace and `SIGKILL` to the group is the
//! leader waited for.
//!
//! A descendant that moves itself into another process group or session
//! escapes group signalling; that is a documented limitation shared with the
//! Codex login-shell probe.
//!
//! Owned hosting is only implemented on Unix. On Windows [`spawn`] refuses
//! before starting anything, because this crate has no ownership-safe way to
//! clean up an agent's process tree there.

#[cfg(unix)]
use std::process::Stdio;
use std::{ffi::OsString, path::Path, time::Duration};

use tokio::process::{Child, ChildStdin, ChildStdout};
#[cfg(unix)]
use tokio::{io::AsyncReadExt, process::Command};

/// Why the agent could not be started.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpawnError {
    /// Owned ACP hosting is not available on this platform.
    #[error("hosting ACP agents is not supported on this platform yet")]
    UnsupportedPlatform,
    /// The operating system refused to start the program.
    #[error("the agent executable could not be started")]
    Start,
}

/// A running agent and its pipes.
pub struct Spawned {
    /// The owned process.
    pub process: AgentProcess,
    /// The agent's input.
    pub stdin: ChildStdin,
    /// The agent's output.
    pub stdout: ChildStdout,
}

/// An owned agent process (the leader of its own process group).
pub struct AgentProcess {
    child: Option<Child>,
    group: Option<i32>,
}

/// Start `program` with `args` verbatim. `path` replaces only the child's
/// `PATH`; `stderr` is drained and discarded (it may echo secrets).
#[cfg(unix)]
pub fn spawn(
    program: &Path,
    args: &[String],
    path: Option<&OsString>,
) -> Result<Spawned, SpawnError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A new group, set before exec, so the agent's tree can be signalled
        // without touching the app's own group.
        .process_group(0)
        .kill_on_drop(false);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command.spawn().map_err(|_| SpawnError::Start)?;
    let group = child.id().and_then(|pid| i32::try_from(pid).ok());
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let mut process = AgentProcess {
            child: Some(child),
            group,
        };
        process.kill_now();
        return Err(SpawnError::Start);
    };
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut sink = [0u8; 8192];
            while matches!(stderr.read(&mut sink).await, Ok(n) if n > 0) {}
        });
    }
    Ok(Spawned {
        process: AgentProcess {
            child: Some(child),
            group,
        },
        stdin,
        stdout,
    })
}

/// Owned hosting is unavailable on this platform.
#[cfg(not(unix))]
pub fn spawn(
    _program: &Path,
    _args: &[String],
    _path: Option<&OsString>,
) -> Result<Spawned, SpawnError> {
    Err(SpawnError::UnsupportedPlatform)
}

/// Whether this platform can host ACP agents.
pub const fn hosting_supported() -> bool {
    cfg!(unix)
}

/// Whether the agent leader `pid` (a child of this process that the owner
/// has not reaped yet) has exited. The check does not reap, so the group it
/// led can still be signalled safely afterwards. An answer that cannot be
/// determined counts as exited, so a lost leader never looks alive.
///
/// This catches an exit that leaves the output pipe open because a
/// descendant inherited it.
pub fn leader_exited(pid: u32) -> bool {
    !matches!(pocket_codex_codex::child_exited_unreaped(pid), Some(false))
}

impl AgentProcess {
    /// The leader's process id, while it has not been reaped.
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    #[cfg(unix)]
    fn signal_group(&self, signal: nix::sys::signal::Signal) {
        if let Some(group) = self.group {
            // Safe against pid reuse: the leader is still unreaped here.
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), signal);
        }
    }

    /// Signal the group with `SIGKILL` and the leader directly, without
    /// reaping (used where awaiting is impossible).
    fn kill_now(&mut self) {
        #[cfg(unix)]
        self.signal_group(nix::sys::signal::Signal::SIGKILL);
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }

    /// Terminate the group (`SIGTERM`, `grace`, `SIGKILL`) and then reap the
    /// leader. Call after the agent's input was closed and its output had a
    /// chance to end.
    pub async fn terminate(mut self, grace: Duration) {
        #[cfg(unix)]
        {
            self.signal_group(nix::sys::signal::Signal::SIGTERM);
            tokio::time::sleep(grace).await;
            self.signal_group(nix::sys::signal::Signal::SIGKILL);
        }
        // No group to signal where owned hosting is unavailable.
        #[cfg(not(unix))]
        let _ = grace;
        let Some(mut child) = self.child.take() else { return };
        // The leader may have left its group in a startup script.
        let _ = child.start_kill();
        if tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .is_err()
        {
            tracing::warn!("the ACP agent did not exit after SIGKILL; leaving it to the reaper");
        }
        self.group = None;
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        // Only reached when `terminate` was skipped (a panic or an aborted
        // task). Dropping the `Child` hands it to Tokio's orphan reaper, which
        // runs after these signals.
        if self.child.is_some() {
            self.kill_now();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn alive(pid: i32) -> bool {
        // Signal 0 checks existence; a reaped pid reports ESRCH.
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
    }

    #[tokio::test]
    async fn terminate_kills_the_whole_group_and_reaps_the_leader() {
        let script = "sleep 30 & echo $! ; wait";
        let Spawned {
            process,
            stdin,
            mut stdout,
        } = spawn(Path::new("/bin/sh"), &["-c".into(), script.into()], None).expect("spawn");
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while stdout.read(&mut byte).await.expect("read") == 1 && byte[0] != b'\n' {
            line.push(byte[0]);
        }
        let grandchild: i32 = String::from_utf8(line)
            .expect("utf8")
            .trim()
            .parse()
            .expect("pid");
        let leader = process.id().expect("pid") as i32;
        assert!(alive(grandchild));
        drop(stdin);
        process.terminate(Duration::from_millis(100)).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (alive(grandchild) || alive(leader)) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive(leader), "leader reaped");
        assert!(!alive(grandchild), "descendant in the group killed");
    }

    #[tokio::test]
    async fn a_leader_exit_is_seen_while_a_descendant_holds_the_output_open() {
        // The leader prints its descendant's pid and exits; the descendant
        // keeps the inherited stdout pipe open, so no EOF ever arrives.
        let script = "sleep 30 & echo $! ; exit 0";
        let Spawned {
            process,
            stdin: _stdin,
            mut stdout,
        } = spawn(Path::new("/bin/sh"), &["-c".into(), script.into()], None).expect("spawn");
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while stdout.read(&mut byte).await.expect("read") == 1 && byte[0] != b'\n' {
            line.push(byte[0]);
        }
        let descendant: i32 = String::from_utf8(line)
            .expect("utf8")
            .trim()
            .parse()
            .expect("pid");
        let leader = process.id().expect("pid");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !leader_exited(leader) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(leader_exited(leader), "the exit is visible without EOF");
        assert!(alive(descendant), "the descendant still holds the pipe");
        assert_eq!(process.id(), Some(leader), "and the leader is still unreaped");
        process.terminate(Duration::from_millis(100)).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while alive(descendant) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive(descendant), "the group was cleaned up through the unreaped leader");
    }

    #[tokio::test]
    async fn a_missing_program_fails_to_start() {
        assert_eq!(
            spawn(Path::new("/definitely/not/here"), &[], None).err(),
            Some(SpawnError::Start)
        );
    }
}

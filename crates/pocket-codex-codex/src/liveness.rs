//! Detect whether a session's rollout file is currently *owned by a live
//! process*, so Pocket-Codex never resumes a thread another codex client
//! (e.g. the desktop app) still has loaded.
//!
//! codex keeps a rollout file open for the whole time a thread is loaded
//! — including while it sits idle awaiting the next user input, not just
//! while a turn runs. So "is this rollout held open?" answers the real
//! ownership question: a `task_complete` in the transcript means the
//! *turn* finished, but if the file is still held open the owning process
//! could start a new turn at any moment, and resuming it from a second
//! app-server would make two writers append to one rollout. See
//! [`crate::takeover`] for how this combines with [`crate::rollout`].
//!
//! ## Platform strategy
//!
//! * **Windows** — there is no advisory lock, but a process holding the file
//!   open prevents a *no-sharing* (`share_mode(0)`) open by anyone else, so
//!   [`is_held_open`] probes that directly. Naming the exact holder PID would
//!   need the Restart Manager FFI, which this `#![forbid(unsafe_code)]` crate
//!   avoids, so [`file_holders`] returns nothing there and [`crate::takeover`]
//!   falls back to the codex app-server candidate pool from
//!   [`codex_app_server_processes`].
//! * **Unix** — opening never reports a sharing violation, so liveness is
//!   derived from `lsof`, which also yields the exact holder PIDs.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// A process relevant to a rollout's liveness: either a confirmed holder
/// of the file (unix) or a codex app-server candidate (the kill pool).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// Operating-system process id.
    pub pid: u32,
    /// Process image name (e.g. `codex` / `codex.exe`).
    pub name: String,
}

/// Whether `path` is currently held open by some live process.
///
/// On Windows this is a precise, point-in-time probe. On unix it is
/// derived from [`file_holders`] (i.e. `lsof`); if `lsof` is unavailable
/// it conservatively reports `false` (cannot prove ownership).
#[cfg(windows)]
pub fn is_held_open(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    // Win32 error codes for "another process has the file open with an
    // incompatible share mode".
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    // Request an open that forbids sharing: it succeeds only if no other
    // handle to the file currently exists. A sharing/lock violation means
    // someone holds it open right now.
    match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
    {
        Ok(_) => false,
        Err(err) => {
            matches!(err.raw_os_error(), Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION))
        },
    }
}

/// Whether `path` is currently held open by some live process (unix:
/// derived from `lsof`).
///
/// Fail-closed: if `lsof` cannot be run (missing binary, spawn error) we
/// cannot prove the rollout is free, so we report it as held rather than risk
/// resuming a session another codex still owns and creating two writers. A
/// successful `lsof` run with no holders reports `false` (genuinely free).
#[cfg(not(windows))]
pub fn is_held_open(path: &Path) -> bool {
    match lsof_pids(path) {
        Some(pids) => !pids.is_empty(),
        None => true,
    }
}

/// The processes currently holding `path` open, with their PIDs.
///
/// Precise on unix (via `lsof`); empty on Windows, where naming the
/// holder would require the Restart Manager FFI (see the module docs).
#[cfg(not(windows))]
pub fn file_holders(path: &Path) -> Vec<Holder> {
    use sysinfo::Pid;

    let pids = lsof_pids(path).unwrap_or_default();
    if pids.is_empty() {
        return Vec::new();
    }
    let mut sys = System::new();
    let sys_pids: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&sys_pids),
        true,
        ProcessRefreshKind::new(),
    );
    pids.into_iter()
        .map(|pid| Holder {
            pid,
            name: sys
                .process(Pid::from_u32(pid))
                .map(|p| p.name().to_string_lossy().into_owned())
                .unwrap_or_default(),
        })
        .collect()
}

/// The processes currently holding `path` open (empty on Windows — see
/// the module docs).
#[cfg(windows)]
pub fn file_holders(_path: &Path) -> Vec<Holder> {
    Vec::new()
}

/// Which of `paths` are currently held open by a live process, returned as the
/// subset that is held.
///
/// Batches the probe so listing many sessions costs O(1) external calls rather
/// than one per path: a single `lsof` field-mode invocation on unix (spawning
/// dozens of `lsof` in a loop blocked the UI for seconds), and the
/// subprocess-free sharing probe per path on Windows.
#[cfg(windows)]
pub fn held_open_paths(paths: &[PathBuf]) -> HashSet<PathBuf> {
    // Windows `is_held_open` is a cheap in-process open(); no batching needed.
    paths.iter().filter(|p| is_held_open(p)).cloned().collect()
}

/// Which of `paths` are currently held open by a live process (unix: one
/// batched `lsof`). Matched back to inputs by file name — rollout filenames
/// carry a unique UUID, so this is unambiguous. Fail-closed: if `lsof` cannot
/// be run, every path is reported held (see [`is_held_open`]).
#[cfg(not(windows))]
pub fn held_open_paths(paths: &[PathBuf]) -> HashSet<PathBuf> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut cmd = std::process::Command::new("lsof");
    cmd.arg("-F").arg("n").arg("--");
    for p in paths {
        cmd.arg(p);
    }
    let output = match cmd.output() {
        Ok(o) => o,
        // lsof unavailable → cannot prove anything free; fail-closed.
        Err(_) => return paths.iter().cloned().collect(),
    };
    let open_names: HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter_map(|name| Path::new(name).file_name().and_then(|f| f.to_str()))
        .map(str::to_string)
        .collect();
    paths
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|n| open_names.contains(n))
        })
        .cloned()
        .collect()
}

/// Run `lsof -t -- <path>` and parse the holder PIDs.
///
/// `None` means `lsof` could not be run at all (missing binary / spawn error)
/// — the holder set is *unknown*, which callers treat as fail-closed.
/// `Some(pids)` means `lsof` ran; the (possibly empty) vec is the holder set.
/// A non-zero `lsof` exit (e.g. "no process has it open") still counts as a
/// successful determination, so only a spawn failure yields `None`.
#[cfg(not(windows))]
fn lsof_pids(path: &Path) -> Option<Vec<u32>> {
    let output = std::process::Command::new("lsof")
        .arg("-t")
        .arg("--")
        .arg(path)
        .output()
        .ok()?;
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .collect(),
    )
}

/// Enumerate every running `codex … app-server` process on this host.
///
/// This is the candidate pool a *force takeover* may terminate to release
/// a held-open rollout: on Windows, where [`file_holders`] cannot name
/// the exact holder, [`crate::takeover`] kills from this pool (re-probing
/// after each kill to stop as soon as the file is released). The match is
/// intentionally narrow — an image named like `codex` *and* an
/// `app-server` argument — so it never sweeps up the Electron desktop GUI
/// shell (whose command line has no `app-server`).
pub fn codex_app_server_processes() -> Vec<Holder> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_cmd(UpdateKind::Always),
    );
    let mut out = Vec::new();
    for (pid, process) in sys.processes() {
        let name = process.name().to_string_lossy();
        let cmd = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        if looks_like_app_server(&name, &cmd) {
            out.push(Holder {
                pid: pid.as_u32(),
                name: name.into_owned(),
            });
        }
    }
    out
}

/// Whether a `(process name, joined command line)` pair looks like a
/// `codex app-server`: a codex-named image carrying an `app-server`
/// argument. The `app-server` requirement is what excludes the Electron
/// desktop shell (also named `Codex.exe`) and any unrelated codex
/// subcommand (`codex exec`, `codex login`, …).
fn looks_like_app_server(name: &str, cmd_joined: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let is_codex = name == "codex" || name == "codex.exe" || name.starts_with("codex");
    is_codex && cmd_joined.contains("app-server")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_codex_app_server_command() {
        assert!(looks_like_app_server("codex", "codex app-server --listen ws://127.0.0.1:18080"));
        assert!(looks_like_app_server(
            "codex.exe",
            r#"C:\…\codex.exe app-server --analytics-default-enabled"#
        ));
    }

    #[test]
    fn rejects_desktop_gui_shell_and_other_subcommands() {
        // The Electron desktop shell is also named Codex.exe but its command
        // line has no `app-server`.
        assert!(!looks_like_app_server("Codex.exe", r#"C:\…\Codex.exe --type=renderer"#));
        // Other codex subcommands are not app-servers.
        assert!(!looks_like_app_server("codex", "codex exec 'do a thing'"));
        // Unrelated processes never match.
        assert!(!looks_like_app_server("node", "node server.js app-server"));
    }
}

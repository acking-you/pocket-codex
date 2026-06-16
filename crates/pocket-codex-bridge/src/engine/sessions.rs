//! Local codex session inventory and *force takeover*, layered on the
//! `pocket-codex-codex` rollout / liveness / takeover primitives.
//!
//! These read `CODEX_HOME` and inspect/terminate local processes, so they
//! act on the machine the bridge runs on (the host that owns the
//! sessions). In Pocket-Codex's desktop setup the Flutter app runs
//! alongside the codex desktop app, so "list the sessions under this
//! CODEX_HOME and tell me which are safe to resume" is answered locally.
//!
//! The two-signal model (see `pocket_codex_codex::takeover`):
//!
//! * a session whose latest turn is *running* and whose rollout is *held open*
//!   by a live process must stay read-only;
//! * a *finished* session whose rollout is *free* is plainly resumable;
//! * a finished session still *held open* by another codex app-server (e.g. the
//!   desktop app) is resumable only via a confirmed [`force_resume`], which
//!   best-effort evicts the holder first.

use anyhow::{anyhow, Result};
use pocket_codex_codex::{liveness::Holder, rollout, takeover};
use pocket_codex_core::state::RuntimeState;

use crate::engine::app_session;

/// One session discovered under `CODEX_HOME`, with the state the UI needs
/// to render it read-only or resumable.
#[derive(Clone, Debug)]
pub struct LocalSession {
    /// Thread / conversation id.
    pub thread_id: String,
    /// Working directory the session controls, when recorded.
    pub cwd: Option<String>,
    /// Best-effort first-user-message preview.
    pub preview: String,
    /// Originating client (`cli` / `vscode` / …), when recorded.
    pub source: Option<String>,
    /// Last-modified time of the rollout, unix seconds.
    pub updated_at: i64,
    /// Most-recent-turn state tag (`empty`/`completed`/`aborted`/`incomplete`).
    pub turn_state: String,
    /// Whether the rollout is currently held open by a live process.
    pub held_open: bool,
    /// Resume-safety tag (`resumable`/`resumableUnfinished`/`ownedRunning`/
    /// `ownedIdle`).
    pub safety: String,
    /// Whether the UI may offer a resume action (false only while a turn is
    /// actively running).
    pub allows_resume: bool,
    /// Whether resuming requires a force takeover (a live owner must be
    /// evicted first).
    pub requires_takeover: bool,
}

/// A single session's liveness detail, including the processes a force
/// takeover would target.
#[derive(Clone, Debug)]
pub struct SessionLivenessView {
    /// Thread / conversation id.
    pub thread_id: String,
    /// Most-recent-turn state tag.
    pub turn_state: String,
    /// Whether the rollout is currently held open.
    pub held_open: bool,
    /// Resume-safety tag.
    pub safety: String,
    /// Whether the UI may offer a resume action.
    pub allows_resume: bool,
    /// Whether resuming requires a force takeover.
    pub requires_takeover: bool,
    /// Processes a force takeover would attempt to terminate (already
    /// excluding Pocket-Codex's own app-server).
    pub holders: Vec<Holder>,
}

/// Outcome of a [`force_resume`].
#[derive(Clone, Debug)]
pub struct ForceResumeOutcome {
    /// Holders that were successfully terminated.
    pub killed: Vec<Holder>,
    /// Holders the kill could not reach (already gone, denied, or
    /// respawned-and-distinct).
    pub survived: Vec<Holder>,
    /// Whether the rollout is still held open after the eviction attempt
    /// (the resume proceeds regardless — an accepted risk).
    pub still_held: bool,
    /// Whether the subsequent `thread/resume` into our app-server
    /// succeeded.
    pub resumed: bool,
    /// The resume error message, when `resumed` is false.
    pub resume_error: Option<String>,
}

/// PIDs that a force takeover must never terminate: Pocket-Codex's own
/// supervised app-server (the very server we resume into) and the current
/// process.
fn protected_pids() -> Vec<u32> {
    let mut pids = vec![std::process::id()];
    if let Ok(state) = RuntimeState::load() {
        if let Some(codex) = state.codex {
            pids.push(codex.pid);
        }
    }
    pids
}

/// List every session under `CODEX_HOME`, newest first, each annotated
/// with its resume-safety state. Reads only the local filesystem and
/// process table; no app-server connection is required.
pub fn list_local_sessions() -> Result<Vec<LocalSession>> {
    let sessions = rollout::scan_sessions().map_err(|e| anyhow!("scanning sessions: {e}"))?;
    let mut out = Vec::with_capacity(sessions.len());
    for info in sessions {
        let held_open = pocket_codex_codex::liveness::is_held_open(&info.rollout_path);
        let safety = takeover::classify(&info.turn_state, held_open);
        out.push(LocalSession {
            thread_id: info.thread_id,
            cwd: info.cwd,
            preview: info.preview,
            source: info.source,
            updated_at: info.updated_at,
            turn_state: info.turn_state.tag().to_string(),
            held_open,
            safety: safety.tag().to_string(),
            allows_resume: safety.allows_resume(),
            requires_takeover: safety.requires_takeover(),
        });
    }
    Ok(out)
}

/// Inspect one session's liveness in detail, listing the would-be
/// takeover targets (with Pocket-Codex's own app-server excluded).
pub fn session_liveness(thread_id: &str) -> Result<SessionLivenessView> {
    let path = rollout::rollout_path_for_thread(thread_id)
        .map_err(|e| anyhow!("locating rollout: {e}"))?
        .ok_or_else(|| anyhow!("no rollout found for thread {thread_id}"))?;
    let live = takeover::inspect(&path).map_err(|e| anyhow!("inspecting rollout: {e}"))?;
    let protected = protected_pids();
    let holders = live
        .holders
        .into_iter()
        .filter(|h| !protected.contains(&h.pid))
        .collect();
    Ok(SessionLivenessView {
        thread_id: thread_id.to_string(),
        turn_state: live.turn_state.tag().to_string(),
        held_open: live.held_open,
        safety: live.safety.tag().to_string(),
        allows_resume: live.safety.allows_resume(),
        requires_takeover: live.safety.requires_takeover(),
        holders,
    })
}

/// Force-resume a session into the app-server behind `service_key`.
///
/// Best-effort evicts every live process holding the session's rollout
/// open (except Pocket-Codex's own app-server), then issues
/// `thread/resume` regardless of whether the eviction fully succeeded —
/// the caller has already confirmed they accept the consequences. The
/// returned [`ForceResumeOutcome`] reports exactly what happened.
///
/// The caller is responsible for gating this on user confirmation and for
/// not offering it while a turn is actively running (see
/// [`SessionLivenessView::allows_resume`]).
pub fn force_resume(service_key: &str, thread_id: &str) -> Result<ForceResumeOutcome> {
    let path = rollout::rollout_path_for_thread(thread_id)
        .map_err(|e| anyhow!("locating rollout: {e}"))?
        .ok_or_else(|| anyhow!("no rollout found for thread {thread_id}"))?;
    let protected = protected_pids();
    let report = takeover::force_release(&path, &protected);

    let (resumed, resume_error) = match app_session::thread_resume(service_key, thread_id) {
        Ok(()) => (true, None),
        Err(err) => (false, Some(err.to_string())),
    };

    Ok(ForceResumeOutcome {
        killed: report.killed,
        survived: report.survived,
        still_held: report.still_held,
        resumed,
        resume_error,
    })
}

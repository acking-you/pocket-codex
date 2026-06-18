//! Combine a session's transcript state ([`crate::rollout`]) with its
//! liveness ([`crate::liveness`]) into a single resume-safety verdict,
//! and implement *force takeover*: evicting whatever live process holds a
//! session's rollout open so Pocket-Codex's own app-server can resume it.
//!
//! ## Why a force takeover exists
//!
//! Resuming a thread appends to its existing rollout file. If a second
//! process (e.g. the desktop app) still has that thread loaded, both
//! would append to one file and diverge — so a resume is only safe when
//! no live process owns the session. The clean cases ([`ResumeSafety`])
//! cover that. But a user may *knowingly* want to pull a session that the
//! desktop app still holds onto their phone; for that, [`force_release`]
//! makes a best-effort attempt to terminate the holders first. It is
//! deliberately best-effort: if a holder cannot be killed (or the desktop
//! app respawns its app-server), the caller resumes anyway — the user has
//! already accepted the consequences.

use std::{path::Path, time::Duration};

use pocket_codex_core::{process::force_kill, Result};

use crate::{
    liveness::{self, Holder},
    rollout::{self, TurnState},
};

/// How long to wait after a kill for the OS to tear down the dead
/// process's file handles before re-probing whether the rollout is still
/// held.
const RELEASE_SETTLE: Duration = Duration::from_millis(150);

/// Whether — and how safely — a session can be resumed by Pocket-Codex's
/// app-server, given who currently owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSafety {
    /// No live process owns the session and its last turn finished —
    /// safe to resume directly.
    Resumable,
    /// No live owner, but the last turn never reached a terminal marker
    /// (it crashed or was interrupted and then released). Resumable, but
    /// the caller should surface that the previous turn was left
    /// unfinished.
    ResumableUnfinished,
    /// A live process owns the session and a turn is running right now —
    /// the UI must stay read-only.
    OwnedRunning,
    /// A live process still has the (finished) session loaded — read-only
    /// by default, but eligible for a confirmed [`force_release`] +
    /// resume.
    OwnedIdle,
}

impl ResumeSafety {
    /// Whether the UI may offer a resume action. Resume is offered for
    /// every state except [`ResumeSafety::OwnedRunning`]: a session whose
    /// turn is actively running must stay read-only (the user resumes
    /// once it finishes). For [`ResumeSafety::OwnedIdle`] the resume is a
    /// *force* takeover.
    pub fn allows_resume(&self) -> bool {
        !matches!(self, ResumeSafety::OwnedRunning)
    }

    /// Whether resuming requires evicting a live owner first (i.e. a
    /// force takeover rather than a plain resume).
    pub fn requires_takeover(&self) -> bool {
        matches!(self, ResumeSafety::OwnedIdle)
    }

    /// Stable camelCase tag for FFI / UI.
    pub fn tag(&self) -> &'static str {
        match self {
            ResumeSafety::Resumable => "resumable",
            ResumeSafety::ResumableUnfinished => "resumableUnfinished",
            ResumeSafety::OwnedRunning => "ownedRunning",
            ResumeSafety::OwnedIdle => "ownedIdle",
        }
    }
}

/// Derive the [`ResumeSafety`] from a transcript turn state and whether
/// the rollout is currently held open by a live process.
pub fn classify(turn_state: &TurnState, held_open: bool) -> ResumeSafety {
    match (held_open, turn_state.is_finished()) {
        (true, false) => ResumeSafety::OwnedRunning,
        (true, true) => ResumeSafety::OwnedIdle,
        (false, true) => ResumeSafety::Resumable,
        (false, false) => ResumeSafety::ResumableUnfinished,
    }
}

/// A session's combined transcript + liveness snapshot.
#[derive(Debug, Clone)]
pub struct SessionLiveness {
    /// Classified state of the most recent turn (from the transcript).
    pub turn_state: TurnState,
    /// Whether the rollout is currently held open by a live process.
    pub held_open: bool,
    /// The resume-safety verdict.
    pub safety: ResumeSafety,
    /// The processes a force takeover would attempt to terminate (empty
    /// when the rollout is not held). Precise on unix; on Windows this is
    /// the codex app-server candidate pool (see [`crate::liveness`]).
    pub holders: Vec<Holder>,
}

/// Inspect one rollout: classify its turn, probe liveness, and compute
/// the resume-safety verdict plus the would-be takeover targets.
pub fn inspect(rollout_path: &Path) -> Result<SessionLiveness> {
    let turn_state = rollout::classify_turn_state(rollout_path)?;
    let held_open = liveness::is_held_open(rollout_path);
    let safety = classify(&turn_state, held_open);
    let holders = if held_open { resume_targets(rollout_path) } else { Vec::new() };
    Ok(SessionLiveness {
        turn_state,
        held_open,
        safety,
        holders,
    })
}

/// The processes a [`force_release`] would attempt to terminate for a
/// held-open rollout — the *raw* set, before any [`force_release`] exclusions.
///
/// On unix this is the `lsof` holders **restricted to codex app-servers**: a
/// rollout can also be held by an editor, `tail -f`, or a backup/indexer, and
/// confirming a codex takeover does not authorize killing those. On Windows,
/// where the exact holder cannot be named without FFI, this is the codex
/// app-server candidate pool, but only when the file is actually held.
#[cfg(not(windows))]
pub fn resume_targets(rollout_path: &Path) -> Vec<Holder> {
    use std::collections::HashSet;
    let codex: HashSet<u32> = liveness::codex_app_server_processes()
        .into_iter()
        .map(|h| h.pid)
        .collect();
    liveness::file_holders(rollout_path)
        .into_iter()
        .filter(|h| codex.contains(&h.pid))
        .collect()
}

/// The processes a [`force_release`] would attempt to terminate (Windows:
/// the codex app-server candidate pool when the rollout is held).
#[cfg(windows)]
pub fn resume_targets(rollout_path: &Path) -> Vec<Holder> {
    if liveness::is_held_open(rollout_path) {
        liveness::codex_app_server_processes()
    } else {
        Vec::new()
    }
}

/// The outcome of a [`force_release`].
#[derive(Debug, Clone, Default)]
pub struct ReleaseReport {
    /// Holders that were successfully sent a kill.
    pub killed: Vec<Holder>,
    /// Holders the kill could not reach (already gone, or denied).
    pub survived: Vec<Holder>,
    /// Whether the rollout is *still* held open after the attempt — true
    /// means a resume will proceed despite a surviving / respawned owner
    /// (an accepted risk).
    pub still_held: bool,
}

/// Best-effort eviction of every live process holding `rollout_path`
/// open, except those in `exclude_pids` (always pass Pocket-Codex's own
/// app-server pid so we never kill the very server we are about to resume
/// into).
///
/// The kill is forceful ([`force_kill`]). On platforms where liveness is
/// a precise point-in-time probe (Windows), the loop re-checks after each
/// kill and stops as soon as the rollout is released, minimising
/// collateral when only one of several codex app-servers was the real
/// owner. The returned [`ReleaseReport`] records what happened; a
/// non-empty `survived` or a `still_held` of `true` does not abort the
/// caller's resume — it is informational, because the user has opted into
/// the takeover regardless of outcome.
pub fn force_release(rollout_path: &Path, exclude_pids: &[u32]) -> ReleaseReport {
    let mut report = ReleaseReport::default();
    let mut targets = resume_targets(rollout_path);
    targets.retain(|holder| !exclude_pids.contains(&holder.pid));

    for holder in targets {
        // Stop early once the file is no longer held: with several codex
        // app-servers running, killing the real owner releases the file and
        // the rest should be spared.
        if !liveness::is_held_open(rollout_path) {
            break;
        }
        if force_kill(holder.pid) {
            report.killed.push(holder);
        } else {
            report.survived.push(holder);
        }
        // Give the OS a moment to tear down the dead process's handles
        // before the next liveness re-probe.
        std::thread::sleep(RELEASE_SETTLE);
    }

    report.still_held = liveness::is_held_open(rollout_path);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_the_four_quadrants() {
        // Held + running ⇒ owned & running (read-only).
        assert_eq!(classify(&TurnState::Incomplete, true), ResumeSafety::OwnedRunning);
        // Held + finished ⇒ owned but idle (force-resume eligible).
        assert_eq!(classify(&TurnState::Completed, true), ResumeSafety::OwnedIdle);
        assert_eq!(
            classify(&TurnState::Aborted("interrupted".into()), true),
            ResumeSafety::OwnedIdle
        );
        // Free + finished ⇒ plainly resumable.
        assert_eq!(classify(&TurnState::Completed, false), ResumeSafety::Resumable);
        assert_eq!(classify(&TurnState::Empty, false), ResumeSafety::Resumable);
        // Free + unfinished ⇒ resumable but the last turn was left dangling.
        assert_eq!(classify(&TurnState::Incomplete, false), ResumeSafety::ResumableUnfinished);
    }

    #[test]
    fn resume_is_offered_unless_a_turn_is_running() {
        assert!(ResumeSafety::Resumable.allows_resume());
        assert!(ResumeSafety::ResumableUnfinished.allows_resume());
        assert!(ResumeSafety::OwnedIdle.allows_resume());
        assert!(!ResumeSafety::OwnedRunning.allows_resume());
    }

    #[test]
    fn only_owned_idle_requires_a_takeover() {
        assert!(ResumeSafety::OwnedIdle.requires_takeover());
        assert!(!ResumeSafety::Resumable.requires_takeover());
        assert!(!ResumeSafety::OwnedRunning.requires_takeover());
        assert!(!ResumeSafety::ResumableUnfinished.requires_takeover());
    }
}

//! Local codex session inventory for the host meta service.
//!
//! Read-only over `CODEX_HOME` via the `pocket-codex-codex` rollout / liveness
//! / takeover primitives — the same two-signal model the bridge's
//! `engine/sessions.rs` uses, but exposed through serde wire DTOs so the
//! inventory can be served over HTTP and consumed by a remote client.
//!
//! Every function here is **blocking** (it scans the filesystem and process
//! table); HTTP handlers run them on a blocking task.

use anyhow::{anyhow, Result};
use pocket_codex_codex::{
    liveness::{held_open_paths, Holder as CdxHolder},
    rollout, takeover,
};
use serde::{Deserialize, Serialize};

/// A live process holding a session's rollout open — a force-takeover target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    /// Operating-system process id.
    pub pid: u32,
    /// Process image name (e.g. `codex` / `codex.exe`).
    pub name: String,
}

impl From<CdxHolder> for Holder {
    fn from(h: CdxHolder) -> Self {
        Self {
            pid: h.pid,
            name: h.name,
        }
    }
}

/// One session discovered under `CODEX_HOME`, annotated with the state the UI
/// needs to render it read-only or resumable.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Whether resuming requires a force takeover (a live owner must be evicted
    /// first).
    pub requires_takeover: bool,
}

/// A single session's liveness detail, including the processes a force takeover
/// would target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionLiveness {
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
    /// Processes a force takeover would attempt to terminate (already excluding
    /// the protected pids passed to [`liveness`]).
    pub holders: Vec<Holder>,
}

/// A read-only transcript row, matching `thread/read`'s `{id, type, title,
/// text}` shape so the viewer can reuse the live-conversation rendering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptItem {
    /// Stable row id (the source line index).
    pub id: String,
    /// Item kind: `userMessage` / `agentMessage` / `reasoning` /
    /// `commandExecution`.
    pub item_type: String,
    /// One-line title (the command for tool calls; empty for messages).
    pub title: String,
    /// Body text: message markdown, reasoning summary, or command output.
    pub text: String,
}

impl From<rollout::TranscriptItem> for TranscriptItem {
    fn from(t: rollout::TranscriptItem) -> Self {
        Self {
            id: t.id,
            item_type: t.item_type,
            title: t.title,
            text: t.text,
        }
    }
}

/// List every session under `CODEX_HOME`, newest first, each annotated with its
/// resume-safety state. Reads only the local filesystem and process table.
pub fn list() -> Result<Vec<LocalSession>> {
    let sessions = rollout::scan_sessions().map_err(|e| anyhow!("scanning sessions: {e}"))?;
    // Batch the liveness probe rather than spawning one per session.
    let held = held_open_paths(
        &sessions
            .iter()
            .map(|s| s.rollout_path.clone())
            .collect::<Vec<_>>(),
    );
    let mut out = Vec::with_capacity(sessions.len());
    for info in sessions {
        let held_open = held.contains(&info.rollout_path);
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

/// Inspect one session's liveness in detail, excluding `protected` pids (the
/// server we resume into + this process) from the listed takeover targets.
pub fn liveness(thread_id: &str, protected: &[u32]) -> Result<SessionLiveness> {
    let path = rollout::rollout_path_for_thread(thread_id)
        .map_err(|e| anyhow!("locating rollout: {e}"))?
        .ok_or_else(|| anyhow!("no rollout found for thread {thread_id}"))?;
    let live = takeover::inspect(&path).map_err(|e| anyhow!("inspecting rollout: {e}"))?;
    let holders = live
        .holders
        .into_iter()
        .filter(|h| !protected.contains(&h.pid))
        .map(Holder::from)
        .collect();
    Ok(SessionLiveness {
        thread_id: thread_id.to_string(),
        turn_state: live.turn_state.tag().to_string(),
        held_open: live.held_open,
        safety: live.safety.tag().to_string(),
        allows_resume: live.safety.allows_resume(),
        requires_takeover: live.safety.requires_takeover(),
        holders,
    })
}

/// Read a local session's full transcript for read-only viewing — parsed from
/// the on-disk rollout, so it works for a session another client owns and never
/// touches the app-server.
pub fn transcript(thread_id: &str) -> Result<Vec<TranscriptItem>> {
    let path = rollout::rollout_path_for_thread(thread_id)
        .map_err(|e| anyhow!("locating rollout: {e}"))?
        .ok_or_else(|| anyhow!("no rollout found for thread {thread_id}"))?;
    let items = rollout::read_transcript(&path).map_err(|e| anyhow!("reading transcript: {e}"))?;
    Ok(items.into_iter().map(TranscriptItem::from).collect())
}

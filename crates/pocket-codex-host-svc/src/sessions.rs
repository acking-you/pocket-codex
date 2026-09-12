//! Local codex session inventory for the host meta service.
//!
//! Read-only over `CODEX_HOME` via the `pocket-codex-codex` rollout / liveness
//! / takeover primitives — the same two-signal model the bridge's
//! `engine/sessions.rs` uses, but exposed through serde wire DTOs so the
//! inventory can be served over HTTP and consumed by a remote client.
//!
//! Every function here is **blocking** (it scans the filesystem and process
//! table); HTTP handlers run them on a blocking task.

use std::path::PathBuf;

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptItem {
    /// Stable row id (the source line index).
    pub id: String,
    /// Item kind: `userMessage` / `agentMessage` / `reasoning` /
    /// `commandExecution` / `contextCompaction` / `plan`.
    pub item_type: String,
    /// One-line title (the command for tool calls; empty for messages).
    pub title: String,
    /// Body text: message markdown, reasoning summary, or command output.
    pub text: String,
    /// Image data URLs attached to a user message. `#[serde(default)]` so a
    /// response from an older host (no field) still deserializes.
    #[serde(default)]
    pub images: Vec<String>,
}

/// One point-in-time read-only view emitted by the session follow stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFollowUpdate {
    /// Current ownership and resume-safety state.
    pub liveness: SessionLiveness,
    /// Full transcript for legacy clients, empty in metadata-only mode.
    pub items: Vec<TranscriptItem>,
    /// Opaque rollout revision when items are omitted for app-server paging.
    /// Absent on legacy full-transcript streams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_revision: Option<String>,
}

impl From<rollout::TranscriptItem> for TranscriptItem {
    fn from(t: rollout::TranscriptItem) -> Self {
        Self {
            id: t.id,
            item_type: t.item_type,
            title: t.title,
            text: t.text,
            images: t.images,
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

/// Discover running sessions without replaying inactive transcript files.
/// Ownership is checked in one batch; only held files need lifecycle scans.
pub fn running() -> Result<Vec<LocalSession>> {
    let paths = rollout::session_paths()?;
    Ok(running_at(&paths))
}

fn running_at(paths: &[PathBuf]) -> Vec<LocalSession> {
    let held = held_open_paths(paths);
    let mut out = Vec::new();
    for path in held {
        if !matches!(rollout::classify_turn_state(&path), Ok(rollout::TurnState::Incomplete)) {
            continue;
        }
        let Ok(info) = rollout::read_session_info(&path) else { continue };
        if info.turn_state != rollout::TurnState::Incomplete {
            continue;
        }
        out.push(LocalSession {
            thread_id: info.thread_id,
            cwd: info.cwd,
            preview: info.preview,
            source: info.source,
            updated_at: info.updated_at,
            turn_state: "incomplete".into(),
            held_open: true,
            safety: "ownedRunning".into(),
            allows_resume: false,
            requires_takeover: false,
        });
    }
    out.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    out
}

/// Inspect one session's liveness in detail, excluding `protected` pids (the
/// server we resume into + this process) from the listed takeover targets.
pub fn liveness(thread_id: &str, protected: &[u32]) -> Result<SessionLiveness> {
    let path = rollout_path(thread_id)?;
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
    let path = rollout_path(thread_id)?;
    let items = rollout::read_transcript(&path).map_err(|e| anyhow!("reading transcript: {e}"))?;
    Ok(items.into_iter().map(TranscriptItem::from).collect())
}

pub(crate) fn rollout_path(thread_id: &str) -> Result<PathBuf> {
    rollout::rollout_path_for_thread(thread_id)
        .map_err(|e| anyhow!("locating rollout: {e}"))?
        .ok_or_else(|| anyhow!("no rollout found for thread {thread_id}"))
}

/// Read the liveness and transcript used to seed a session follow stream.
pub fn follow_update(thread_id: &str, protected: &[u32]) -> Result<SessionFollowUpdate> {
    Ok(SessionFollowUpdate {
        liveness: liveness(thread_id, protected)?,
        items: transcript(thread_id)?,
        history_revision: None,
    })
}

#[cfg(test)]
mod running_tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn inventory_observes_start_and_completion_without_resuming() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rollout-running.jsonl");
        let orphan = dir.path().join("rollout-orphan.jsonl");
        let started = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
        let completed = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n";
        std::fs::write(&orphan, started).expect("orphaned incomplete rollout");
        let mut writer = std::fs::File::create(&path).expect("open writer");
        writeln!(
            writer,
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"running\",\"cwd\":\"/project\"}}}}"
        )
        .expect("metadata");
        let paths = [path, orphan];
        assert!(running_at(&paths).is_empty(), "ownership alone is not running");
        writer.write_all(started.as_bytes()).expect("start");
        let active = running_at(&paths);
        assert_eq!(active.len(), 1, "unowned incomplete sessions are not active");
        assert_eq!(active[0].thread_id, "running");
        assert_eq!(active[0].safety, "ownedRunning");
        assert!(!active[0].allows_resume);
        writer.write_all(completed.as_bytes()).expect("complete");
        assert!(running_at(&paths).is_empty(), "completion is visible with the file still open");
        writer.write_all(started.as_bytes()).expect("restart");
        assert_eq!(running_at(&paths).len(), 1);
        drop(writer);
        assert!(running_at(&paths).is_empty(), "exiting a writer clears the badge");
    }
}

//! Native OpenCode v2 wire shapes. Unknown fields are preserved in `extra`
//! rather than reinterpreted, so newer servers stay readable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Server identity from `GET /api/info`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerInfo {
    /// Server release, e.g. `2.0.18`.
    pub version: String,
    /// Operating-system process id, or zero when unavailable upstream.
    #[serde(default)]
    pub pid: u64,
    /// Additional native metadata, including URLs and runtime paths.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native host location.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Location {
    /// Absolute directory on the host.
    pub directory: String,
    /// Future location fields (e.g. `workspaceID`).
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native session metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    /// Server-assigned session identity (`ses_…`).
    pub id: String,
    /// Parent session for subagent (child) sessions.
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Optional title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Current primary agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Current model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Working directory the session runs in.
    pub location: Location,
    /// Other metadata: time, tokens, cost, outcome, …
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Session {
    /// `time.updated` (milliseconds since the epoch), falling back to created.
    pub fn updated_ms(&self) -> i64 {
        let time = self.extra.get("time");
        time.and_then(|t| t["updated"].as_f64())
            .or_else(|| time.and_then(|t| t["created"].as_f64()))
            .unwrap_or(0.0) as i64
    }
}

/// A native model reference (`providerID/id#variant`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelRef {
    /// Model id within its provider.
    pub id: String,
    /// Provider id.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Optional variant (reasoning level etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl ModelRef {
    /// `providerID/id`, the id Pocket-Codex shows as the model.
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.provider_id, self.id)
    }

    /// Parse `providerID/id`; the id itself may contain further slashes.
    pub fn parse(qualified: &str, variant: Option<String>) -> Option<Self> {
        let (provider, id) = qualified.split_once('/')?;
        if provider.is_empty() || id.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_string(),
            provider_id: provider.to_string(),
            variant,
        })
    }
}

/// Native discriminated message (`user`, `assistant`, `shell`, …).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    /// Message identity (`msg_…`).
    pub id: String,
    /// Native variant discriminator.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native text, content, time, metadata and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Message {
    /// `time.created` in milliseconds.
    pub fn created_ms(&self) -> i64 {
        self.extra
            .get("time")
            .and_then(|t| t["created"].as_f64())
            .unwrap_or(0.0) as i64
    }
}

/// One bounded chronological message window.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MessagePage {
    /// Oldest to newest messages in this window.
    pub messages: Vec<Message>,
    /// Opaque cursor for the next OLDER window, `None` at the beginning.
    pub older_cursor: Option<String>,
}

/// One page of sessions, newest first.
#[derive(Clone, Debug, Default)]
pub struct SessionPage {
    /// Sessions on this page.
    pub sessions: Vec<Session>,
    /// Opaque cursor for the next page, `None` on the last one.
    pub next_cursor: Option<String>,
}

/// Durable prompt admission; not an indication that execution completed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptAcceptance {
    /// Admitted user message id.
    pub id: String,
    /// Owning session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Inbox discriminator, `user` for a prompt.
    #[serde(rename = "type")]
    pub kind: String,
    /// `queue` or `steer`.
    pub delivery: String,
    /// Other native inbox fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A pending native permission request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Permission {
    /// Request identity (`per…`).
    pub id: String,
    /// Owning session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Action to authorize (e.g. `shell`, `edit`).
    pub action: String,
    /// Requested resources.
    #[serde(default)]
    pub resources: Vec<String>,
    /// Project-rule patterns an `always` reply would persist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    /// Human-readable explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Source, metadata and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Reply to a permission request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    /// Allow this one request.
    Once,
    /// Allow and persist the offered rule for the project.
    Always,
    /// Deny.
    Reject,
}

/// A pending native form (question).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Form {
    /// Form identity (`frm_…`).
    pub id: String,
    /// Owning session, or `global`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Title shown above the fields.
    #[serde(default)]
    pub title: String,
    /// Typed fields, kept native.
    pub fields: Vec<Value>,
    /// Metadata and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One changed file from `GET /api/vcs/diff`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileDiff {
    /// Path relative to the repository.
    pub file: String,
    /// Unified diff text.
    #[serde(default)]
    pub patch: String,
    /// `added` / `deleted` / `modified`.
    #[serde(default)]
    pub status: String,
}

#[derive(Deserialize)]
pub(super) struct Data<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub(super) struct Page<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub cursor: Cursor,
}

#[derive(Default, Deserialize)]
pub(super) struct Cursor {
    #[serde(default)]
    pub next: Option<String>,
}

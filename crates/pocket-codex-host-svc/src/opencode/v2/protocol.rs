use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Ready server identity. Advertised URLs are metadata, never credential
/// destinations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerInfo {
    /// Exact native protocol version.
    pub version: String,
    /// Operating-system process identity, or zero when unavailable upstream.
    pub pid: u64,
    /// Additional native metadata, including URLs and runtime paths.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native host location; additional fields are preserved rather than
/// reinterpreted.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Location {
    /// Absolute directory reported by the server.
    pub directory: String,
    /// Future location fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native session metadata without v1 directory or title assumptions.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    /// Server-assigned session identity.
    pub id: String,
    /// Optional native title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Current authoritative location.
    pub location: Location,
    /// Other metadata, including time, model, cost and execution outcome.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native discriminated message, retaining unsupported variants for display.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    /// Message identity within its separately authorized session endpoint.
    pub id: String,
    /// Native variant, not the v1 role field.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native text, content, time, metadata and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One bounded chronological history window.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessagePage {
    /// Oldest to newest messages in this window.
    pub messages: Vec<Message>,
    /// Opaque native cursor for the next older window, never a URL to follow.
    pub next_cursor: Option<String>,
}

/// Durable prompt admission, not an indication that model execution completed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptAcceptance {
    /// Identity chosen by the caller or assigned by the server.
    pub id: String,
    /// Session that owns this inbox item.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Native inbox discriminator, required to be user for a text prompt.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native admitted prompt payload.
    pub payload: Value,
    /// Native delivery boundary, queue or steer.
    pub delivery: String,
    /// Other native inbox fields, including admission time.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A current native permission request; saved rules may persist by project.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Permission {
    /// Pending request identity.
    pub id: String,
    /// Owning session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Native action to authorize.
    pub action: String,
    /// Explicit requested resources.
    pub resources: Vec<String>,
    /// Persistent project-rule patterns offered by the server, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    /// Native source, metadata, message and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A current native form, not a v1 array of question choices.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Form {
    /// Pending form identity.
    pub id: String,
    /// Owning session; global forms are intentionally excluded by this client.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Native title.
    pub title: String,
    /// Native keyed, typed fields and constraints, kept without lossy
    /// conversion.
    pub fields: Vec<Value>,
    /// Native metadata and future fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
pub(super) struct Located<T> {
    pub location: Location,
    pub data: T,
}

#[derive(Deserialize)]
pub(super) struct Data<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub(super) struct Page<T> {
    pub data: Vec<T>,
    pub cursor: Cursor,
}

#[derive(Deserialize)]
pub(super) struct Cursor {
    #[serde(default)]
    pub next: Option<String>,
}

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Reported external service health.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Health {
    /// True when the external service reports ready.
    pub healthy: bool,
    /// Upstream version, not a Pocket-Codex version.
    pub version: String,
}

/// Successfully negotiated unprefixed session API capabilities.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Capabilities {
    /// Reported upstream version.
    pub version: String,
    /// Whether this exact version was exercised by the repository fixture.
    pub tested_version: bool,
}

/// An upstream session, retaining extension fields without interpreting them.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    /// Stable upstream session identifier.
    pub id: String,
    /// User-visible upstream title.
    pub title: String,
    /// Canonical project directory on the server.
    pub directory: String,
    /// Optional upstream workspace, unsupported in the initial local gateway.
    #[serde(rename = "workspaceID", default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Additional upstream metadata, including timestamps and model selection.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native message metadata with stable source identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageInfo {
    /// Upstream message identifier.
    pub id: String,
    /// Owning upstream session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Upstream role, normally user or assistant.
    pub role: String,
    /// Upstream metadata retained without a Codex projection.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A native message part; unknown part types retain their original fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessagePart {
    /// Upstream part identifier.
    pub id: String,
    /// Owning upstream message.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Owning upstream session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Native part type, such as text, reasoning, or tool.
    #[serde(rename = "type")]
    pub kind: String,
    /// Part body and extension fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A message and its currently available parts.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    /// Native message metadata.
    pub info: MessageInfo,
    /// Ordered native parts; an active message may initially have no parts.
    pub parts: Vec<MessagePart>,
}

/// One bounded chronological page, with an opaque cursor for earlier messages.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessagePage {
    /// Messages ordered from oldest to newest within the page.
    pub messages: Vec<Message>,
    /// Next earlier page cursor; never a URL from the Link header.
    pub next_cursor: Option<String>,
}

/// Supported user input parts. File, shell, and tool injection are not exposed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum PromptPart {
    /// Plain user text interpreted by the existing OpenCode session.
    Text {
        /// User-authored text.
        text: String,
    },
}

/// Explicit asynchronous input; model settings continue to come from OpenCode.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInput {
    /// Native input parts.
    pub parts: Vec<PromptPart>,
    /// Optional client-created message identity for later reconciliation.
    #[serde(rename = "messageID", default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl PromptInput {
    /// Construct a text-only continuation without changing upstream model
    /// settings.
    pub fn text(text: impl Into<String>, message_id: Option<String>) -> Self {
        Self {
            parts: vec![PromptPart::Text {
                text: text.into(),
            }],
            message_id,
        }
    }
}

/// One current upstream permission request, never reconstructed from history.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PermissionRequest {
    /// Current request identifier.
    pub id: String,
    /// Owning session.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Permission name, for example bash.
    pub permission: String,
    /// Targets requested by the current operation.
    pub patterns: Vec<String>,
    /// Upstream explanation and tool metadata.
    pub metadata: Value,
    /// Patterns that instance-wide subsequent permission would authorize.
    pub always: Vec<String>,
    /// Additional upstream fields, including optional tool identity.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Native permission decision; Always affects the current upstream instance.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    /// Allow only this request.
    Once,
    /// Allow matching subsequent requests in the upstream instance memory.
    Always,
    /// Reject this request and any upstream-coupled pending requests.
    Reject,
}

/// A selectable upstream question answer.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestionOption {
    /// Exact answer label to submit.
    pub label: String,
    /// Explanation shown alongside the label.
    pub description: String,
}

/// A single question in an ordered question request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Question {
    /// Full question text.
    pub question: String,
    /// Compact upstream heading.
    pub header: String,
    /// Available labels and explanations.
    pub options: Vec<QuestionOption>,
    /// Whether several answers can be selected; omitted means false.
    #[serde(default)]
    pub multiple: bool,
    /// Whether arbitrary text is accepted; omitted means true.
    #[serde(default = "default_true")]
    pub custom: bool,
}

fn default_true() -> bool {
    true
}

/// A current question request, separate from permission authorization.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestionRequest {
    /// Pending upstream request identifier.
    pub id: String,
    /// Session waiting for the answers.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Questions in the required answer order.
    pub questions: Vec<Question>,
    /// Additional metadata such as optional tool identity.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

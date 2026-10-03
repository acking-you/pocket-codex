//! ACP v1 request, response and capability types (subset of
//! `schema-v1.23.0`).
//!
//! Conventions: camelCase on the wire, unknown fields ignored (ACP only adds
//! fields), optional fields skipped when absent, `_meta` kept verbatim, and
//! protocol enums stored as strings so unknown values survive a round trip.

use std::collections::BTreeMap;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// ACP method names used by Pocket-Codex.
pub mod methods {
    /// `initialize`
    pub const INITIALIZE: &str = "initialize";
    /// `authenticate`
    pub const AUTHENTICATE: &str = "authenticate";
    /// `session/new`
    pub const SESSION_NEW: &str = "session/new";
    /// `session/load`
    pub const SESSION_LOAD: &str = "session/load";
    /// `session/resume`
    pub const SESSION_RESUME: &str = "session/resume";
    /// `session/close`
    pub const SESSION_CLOSE: &str = "session/close";
    /// `session/list`
    pub const SESSION_LIST: &str = "session/list";
    /// `session/prompt`
    pub const SESSION_PROMPT: &str = "session/prompt";
    /// `session/cancel` (notification)
    pub const SESSION_CANCEL: &str = "session/cancel";
    /// `session/set_config_option`
    pub const SESSION_SET_CONFIG_OPTION: &str = "session/set_config_option";
    /// `session/set_mode`
    pub const SESSION_SET_MODE: &str = "session/set_mode";
    /// `session/update` (notification)
    pub const SESSION_UPDATE: &str = "session/update";
    /// `session/request_permission`
    pub const SESSION_REQUEST_PERMISSION: &str = "session/request_permission";
    /// `elicitation/create`
    pub const ELICITATION_CREATE: &str = "elicitation/create";
    /// `elicitation/complete` (notification)
    pub const ELICITATION_COMPLETE: &str = "elicitation/complete";
    /// `fs/read_text_file`
    pub const FS_READ_TEXT_FILE: &str = "fs/read_text_file";
    /// `fs/write_text_file`
    pub const FS_WRITE_TEXT_FILE: &str = "fs/write_text_file";
    /// `$/cancel_request` (notification)
    pub const CANCEL_REQUEST: &str = "$/cancel_request";
}

/// Known `stopReason` values.
pub mod stop_reason {
    /// The turn ended normally.
    pub const END_TURN: &str = "end_turn";
    /// The model hit its token limit.
    pub const MAX_TOKENS: &str = "max_tokens";
    /// The agent hit its request limit.
    pub const MAX_TURN_REQUESTS: &str = "max_turn_requests";
    /// The model refused.
    pub const REFUSAL: &str = "refusal";
    /// The turn was cancelled.
    pub const CANCELLED: &str = "cancelled";
}

/// Known tool-call `status` values.
pub mod tool_status {
    /// Not started.
    pub const PENDING: &str = "pending";
    /// Running.
    pub const IN_PROGRESS: &str = "in_progress";
    /// Finished successfully.
    pub const COMPLETED: &str = "completed";
    /// Failed.
    pub const FAILED: &str = "failed";
}

/// Known permission option `kind` values.
pub mod permission_kind {
    /// Allow this once.
    pub const ALLOW_ONCE: &str = "allow_once";
    /// Always allow.
    pub const ALLOW_ALWAYS: &str = "allow_always";
    /// Reject this once.
    pub const REJECT_ONCE: &str = "reject_once";
    /// Always reject.
    pub const REJECT_ALWAYS: &str = "reject_always";
}

/// Name and version of a client or agent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Implementation {
    /// Programmatic name.
    #[serde(default)]
    pub name: String,
    /// Display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// An empty capability object; its presence means "supported".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilityMarker {
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Client file-system capabilities.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemCapabilities {
    /// `fs/read_text_file` supported.
    #[serde(default)]
    pub read_text_file: bool,
    /// `fs/write_text_file` supported.
    #[serde(default)]
    pub write_text_file: bool,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Client authentication capabilities.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCapabilities {
    /// Terminal-based auth methods supported.
    #[serde(default)]
    pub terminal: bool,
    /// Extension metadata (e.g. `{"gateway": true}`).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Client elicitation capabilities.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationCapabilities {
    /// Form mode supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<CapabilityMarker>,
    /// URL mode supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<CapabilityMarker>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Capabilities the client advertises in `initialize`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    /// File-system methods.
    #[serde(default)]
    pub fs: FileSystemCapabilities,
    /// Terminal methods.
    #[serde(default)]
    pub terminal: bool,
    /// Authentication.
    #[serde(default)]
    pub auth: AuthCapabilities,
    /// Elicitation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapabilities>,
    /// Session capabilities (`{"configOptions": {"boolean": {}}}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `initialize` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    /// Latest protocol version the client supports.
    pub protocol_version: u16,
    /// Client capabilities.
    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
    /// Client identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<Implementation>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Prompt content the agent accepts beyond text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    /// Image blocks.
    #[serde(default)]
    pub image: bool,
    /// Audio blocks.
    #[serde(default)]
    pub audio: bool,
    /// Embedded resource blocks.
    #[serde(default)]
    pub embedded_context: bool,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Session methods beyond new/prompt the agent supports. `fork` is unstable
/// and deliberately not modelled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    /// `session/list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<CapabilityMarker>,
    /// `session/resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<CapabilityMarker>,
    /// `session/close`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<CapabilityMarker>,
    /// `session/delete`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<CapabilityMarker>,
    /// `additionalDirectories` accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<CapabilityMarker>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Capabilities the agent advertises in `initialize`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// `session/load` supported.
    #[serde(default)]
    pub load_session: bool,
    /// Accepted prompt content.
    #[serde(default)]
    pub prompt_capabilities: PromptCapabilities,
    /// MCP transports supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_capabilities: Option<Value>,
    /// Optional session methods.
    #[serde(default)]
    pub session_capabilities: SessionCapabilities,
    /// Agent-side auth capabilities (e.g. logout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One authentication method the agent offers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    /// Method id passed to `authenticate`.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `terminal` for terminal methods; absent for agent methods.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Extra arguments for a terminal method.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Extra environment for a terminal method.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Extension metadata (`terminal-auth`, `gateway`, …).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `initialize` response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    /// Negotiated protocol version.
    pub protocol_version: u16,
    /// Agent capabilities.
    #[serde(default)]
    pub agent_capabilities: AgentCapabilities,
    /// Authentication methods.
    #[serde(default)]
    pub auth_methods: Vec<AuthMethod>,
    /// Agent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<Implementation>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `authenticate` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateRequest {
    /// Method id from `authMethods`.
    pub method_id: String,
    /// Extension metadata (e.g. the D20 `gateway` object).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/new` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionRequest {
    /// Absolute working directory.
    pub cwd: String,
    /// MCP servers; Pocket-Codex always sends `[]`.
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/load` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionRequest {
    /// Session to load.
    pub session_id: String,
    /// Absolute working directory.
    pub cwd: String,
    /// MCP servers; Pocket-Codex always sends `[]`.
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/resume` request (same shape as load, without replay).
pub type ResumeSessionRequest = LoadSessionRequest;

/// `session/close` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionRequest {
    /// Session to close.
    pub session_id: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Response shared by `session/new`, `session/load` and `session/resume`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetup {
    /// New session id (only for `session/new`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<SessionModeState>,
    /// Config options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Vec<ConfigOption>>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/list` request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsRequest {
    /// Only sessions for this working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Pagination cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One listed session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// Session id.
    pub session_id: String,
    /// Working directory.
    #[serde(default)]
    pub cwd: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Last update (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/list` response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsResponse {
    /// Sessions on this page.
    #[serde(default)]
    pub sessions: Vec<SessionInfo>,
    /// Cursor of the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/prompt` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    /// Session id.
    pub session_id: String,
    /// Prompt content.
    pub prompt: Vec<ContentBlock>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/prompt` response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    /// Why the turn ended (see [`stop_reason`]).
    pub stop_reason: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/cancel` notification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    /// Session id.
    pub session_id: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `$/cancel_request` notification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequestNotification {
    /// Id of the request to cancel.
    pub request_id: crate::acp::rpc::RequestId,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Text content.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    /// The text.
    pub text: String,
    /// Annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Base64 image content.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    /// Base64 data.
    pub data: String,
    /// MIME type.
    pub mime_type: String,
    /// Optional source URI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Base64 audio content.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioContent {
    /// Base64 data.
    pub data: String,
    /// MIME type.
    pub mime_type: String,
    /// Annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A link to a resource.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLink {
    /// Resource URI.
    pub uri: String,
    /// Resource name.
    pub name: String,
    /// MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// An embedded resource (text or blob contents kept verbatim).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedResource {
    /// `{uri, text | blob, mimeType?}`.
    pub resource: Value,
    /// Annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A content block, discriminated by `type`.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentBlock {
    /// `text`
    Text(TextContent),
    /// `image`
    Image(ImageContent),
    /// `audio`
    Audio(AudioContent),
    /// `resource_link`
    ResourceLink(ResourceLink),
    /// `resource`
    Resource(EmbeddedResource),
    /// Unknown or malformed block, kept verbatim.
    Unknown(Value),
}

impl ContentBlock {
    /// A plain text block.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextContent {
            text: text.into(),
            ..TextContent::default()
        })
    }

    /// The text of a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}

/// Deserialize a tagged payload; `None` when it does not fit the variant.
fn payload<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
    T::deserialize(value).ok()
}

/// Serialize `payload` as an object and insert `tag: name` first.
fn with_tag<S: Serializer, T: Serialize>(
    serializer: S,
    tag: &str,
    name: &str,
    payload: &T,
) -> Result<S::Ok, S::Error> {
    let mut map = match serde_json::to_value(payload).map_err(serde::ser::Error::custom)? {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert(tag.to_string(), Value::String(name.to_string()));
    Value::Object(map).serialize(serializer)
}

impl Serialize for ContentBlock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(p) => with_tag(serializer, "type", "text", p),
            Self::Image(p) => with_tag(serializer, "type", "image", p),
            Self::Audio(p) => with_tag(serializer, "type", "audio", p),
            Self::ResourceLink(p) => with_tag(serializer, "type", "resource_link", p),
            Self::Resource(p) => with_tag(serializer, "type", "resource", p),
            Self::Unknown(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("content block is not an object"));
        }
        let parsed = match value.get("type").and_then(Value::as_str) {
            Some("text") => payload(&value).map(Self::Text),
            Some("image") => payload(&value).map(Self::Image),
            Some("audio") => payload(&value).map(Self::Audio),
            Some("resource_link") => payload(&value).map(Self::ResourceLink),
            Some("resource") => payload(&value).map(Self::Resource),
            _ => None,
        };
        Ok(parsed.unwrap_or(Self::Unknown(value)))
    }
}

/// Regular content produced by a tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContent {
    /// The content block.
    pub content: ContentBlock,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A file diff produced by a tool.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffContent {
    /// Absolute file path.
    pub path: String,
    /// Previous text (`None` for a new file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    /// New text.
    pub new_text: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A client terminal embedded in a tool call.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalContent {
    /// Terminal id.
    pub terminal_id: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Tool-call content, discriminated by `type`.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolCallContent {
    /// `content`
    Content(ToolContent),
    /// `diff`
    Diff(DiffContent),
    /// `terminal`
    Terminal(TerminalContent),
    /// Unknown or malformed content, kept verbatim.
    Unknown(Value),
}

impl Serialize for ToolCallContent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Content(p) => with_tag(serializer, "type", "content", p),
            Self::Diff(p) => with_tag(serializer, "type", "diff", p),
            Self::Terminal(p) => with_tag(serializer, "type", "terminal", p),
            Self::Unknown(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolCallContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("tool call content is not an object"));
        }
        let parsed = match value.get("type").and_then(Value::as_str) {
            Some("content") => payload(&value).map(Self::Content),
            Some("diff") => payload(&value).map(Self::Diff),
            Some("terminal") => payload(&value).map(Self::Terminal),
            _ => None,
        };
        Ok(parsed.unwrap_or(Self::Unknown(value)))
    }
}

/// A file location a tool touches.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLocation {
    /// Absolute path.
    pub path: String,
    /// Line number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A full tool call (`tool_call` update).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Unique within the session.
    pub tool_call_id: String,
    /// Display title.
    #[serde(default)]
    pub title: String,
    /// Tool kind (`read`, `edit`, `execute`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Status (see [`tool_status`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Produced content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ToolCallContent>,
    /// Touched locations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<ToolCallLocation>,
    /// Raw input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    /// Raw output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
    /// Tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A partial tool call (`tool_call_update`); absent fields are unchanged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallUpdate {
    /// Tool call being updated.
    pub tool_call_id: String,
    /// New title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// New status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Replacement content list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolCallContent>>,
    /// Replacement locations list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<ToolCallLocation>>,
    /// New raw input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    /// New raw output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
    /// Tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One plan entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    /// Task description.
    pub content: String,
    /// `high` | `medium` | `low`
    #[serde(default)]
    pub priority: String,
    /// `pending` | `in_progress` | `completed`
    #[serde(default)]
    pub status: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One session mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMode {
    /// Mode id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Current and available modes of a session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModeState {
    /// Current mode id.
    pub current_mode_id: String,
    /// Available modes.
    #[serde(default)]
    pub available_modes: Vec<SessionMode>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One value of a select config option.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSelectOption {
    /// Value id.
    pub value: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A named group of select values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSelectGroup {
    /// Group id.
    pub group: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Values in this group.
    #[serde(default)]
    pub options: Vec<ConfigSelectOption>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A select value or a group of values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigSelectEntry {
    /// `{group, name, options}`
    Group(ConfigSelectGroup),
    /// `{value, name, description?}`
    Option(ConfigSelectOption),
}

/// A session configuration option (`select` or `boolean`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    /// Option id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `mode` | `model` | `model_config` | `thought_level` | other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// `select` | `boolean`
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Current value (string id for select, bool for boolean).
    #[serde(default)]
    pub current_value: Value,
    /// Values of a select option.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ConfigSelectEntry>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl ConfigOption {
    /// Every select value, flattening groups.
    pub fn flat_options(&self) -> Vec<&ConfigSelectOption> {
        self.options
            .iter()
            .flat_map(|entry| match entry {
                ConfigSelectEntry::Option(o) => vec![o],
                ConfigSelectEntry::Group(g) => g.options.iter().collect(),
            })
            .collect()
    }
}

/// `session/set_config_option` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetConfigOptionRequest {
    /// Session id.
    pub session_id: String,
    /// Option id.
    pub config_id: String,
    /// New value.
    pub value: Value,
    /// `boolean` for boolean options; absent for select.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl SetConfigOptionRequest {
    /// Build a request, adding `type: "boolean"` for boolean values.
    pub fn new(session_id: impl Into<String>, config_id: impl Into<String>, value: Value) -> Self {
        let kind = value.is_boolean().then(|| "boolean".to_string());
        Self {
            session_id: session_id.into(),
            config_id: config_id.into(),
            value,
            kind,
            meta: None,
        }
    }
}

/// `session/set_config_option` response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetConfigOptionResponse {
    /// All options after the change.
    #[serde(default)]
    pub config_options: Vec<ConfigOption>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/set_mode` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeRequest {
    /// Session id.
    pub session_id: String,
    /// Mode id.
    pub mode_id: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A slash command the agent offers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCommand {
    /// Command name (without `/`).
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Input hint (`{hint}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Cost of a session so far.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    /// Amount.
    pub amount: f64,
    /// ISO 4217 currency code.
    pub currency: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Context-window usage (`usage_update`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageUpdate {
    /// Tokens in the context.
    pub used: u64,
    /// Context window size.
    pub size: u64,
    /// Cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One option of a permission request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    /// Option id.
    pub option_id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// See [`permission_kind`].
    #[serde(default)]
    pub kind: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `session/request_permission` request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionRequest {
    /// Session id.
    pub session_id: String,
    /// The tool call needing permission.
    pub tool_call: ToolCallUpdate,
    /// Offered options.
    #[serde(default)]
    pub options: Vec<PermissionOption>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// The outcome of a permission request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PermissionOutcome {
    /// The prompt turn was cancelled.
    Cancelled,
    /// An option was selected.
    Selected {
        /// Selected option id.
        #[serde(rename = "optionId")]
        option_id: String,
    },
}

/// `session/request_permission` response: `{"outcome": {...}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionResponse {
    /// The outcome.
    pub outcome: PermissionOutcome,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `elicitation/create` request (form or URL mode).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRequest {
    /// Message shown to the user.
    pub message: String,
    /// `form` | `url`
    #[serde(default)]
    pub mode: String,
    /// JSON schema of a form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_schema: Option<Value>,
    /// URL-mode id, echoed by `elicitation/complete`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation_id: Option<String>,
    /// URL to open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Session scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Tool-call scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Request scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Known elicitation actions.
pub mod elicitation_action {
    /// The user accepted (form content attached).
    pub const ACCEPT: &str = "accept";
    /// The user declined.
    pub const DECLINE: &str = "decline";
    /// The request was cancelled.
    pub const CANCEL: &str = "cancel";
}

/// `elicitation/create` response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationResponse {
    /// See [`elicitation_action`].
    pub action: String,
    /// Form content for `accept`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `elicitation/complete` notification.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationComplete {
    /// URL-mode id.
    pub elicitation_id: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `fs/read_text_file` request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileRequest {
    /// Session id.
    pub session_id: String,
    /// Absolute path.
    pub path: String,
    /// First line (1-based).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Maximum number of lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `fs/read_text_file` response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileResponse {
    /// File content.
    pub content: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `fs/write_text_file` request (the response is `{}`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteTextFileRequest {
    /// Session id.
    pub session_id: String,
    /// Absolute path.
    pub path: String,
    /// New content.
    pub content: String,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

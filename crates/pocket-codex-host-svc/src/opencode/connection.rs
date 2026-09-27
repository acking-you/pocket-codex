//! Negotiated OpenCode connections, preserving each upstream's native payloads.

use std::{collections::BTreeMap, pin::Pin};

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    v2, BasicCredentials, Error, Message, OpenCodeClient, OpenCodeEvent, PermissionReply,
    PermissionRequest, PromptInput, QuestionRequest, Result, Session,
};

mod remote;
use remote::Remote;

/// A native message. Serialization never turns v2 content into v1 parts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NativeMessage {
    /// Original v1 wire message.
    V1(Message),
    /// Original v2 wire message.
    V2(v2::Message),
}

impl NativeMessage {
    /// Identity within the independently authorized session.
    pub fn id(&self) -> &str {
        match self {
            Self::V1(value) => &value.info.id,
            Self::V2(value) => &value.id,
        }
    }
}

/// A current permission, with its native authorization semantics intact.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NativePermission {
    /// Instance-lifetime legacy permission.
    V1(PermissionRequest),
    /// Native v2 permission, potentially project-persistent when saved.
    V2(v2::Permission),
}

impl NativePermission {
    /// Owning upstream session.
    pub fn session_id(&self) -> &str {
        match self {
            Self::V1(value) => &value.session_id,
            Self::V2(value) => &value.session_id,
        }
    }
    /// Current pending request identity.
    pub fn id(&self) -> &str {
        match self {
            Self::V1(value) => &value.id,
            Self::V2(value) => &value.id,
        }
    }
}

/// A legacy question or native typed form, never converted between schemas.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NativeQuestion {
    /// Ordered v1 questions.
    V1(QuestionRequest),
    /// Keyed, typed v2 form.
    V2(v2::Form),
}

impl NativeQuestion {
    /// Owning upstream session.
    pub fn session_id(&self) -> &str {
        match self {
            Self::V1(value) => &value.session_id,
            Self::V2(value) => &value.session_id,
        }
    }
    /// Current pending interaction identity.
    pub fn id(&self) -> &str {
        match self {
            Self::V1(value) => &value.id,
            Self::V2(value) => &value.id,
        }
    }
}

/// A native event after the upstream adapter enforces directory scope.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NativeEvent {
    /// Original v1 event envelope.
    V1(OpenCodeEvent),
    /// Original v2 event envelope.
    V2(v2::Event),
}

impl NativeEvent {
    /// Upstream event name.
    pub fn kind(&self) -> &str {
        match self {
            Self::V1(value) => &value.kind,
            Self::V2(value) => &value.kind,
        }
    }
}

/// Scoped live events. Dropping the stream disconnects only this observer.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<NativeEvent>> + Send>>;

/// One bounded chronological native message page.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NativePage {
    /// Native messages ordered oldest first.
    pub messages: Vec<NativeMessage>,
    /// Opaque cursor for the next earlier page.
    pub next_cursor: Option<String>,
}

/// Explicit Pocket gateway contract; this is not an upstream OpenCode identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GatewayInfo {
    /// Version of the Pocket transport contract.
    pub gateway_protocol: u32,
    /// Native payload family, v1 or v2.
    pub upstream_protocol: String,
    /// Verified upstream release version.
    pub version: String,
}

/// One negotiated direct or restricted gateway connection.
#[derive(Clone, Debug)]
pub enum Connection {
    /// Existing v1 protocol, unchanged.
    V1(OpenCodeClient),
    /// OpenCode 2.0.18 native protocol.
    V2(v2::V2Client),
    /// Explicitly versioned Pocket gateway transport.
    Gateway(Remote),
}

impl From<OpenCodeClient> for Connection {
    fn from(client: OpenCodeClient) -> Self {
        Self::V1(client)
    }
}

impl From<v2::V2Client> for Connection {
    fn from(client: v2::V2Client) -> Self {
        Self::V2(client)
    }
}

impl Connection {
    /// Negotiate using bounded read-only requests to the same validated origin.
    pub async fn connect(
        origin: &str,
        directory: &str,
        credentials: Option<BasicCredentials>,
    ) -> Result<Self> {
        let legacy = OpenCodeClient::new(origin, directory, credentials.clone())?;
        match Remote::probe(legacy.clone()).await {
            Ok(Some(remote)) => return Ok(Self::Gateway(remote)),
            Ok(None) => {},
            Err(error) => return Err(error),
        }
        match legacy.health().await {
            Ok(_) => {
                legacy.capabilities().await?;
                return Ok(Self::V1(legacy));
            },
            Err(Error::Protocol | Error::Rejected(404)) => {},
            Err(error) => return Err(error),
        }
        let native = v2::V2Client::new(origin, directory, credentials)?;
        native.connect().await?;
        Ok(Self::V2(native))
    }

    /// Selected host directory.
    pub fn directory(&self) -> &str {
        match self {
            Self::V1(c) => c.directory(),
            Self::V2(c) => c.directory(),
            Self::Gateway(c) => c.directory(),
        }
    }

    /// Whether this connection uses native v2 payloads.
    pub fn is_v2(&self) -> bool {
        match self {
            Self::V1(_) => false,
            Self::V2(_) => true,
            Self::Gateway(c) => c.info.upstream_protocol == "v2",
        }
    }

    /// Publish only the gateway contract and upstream version, never
    /// credentials.
    pub async fn gateway_info(&self) -> Result<GatewayInfo> {
        let (version, protocol) = match self {
            Self::V1(c) => (c.health().await?.version, "v1"),
            Self::V2(c) => (c.connect().await?.version, "v2"),
            Self::Gateway(c) => return Ok(c.info.clone()),
        };
        Ok(GatewayInfo {
            gateway_protocol: 2,
            upstream_protocol: protocol.into(),
            version,
        })
    }

    /// List bounded session summaries, retaining native history separately.
    pub async fn sessions(&self, search: Option<&str>) -> Result<Vec<Session>> {
        match self {
            Self::V1(c) => c.sessions(search).await,
            Self::V2(c) => Ok(c
                .sessions(search)
                .await?
                .into_iter()
                .map(session_summary)
                .collect()),
            Self::Gateway(c) => c.sessions(search).await,
        }
    }

    /// Read and authorize the session's current location.
    pub async fn session(&self, id: &str) -> Result<Session> {
        match self {
            Self::V1(c) => c.session(id).await,
            Self::V2(c) => Ok(session_summary(c.session(id).await?)),
            Self::Gateway(c) => c.session(id).await,
        }
    }

    /// Read one bounded chronological history page.
    pub async fn history(&self, id: &str, limit: u32, before: Option<&str>) -> Result<NativePage> {
        match self {
            Self::V1(c) => {
                let page = c.history(id, limit, before).await?;
                Ok(NativePage {
                    messages: page.messages.into_iter().map(NativeMessage::V1).collect(),
                    next_cursor: page.next_cursor,
                })
            },
            Self::V2(c) => {
                let page = c.history(id, limit, before).await?;
                Ok(NativePage {
                    messages: page.messages.into_iter().map(NativeMessage::V2).collect(),
                    next_cursor: page.next_cursor,
                })
            },
            Self::Gateway(c) => c.history(id, limit, before).await,
        }
    }

    /// Read an authoritative native message within its selected session.
    pub async fn message(&self, id: &str, message_id: &str) -> Result<NativeMessage> {
        match self {
            Self::V1(c) => Ok(NativeMessage::V1(c.message(id, message_id).await?)),
            Self::V2(c) => Ok(NativeMessage::V2(c.message(id, message_id).await?)),
            Self::Gateway(c) => c.message(id, message_id).await,
        }
    }

    /// Create an empty session, without invoking a model.
    pub async fn create(&self, title: Option<&str>) -> Result<Session> {
        match self {
            Self::V1(c) => c.create(title).await,
            Self::V2(c) => Ok(session_summary(c.create(title).await?)),
            Self::Gateway(c) => c.create(title).await,
        }
    }

    /// Submit plain text once; acceptance is not model completion.
    pub async fn prompt(&self, id: &str, input: &PromptInput) -> Result<()> {
        match self {
            Self::V1(c) => c.prompt(id, input).await,
            Self::V2(c) => {
                if input.parts.len() != 1 {
                    return Err(Error::InvalidInput);
                }
                let super::PromptPart::Text {
                    text,
                } = &input.parts[0];
                c.prompt(id, text, input.message_id.as_deref()).await?;
                Ok(())
            },
            Self::Gateway(c) => c.prompt(id, input).await,
        }
    }

    /// Current statuses normalized only for the controller's busy indicator.
    pub async fn status(&self) -> Result<BTreeMap<String, Value>> {
        match self {
            Self::V1(c) => c.status().await,
            Self::V2(c) => Ok(c
                .active()
                .await?
                .into_keys()
                .map(|id| (id, serde_json::json!({"type":"busy"})))
                .collect()),
            Self::Gateway(c) => c.status().await,
        }
    }

    /// Read authoritative scoped permissions, preserving native semantics.
    pub async fn permissions(&self) -> Result<Vec<NativePermission>> {
        match self {
            Self::V1(c) => Ok(c
                .permissions()
                .await?
                .into_iter()
                .map(NativePermission::V1)
                .collect()),
            Self::V2(c) => Ok(c
                .permissions()
                .await?
                .into_iter()
                .map(NativePermission::V2)
                .collect()),
            Self::Gateway(c) => c.permissions().await,
        }
    }

    /// Reply once to a currently pending scoped permission.
    pub async fn reply_permission(
        &self,
        id: &str,
        reply: PermissionReply,
        message: Option<&str>,
    ) -> Result<()> {
        match self {
            Self::V1(c) => c.reply_permission(id, reply, message).await,
            Self::V2(c) => c.reply_permission(id, reply, message).await,
            Self::Gateway(c) => c.reply_permission(id, reply, message).await,
        }
    }

    /// Read current questions or typed forms, never historical approvals.
    pub async fn questions(&self) -> Result<Vec<NativeQuestion>> {
        match self {
            Self::V1(c) => Ok(c
                .questions()
                .await?
                .into_iter()
                .map(NativeQuestion::V1)
                .collect()),
            Self::V2(c) => Ok(c
                .forms()
                .await?
                .into_iter()
                .map(NativeQuestion::V2)
                .collect()),
            Self::Gateway(c) => c.questions().await,
        }
    }

    /// Reply to v1 ordered questions. Typed v2 forms have a separate interface.
    pub async fn reply_question(&self, id: &str, answers: Vec<Vec<String>>) -> Result<()> {
        match self {
            Self::V1(c) => c.reply_question(id, answers).await,
            Self::V2(_) => Err(Error::InvalidInput),
            Self::Gateway(c) => c.reply_question(id, answers).await,
        }
    }

    /// Reply to a v2 form using typed keyed values, without schema coercion.
    pub async fn reply_form(&self, id: &str, answers: Value) -> Result<()> {
        match self {
            Self::V1(_) => Err(Error::InvalidInput),
            Self::V2(c) => c.reply_form(id, answers).await,
            Self::Gateway(c) => c.reply_form(id, answers).await,
        }
    }

    /// Reject one current question/form without disposing the service.
    pub async fn reject_question(&self, id: &str) -> Result<()> {
        match self {
            Self::V1(c) => c.reject_question(id).await,
            Self::V2(c) => c.reject_form(id).await,
            Self::Gateway(c) => c.reject_question(id).await,
        }
    }

    /// Interrupt only the selected session; never stop the server process.
    pub async fn abort(&self, id: &str) -> Result<()> {
        match self {
            Self::V1(c) => c.abort(id).await,
            Self::V2(c) => {
                c.abort(id).await?;
                Ok(())
            },
            Self::Gateway(c) => c.abort(id).await,
        }
    }

    /// Open a scoped native stream. Losing it requires state reconciliation.
    pub async fn events(&self) -> Result<EventStream> {
        match self {
            Self::V1(c) => Ok(Box::pin(c.events().await?.map(|event| event.map(NativeEvent::V1)))),
            Self::V2(c) => Ok(Box::pin(c.events().await?.map(|event| event.map(NativeEvent::V2)))),
            Self::Gateway(c) => c.events().await,
        }
    }
}

fn session_summary(session: v2::Session) -> Session {
    Session {
        id: session.id,
        title: session.title.unwrap_or_else(|| "Untitled".into()),
        directory: session.location.directory,
        workspace_id: None,
        extra: session.extra,
    }
}

//! `session/update` notification payloads, discriminated by `sessionUpdate`.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::types::{
    AvailableCommand, ConfigOption, ContentBlock, PlanEntry, ToolCall, ToolCallUpdate, UsageUpdate,
};

/// A streamed chunk of a user, agent or thought message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageChunk {
    /// The content.
    pub content: ContentBlock,
    /// Groups chunks of one message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One `session/update` payload.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionUpdate {
    /// `user_message_chunk`
    UserMessageChunk(MessageChunk),
    /// `agent_message_chunk`
    AgentMessageChunk(MessageChunk),
    /// `agent_thought_chunk`
    AgentThoughtChunk(MessageChunk),
    /// `tool_call`
    ToolCall(ToolCall),
    /// `tool_call_update`
    ToolCallUpdate(ToolCallUpdate),
    /// `plan`
    Plan {
        /// The complete plan.
        entries: Vec<PlanEntry>,
    },
    /// `available_commands_update`
    AvailableCommandsUpdate {
        /// All commands.
        available_commands: Vec<AvailableCommand>,
    },
    /// `current_mode_update`
    CurrentModeUpdate {
        /// New mode id.
        current_mode_id: String,
    },
    /// `config_option_update`
    ConfigOptionUpdate {
        /// All options.
        config_options: Vec<ConfigOption>,
    },
    /// `session_info_update`
    SessionInfoUpdate {
        /// Outer `None` = omitted (unchanged); `Some(None)` = explicit null
        /// (cleared).
        title: Option<Option<String>>,
        /// Same convention as `title`.
        updated_at: Option<Option<String>>,
    },
    /// `usage_update`
    UsageUpdate(UsageUpdate),
    /// A variant this build does not know (or a malformed known one), kept
    /// verbatim.
    Unknown(Value),
}

const TAG: &str = "sessionUpdate";

impl SessionUpdate {
    /// The `sessionUpdate` discriminator of this payload.
    pub fn tag(&self) -> Option<&str> {
        Some(match self {
            Self::UserMessageChunk(_) => "user_message_chunk",
            Self::AgentMessageChunk(_) => "agent_message_chunk",
            Self::AgentThoughtChunk(_) => "agent_thought_chunk",
            Self::ToolCall(_) => "tool_call",
            Self::ToolCallUpdate(_) => "tool_call_update",
            Self::Plan {
                ..
            } => "plan",
            Self::AvailableCommandsUpdate {
                ..
            } => "available_commands_update",
            Self::CurrentModeUpdate {
                ..
            } => "current_mode_update",
            Self::ConfigOptionUpdate {
                ..
            } => "config_option_update",
            Self::SessionInfoUpdate {
                ..
            } => "session_info_update",
            Self::UsageUpdate(_) => "usage_update",
            Self::Unknown(value) => return value.get(TAG).and_then(Value::as_str),
        })
    }
}

fn field<T: serde::de::DeserializeOwned>(map: &Map<String, Value>, name: &str) -> Option<T> {
    T::deserialize(map.get(name)?).ok()
}

fn nullable_string(map: &Map<String, Value>, name: &str) -> Result<Option<Option<String>>, ()> {
    match map.get(name) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => Ok(Some(Some(s.clone()))),
        Some(_) => Err(()),
    }
}

fn parse(value: &Value) -> Option<SessionUpdate> {
    let map = value.as_object()?;
    let chunk = || MessageChunk::deserialize(value).ok();
    Some(match map.get(TAG)?.as_str()? {
        "user_message_chunk" => SessionUpdate::UserMessageChunk(chunk()?),
        "agent_message_chunk" => SessionUpdate::AgentMessageChunk(chunk()?),
        "agent_thought_chunk" => SessionUpdate::AgentThoughtChunk(chunk()?),
        "tool_call" => SessionUpdate::ToolCall(ToolCall::deserialize(value).ok()?),
        "tool_call_update" => {
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::deserialize(value).ok()?)
        },
        "plan" => SessionUpdate::Plan {
            entries: field(map, "entries")?,
        },
        "available_commands_update" => SessionUpdate::AvailableCommandsUpdate {
            available_commands: field(map, "availableCommands")?,
        },
        "current_mode_update" => SessionUpdate::CurrentModeUpdate {
            current_mode_id: field(map, "currentModeId")?,
        },
        "config_option_update" => SessionUpdate::ConfigOptionUpdate {
            config_options: field(map, "configOptions")?,
        },
        "session_info_update" => SessionUpdate::SessionInfoUpdate {
            title: nullable_string(map, "title").ok()?,
            updated_at: nullable_string(map, "updatedAt").ok()?,
        },
        "usage_update" => SessionUpdate::UsageUpdate(UsageUpdate::deserialize(value).ok()?),
        _ => return None,
    })
}

impl<'de> Deserialize<'de> for SessionUpdate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("session update is not an object"));
        }
        Ok(parse(&value).unwrap_or(Self::Unknown(value)))
    }
}

fn object<S: Serializer, P: Serialize>(payload: &P) -> Result<Map<String, Value>, S::Error> {
    match serde_json::to_value(payload).map_err(serde::ser::Error::custom)? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

impl Serialize for SessionUpdate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = match self {
            Self::Unknown(value) => return value.serialize(serializer),
            Self::UserMessageChunk(c) | Self::AgentMessageChunk(c) | Self::AgentThoughtChunk(c) => {
                object::<S, _>(c)?
            },
            Self::ToolCall(t) => object::<S, _>(t)?,
            Self::ToolCallUpdate(t) => object::<S, _>(t)?,
            Self::UsageUpdate(u) => object::<S, _>(u)?,
            Self::Plan {
                entries,
            } => {
                let mut map = Map::new();
                map.insert("entries".into(), serde_json::to_value(entries).map_err(ser)?);
                map
            },
            Self::AvailableCommandsUpdate {
                available_commands,
            } => {
                let mut map = Map::new();
                map.insert(
                    "availableCommands".into(),
                    serde_json::to_value(available_commands).map_err(ser)?,
                );
                map
            },
            Self::CurrentModeUpdate {
                current_mode_id,
            } => {
                let mut map = Map::new();
                map.insert("currentModeId".into(), Value::String(current_mode_id.clone()));
                map
            },
            Self::ConfigOptionUpdate {
                config_options,
            } => {
                let mut map = Map::new();
                map.insert(
                    "configOptions".into(),
                    serde_json::to_value(config_options).map_err(ser)?,
                );
                map
            },
            Self::SessionInfoUpdate {
                title,
                updated_at,
            } => {
                let mut map = Map::new();
                for (name, value) in [("title", title), ("updatedAt", updated_at)] {
                    if let Some(value) = value {
                        map.insert(name.into(), value.clone().map_or(Value::Null, Value::String));
                    }
                }
                map
            },
        };
        if let Some(tag) = self.tag() {
            map.insert(TAG.into(), Value::String(tag.to_string()));
        }
        Value::Object(map).serialize(serializer)
    }
}

fn ser<E: serde::ser::Error>(error: serde_json::Error) -> E {
    E::custom(error)
}

/// `session/update` notification params.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNotification {
    /// Session id.
    pub session_id: String,
    /// The update.
    pub update: SessionUpdate,
    /// Extension metadata (the hub adds `_meta.pcx` here, T14).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn unknown_session_update_round_trips() {
        let raw = json!({"sessionUpdate": "future_thing", "x": [1, 2], "_meta": {"a": 1}});
        let update: SessionUpdate = serde_json::from_value(raw.clone()).expect("de");
        assert!(matches!(update, SessionUpdate::Unknown(_)));
        assert_eq!(update.tag(), Some("future_thing"));
        assert_eq!(serde_json::to_value(&update).expect("ser"), raw);

        // Other fields of the same notification are unaffected.
        let note: SessionNotification = serde_json::from_value(json!({
            "sessionId": "s1", "update": raw, "_meta": {"k": true}
        }))
        .expect("notification");
        assert_eq!(note.session_id, "s1");
        assert_eq!(note.meta, Some(json!({"k": true})));

        // A known variant that is malformed is kept verbatim too.
        let broken = json!({"sessionUpdate": "plan", "entries": "nope"});
        let update: SessionUpdate = serde_json::from_value(broken.clone()).expect("de");
        assert_eq!(update, SessionUpdate::Unknown(broken));
    }

    #[test]
    fn session_info_update_distinguishes_null_from_omitted() {
        let omitted: SessionUpdate =
            serde_json::from_value(json!({"sessionUpdate": "session_info_update"})).expect("de");
        assert_eq!(omitted, SessionUpdate::SessionInfoUpdate {
            title: None,
            updated_at: None
        });
        let cleared: SessionUpdate = serde_json::from_value(
            json!({"sessionUpdate": "session_info_update", "title": null, "updatedAt": "t"}),
        )
        .expect("de");
        assert_eq!(cleared, SessionUpdate::SessionInfoUpdate {
            title: Some(None),
            updated_at: Some(Some("t".into()))
        });
        assert_eq!(
            serde_json::to_value(&cleared).expect("ser"),
            json!({"sessionUpdate": "session_info_update", "title": null, "updatedAt": "t"})
        );
        assert_eq!(
            serde_json::to_value(&omitted).expect("ser"),
            json!({"sessionUpdate": "session_info_update"})
        );
    }

    #[test]
    fn known_variants_round_trip() {
        for raw in [
            json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}, "messageId": "m1"}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read", "kind": "read", "status": "pending"}),
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
            json!({"sessionUpdate": "plan", "entries": [{"content": "a", "priority": "high", "status": "pending"}]}),
            json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}),
            json!({"sessionUpdate": "usage_update", "used": 10, "size": 100}),
        ] {
            let update: SessionUpdate = serde_json::from_value(raw.clone()).expect("de");
            assert!(!matches!(update, SessionUpdate::Unknown(_)), "{raw}");
            assert_eq!(serde_json::to_value(&update).expect("ser"), raw);
        }
    }
}

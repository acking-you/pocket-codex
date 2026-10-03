//! Translation of OpenCode's live event stream into the app-server-shaped
//! [`AppEvent`]s the session UI consumes.

use std::collections::HashMap;

use pocket_codex_host_svc::opencode::{Event, Form, Permission};
use serde_json::{json, Value};

use super::mapping;
use crate::engine::{
    app_events::{bare_item, event, item_event},
    app_session::AppEvent,
};

/// Bound on remembered tool calls (name + input between start and result).
const MAX_TOOLS: usize = 2048;

/// Per-connection translation state.
#[derive(Default)]
pub struct Translator {
    /// session → turn id (user message id) of its current / latest turn.
    pub turns: HashMap<String, String>,
    /// (message id, call id) → (tool name, latest input).
    tools: HashMap<(String, String), (String, Value)>,
}

/// A request that was answered or withdrawn: the UI drops its card.
pub fn resolved(session: &str, request_id: &str) -> AppEvent {
    event("serverRequest/resolved", session, json!({"threadId": session, "requestId": request_id}))
}

impl Translator {
    /// The turn id of a session's current turn (synthetic when unknown).
    pub fn turn_id(&self, session: &str) -> String {
        self.turns
            .get(session)
            .cloned()
            .unwrap_or_else(|| format!("live:{session}"))
    }

    /// Translate one native event; unknown events translate to nothing.
    pub fn translate(&mut self, ev: &Event) -> Vec<AppEvent> {
        let name = ev.kind.strip_prefix("session.").unwrap_or(&ev.kind);
        let Some(session) = ev.session_id().map(str::to_string) else {
            return Vec::new();
        };
        let s = session.as_str();
        let data = &ev.data;
        let message = data["assistantMessageID"].as_str().unwrap_or("");
        let ordinal = data["ordinal"].as_u64().unwrap_or(0);
        match name {
            "execution.started" => {
                let turn = self.turn_id(s);
                let raw = json!({"threadId": s, "turnId": turn, "turn": {"id": turn, "status": "inProgress"}});
                vec![event("turn/started", s, raw)]
            },
            "execution.succeeded" | "execution.failed" | "execution.interrupted" => {
                self.finished(s, name, data)
            },
            "text.started" | "reasoning.started" => {
                let (kind, tag) = text_kind(name);
                let item = bare_item(mapping::part_id(message, tag, ordinal), kind);
                vec![item_event("item/started", s, &item, Some(String::new()))]
            },
            "text.delta" | "reasoning.delta" => {
                let (kind, tag) = text_kind(name);
                let method = if kind == "reasoning" {
                    "item/reasoning/textDelta"
                } else {
                    "item/agentMessage/delta"
                };
                let item = bare_item(mapping::part_id(message, tag, ordinal), kind);
                vec![item_event(method, s, &item, data["delta"].as_str().map(str::to_string))]
            },
            "text.ended" | "reasoning.ended" => {
                let (kind, tag) = text_kind(name);
                let item = bare_item(mapping::part_id(message, tag, ordinal), kind);
                vec![item_event(
                    "item/completed",
                    s,
                    &item,
                    data["text"].as_str().map(str::to_string),
                )]
            },
            "tool.input.started" | "tool.called" | "tool.progress" | "tool.success"
            | "tool.failed" => self.tool(s, name, message, data),
            "compaction.ended" => vec![event("thread/compacted", s, json!({"threadId": s}))],
            "renamed" => {
                vec![event("thread/name/updated", s, json!({"threadId": s, "name": data["title"]}))]
            },
            "created" => vec![event("thread/started", s, json!({"thread": {"id": s}}))],
            "permission.asked" => match serde_json::from_value::<Permission>(data.clone()) {
                Ok(permission) => {
                    let directory = ev
                        .location
                        .as_ref()
                        .map(|l| l.directory.as_str())
                        .unwrap_or("");
                    vec![mapping::permission_event(&permission, directory)]
                },
                Err(_) => Vec::new(),
            },
            "form.created" => match serde_json::from_value::<Form>(data["form"].clone()) {
                Ok(form) => vec![mapping::form_event(&form)],
                Err(_) => Vec::new(),
            },
            "permission.replied" => data["requestID"]
                .as_str()
                .map(|id| vec![resolved(s, id)])
                .unwrap_or_default(),
            "form.replied" | "form.cancelled" => data["id"]
                .as_str()
                .map(|id| vec![resolved(s, id)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn finished(&mut self, s: &str, name: &str, data: &Value) -> Vec<AppEvent> {
        let turn = self.turn_id(s);
        let status = match name {
            "execution.succeeded" => "completed",
            "execution.failed" => "failed",
            _ => "interrupted",
        };
        let mut turn_raw = json!({"id": turn, "status": status});
        if let Some(message) = data["error"]["message"]
            .as_str()
            .or_else(|| data["error"].as_str())
        {
            turn_raw["error"] = json!({"message": message});
        }
        vec![event("turn/completed", s, json!({"threadId": s, "turn": turn_raw}))]
    }

    fn tool(&mut self, s: &str, name: &str, message: &str, data: &Value) -> Vec<AppEvent> {
        let call = data["id"].as_str().unwrap_or("").to_string();
        let key = (message.to_string(), call.clone());
        if self.tools.len() >= MAX_TOOLS && !self.tools.contains_key(&key) {
            self.tools.clear();
        }
        let entry = self
            .tools
            .entry(key.clone())
            .or_insert_with(|| (String::new(), Value::Null));
        if let Some(tool) = data["name"].as_str() {
            entry.0 = tool.to_string();
        }
        if !data["input"].is_null() {
            entry.1 = data["input"].clone();
        }
        let (tool, input) = entry.clone();
        let (status, done) = match name {
            "tool.success" => ("completed", true),
            "tool.failed" => ("error", true),
            "tool.input.started" => ("streaming", false),
            _ => ("running", false),
        };
        let part = json!({
            "type": "tool", "id": call, "name": tool,
            "state": {
                "status": status, "input": input,
                "content": data["content"], "metadata": data["metadata"], "error": data["error"],
            },
        });
        let item = mapping::tool_item(message, &part);
        if done {
            self.tools.remove(&key);
        }
        let method = if done { "item/completed" } else { "item/started" };
        vec![item_event(method, s, &item, Some(item.text.clone()))]
    }
}

fn text_kind(name: &str) -> (&'static str, char) {
    if name.starts_with("reasoning") {
        ("reasoning", 'r')
    } else {
        ("agentMessage", 't')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(value: Value) -> Event {
        serde_json::from_value(value).expect("event")
    }

    #[test]
    fn a_live_turn_translates_to_app_server_events() {
        let mut t = Translator::default();
        t.turns.insert("ses_1".into(), "msg_u".into());
        let mut kinds = Vec::new();
        for value in [
            json!({"type": "session.execution.started", "data": {"sessionID": "ses_1"}}),
            json!({"type": "session.text.started", "data": {"sessionID": "ses_1", "assistantMessageID": "msg_a", "ordinal": 0}}),
            json!({"type": "session.text.delta", "data": {"sessionID": "ses_1", "assistantMessageID": "msg_a", "ordinal": 0, "delta": "he"}}),
            json!({"type": "session.tool.input.started", "data": {"sessionID": "ses_1", "assistantMessageID": "msg_a", "id": "c1", "name": "shell"}}),
            json!({"type": "session.tool.called", "data": {"sessionID": "ses_1", "assistantMessageID": "msg_a", "id": "c1", "input": {"command": "ls"}}}),
            json!({"type": "session.tool.success", "data": {"sessionID": "ses_1", "assistantMessageID": "msg_a", "id": "c1",
                "content": [{"type": "text", "text": "out"}], "metadata": {"exit": 0}}}),
            json!({"type": "session.execution.interrupted", "data": {"sessionID": "ses_1"}}),
            json!({"type": "session.unknown", "data": {"sessionID": "ses_1"}}),
        ] {
            for out in t.translate(&ev(value)) {
                kinds.push((
                    out.kind.clone(),
                    out.item_id.clone(),
                    out.text.clone(),
                    out.title.clone(),
                ));
            }
        }
        assert_eq!(kinds[0].0, "turn/started");
        assert_eq!(
            kinds[2],
            (
                "item/agentMessage/delta".into(),
                Some("msg_a:t0".into()),
                Some("he".into()),
                Some(String::new())
            )
        );
        assert_eq!(kinds[4].3.as_deref(), Some("ls"));
        assert_eq!(kinds[5].0, "item/completed");
        assert_eq!(kinds[5].2.as_deref(), Some("out\n[exit 0]"));
        assert_eq!(kinds.last().expect("last").0, "turn/completed");
        assert_eq!(kinds.len(), 7);
    }

    #[test]
    fn requests_and_their_resolution() {
        let mut t = Translator::default();
        let asked = t.translate(&ev(json!({
            "type": "permission.asked",
            "location": {"directory": "/w"},
            "data": {"id": "per_1", "sessionID": "ses_1", "action": "shell", "resources": ["ls"]}
        })));
        assert_eq!(asked[0].request_id.as_deref(), Some("per_1"));
        let done = t.translate(&ev(json!({"type": "permission.replied", "data": {"sessionID": "ses_1", "requestID": "per_1", "reply": "once"}})));
        assert_eq!(done[0].kind, "serverRequest/resolved");
        let form = t.translate(&ev(json!({"type": "form.created", "data": {"form": {
            "id": "frm_1", "sessionID": "ses_1", "title": "Q", "fields": [{"key": "a", "type": "string"}]}}})));
        assert_eq!(form[0].kind, "item/tool/requestUserInput");
        assert!(t
            .translate(&ev(
                json!({"type": "form.created", "data": {"form": {"sessionID": "global"}}})
            ))
            .is_empty());
    }
}

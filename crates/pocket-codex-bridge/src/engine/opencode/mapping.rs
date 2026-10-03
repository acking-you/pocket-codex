//! Pure translation of OpenCode v2 wire objects into the item, turn and model
//! shapes the shared session UI already renders.
//!
//! OpenCode has no turns; a **turn** here is one user message plus everything
//! after it up to the next user message, identified by that user message id.

use std::collections::HashMap;

use pocket_codex_host_svc::opencode::{forms, Form, Message, Permission, Session};
use serde_json::{json, Map, Value};

use crate::engine::app_session::{
    encode_plan, format_content_diff, AppEvent, ModelInfo, ThreadItem, ThreadMeta, TurnSummary,
};

/// Largest inline attachment turned into a data URL for display.
const MAX_INLINE_IMAGE: usize = 8 * 1024 * 1024;

/// One user message that opens a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnStart {
    /// User message id (the turn id).
    pub id: String,
    /// Creation time, milliseconds.
    pub created_ms: i64,
    /// Prompt text.
    pub text: String,
}

impl TurnStart {
    /// From a native `user` message.
    pub fn from_message(message: &Message) -> Option<Self> {
        (message.kind == "user").then(|| Self {
            id: message.id.clone(),
            created_ms: message.created_ms(),
            text: str_of(message.extra.get("text")).to_string(),
        })
    }
}

fn str_of(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or("")
}

/// The turn a message belongs to: a user message opens its own; anything else
/// belongs to the latest turn that started at or before it (`starts` are
/// chronological). Empty when no known turn precedes it.
pub fn turn_of(message: &Message, starts: &[TurnStart]) -> String {
    if message.kind == "user" {
        return message.id.clone();
    }
    let created = message.created_ms();
    starts
        .iter()
        .rev()
        .find(|start| start.created_ms <= created)
        .map(|start| start.id.clone())
        .unwrap_or_default()
}

/// Items of a chronological message window, stamped with their turn and the
/// turn's completion (except for `running_turn`, which is still executing).
pub fn map_window(
    messages: &[Message],
    starts: &[TurnStart],
    running_turn: Option<&str>,
) -> Vec<ThreadItem> {
    let mut items = Vec::new();
    // turn id -> latest assistant completion (ms) seen in this window.
    let mut completed: HashMap<String, i64> = HashMap::new();
    let mut known: Vec<TurnStart> = starts.to_vec();
    for message in messages {
        if let Some(start) = TurnStart::from_message(message) {
            if !known.iter().any(|s| s.id == start.id) {
                let at = known.partition_point(|s| s.created_ms <= start.created_ms);
                known.insert(at, start);
            }
        }
        let turn = turn_of(message, &known);
        if let Some(done) = message
            .extra
            .get("time")
            .and_then(|t| t["completed"].as_f64())
        {
            let entry = completed.entry(turn.clone()).or_insert(0);
            *entry = (*entry).max(done as i64);
        }
        for mut item in message_items(message) {
            item.turn_id.clone_from(&turn);
            items.push(item);
        }
    }
    for item in &mut items {
        if item.turn_id.is_empty() || running_turn == Some(item.turn_id.as_str()) {
            continue;
        }
        if let Some(done) = completed.get(&item.turn_id) {
            item.turn_completed_at = Some(done / 1000);
            if let Some(start) = known.iter().find(|s| s.id == item.turn_id) {
                item.turn_duration_ms = Some((done - start.created_ms).max(0));
            }
        }
    }
    items
}

fn item(
    id: String,
    item_type: &str,
    title: impl Into<String>,
    text: impl Into<String>,
) -> ThreadItem {
    ThreadItem {
        id,
        item_type: item_type.to_string(),
        title: title.into(),
        text: text.into(),
        questions_json: None,
        images: Vec::new(),
        turn_id: String::new(),
        turn_completed_at: None,
        turn_duration_ms: None,
    }
}

/// Item id of the `ordinal`-th text (`t`) or reasoning (`r`) part.
pub fn part_id(message_id: &str, kind: char, ordinal: u64) -> String {
    format!("{message_id}:{kind}{ordinal}")
}

/// Item id of a tool call.
pub fn tool_id(message_id: &str, call_id: &str) -> String {
    format!("{message_id}:{call_id}")
}

/// Items shown for one native message (turn fields left empty).
pub fn message_items(message: &Message) -> Vec<ThreadItem> {
    let extra = &message.extra;
    match message.kind.as_str() {
        "user" => {
            let mut user = item(message.id.clone(), "userMessage", "", str_of(extra.get("text")));
            user.images = user_images(extra.get("files"));
            vec![user]
        },
        "assistant" => {
            let mut out = Vec::new();
            let (mut texts, mut thoughts) = (0u64, 0u64);
            for part in extra
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                match part["type"].as_str() {
                    Some("text") => {
                        let text = str_of(part.get("text"));
                        if !text.trim().is_empty() {
                            out.push(item(
                                part_id(&message.id, 't', texts),
                                "agentMessage",
                                "",
                                text,
                            ));
                        }
                        texts += 1;
                    },
                    Some("reasoning") => {
                        let text = str_of(part.get("text"));
                        if !text.trim().is_empty() {
                            out.push(item(
                                part_id(&message.id, 'r', thoughts),
                                "reasoning",
                                "",
                                text,
                            ));
                        }
                        thoughts += 1;
                    },
                    Some("tool") => out.push(tool_item(&message.id, part)),
                    _ => {},
                }
            }
            if let Some(error) = extra.get("error").filter(|e| !e.is_null()) {
                let text = error["message"]
                    .as_str()
                    .map_or_else(|| error.to_string(), str::to_string);
                out.push(item(format!("{}:error", message.id), "error", "error", text));
            }
            out
        },
        "shell" => {
            let mut text = str_of(extra.get("output")).to_string();
            if let Some(code) = extra.get("exit").and_then(Value::as_i64) {
                push_exit(&mut text, code);
            }
            vec![item(message.id.clone(), "commandExecution", str_of(extra.get("command")), text)]
        },
        "compaction" => {
            let status = str_of(extra.get("status"));
            let title = if status == "running" { "inProgress" } else { status };
            let text = extra
                .get("summary")
                .and_then(Value::as_str)
                .or_else(|| extra.get("error").and_then(|e| e["message"].as_str()))
                .unwrap_or("");
            vec![item(message.id.clone(), "contextCompaction", title, text)]
        },
        _ => Vec::new(),
    }
}

fn user_images(files: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    for file in files.and_then(Value::as_array).into_iter().flatten() {
        let mime = str_of(file.get("mime"));
        if !mime.starts_with("image/") {
            continue;
        }
        let data = str_of(file.get("data"));
        if !data.is_empty() && data.len() <= MAX_INLINE_IMAGE {
            out.push(format!("data:{mime};base64,{data}"));
        } else if let Some(uri) = file["source"]["uri"].as_str() {
            out.push(uri.strip_prefix("file://").unwrap_or(uri).to_string());
        }
    }
    out
}

fn push_exit(text: &mut String, code: i64) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("[exit {code}]"));
}

fn tool_text(state: &Value) -> String {
    let mut text = String::new();
    for part in state
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(t) = part["text"].as_str() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(t);
        }
    }
    text
}

/// The item for one assistant tool part.
pub fn tool_item(message_id: &str, part: &Value) -> ThreadItem {
    let name = str_of(part.get("name"));
    let state = &part["state"];
    let status = str_of(state.get("status"));
    let input = if state["input"].is_object() {
        state["input"].clone()
    } else {
        // A streaming call carries its partial input as raw text.
        serde_json::from_str(str_of(state.get("input"))).unwrap_or(Value::Null)
    };
    let metadata = &state["metadata"];
    let id = tool_id(message_id, str_of(part.get("id")));
    let mut result = match name {
        "shell" | "bash" => {
            let mut text = tool_text(state);
            if let Some(code) = metadata["exit"].as_i64() {
                push_exit(&mut text, code);
            }
            item(id, "commandExecution", str_of(input.get("command")), text)
        },
        "edit" | "write" | "apply_patch" | "patch" | "multiedit" => {
            file_change(id, &input, metadata)
        },
        "todowrite" => {
            let plan: Vec<Value> = input["todos"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|todo| {
                    let status = match todo["status"].as_str() {
                        Some("in_progress") => "in_progress",
                        Some("completed") => "completed",
                        _ => "pending",
                    };
                    json!({"step": todo["content"], "status": status})
                })
                .collect();
            item(id, "plan", "", encode_plan(&json!({"plan": plan})))
        },
        "webfetch" | "websearch" => {
            let title = input["url"]
                .as_str()
                .or_else(|| input["query"].as_str())
                .unwrap_or(name);
            item(id, "webSearch", title, tool_text(state))
        },
        "subagent" | "task" => {
            let child = metadata["sessionID"]
                .as_str()
                .or_else(|| input["sessionID"].as_str());
            let title = match (input["agent"].as_str(), input["description"].as_str()) {
                (Some(agent), Some(description)) => format!("{agent}: {description}"),
                (Some(agent), None) => agent.to_string(),
                (None, Some(description)) => description.to_string(),
                (None, None) => name.to_string(),
            };
            let detail = json!({
                "status": status,
                "prompt": input["prompt"],
                "receiverThreadIds": child.map(|c| vec![c]).unwrap_or_default(),
                "result": tool_text(state),
            });
            item(id, "collabAgentToolCall", title, detail.to_string())
        },
        _ => {
            let title = primary_argument(&input)
                .map_or_else(|| name.to_string(), |arg| format!("{name} {arg}"));
            let detail = json!({"tool": name, "input": input, "output": tool_text(state)});
            item(id, "dynamicToolCall", title, detail.to_string())
        },
    };
    if status == "error" {
        let error = state["error"]["message"]
            .as_str()
            .or_else(|| state["error"].as_str())
            .unwrap_or("tool failed");
        if !result.text.is_empty() {
            result.text.push('\n');
        }
        result.text.push_str(&format!("[error] {error}"));
    }
    result
}

fn primary_argument(input: &Value) -> Option<String> {
    ["path", "pattern", "filePath", "url", "query", "id", "name"]
        .iter()
        .find_map(|key| input[*key].as_str())
        .map(|value| value.chars().take(160).collect())
}

fn file_change(id: String, input: &Value, metadata: &Value) -> ThreadItem {
    let patches: Vec<(String, String)> = metadata["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some((file["file"].as_str()?.to_string(), file["patch"].as_str()?.to_string()))
        })
        .collect();
    if !patches.is_empty() {
        let title = match patches.as_slice() {
            [(one, _)] => one.clone(),
            many => format!("{} files", many.len()),
        };
        let text = patches
            .iter()
            .map(|(_, p)| p.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return item(id, "fileChange", title, text);
    }
    let path = str_of(input.get("path").or_else(|| input.get("filePath"))).to_string();
    let text = if let Some(content) = input["content"].as_str() {
        format_content_diff(&path, content, true)
    } else if let (Some(old), Some(new)) =
        (input["oldString"].as_str(), input["newString"].as_str())
    {
        let mut diff = format!(
            "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@",
            old.lines().count(),
            new.lines().count()
        );
        for line in old.lines() {
            diff.push_str(&format!("\n-{line}"));
        }
        for line in new.lines() {
            diff.push_str(&format!("\n+{line}"));
        }
        diff
    } else {
        String::new()
    };
    item(id, "fileChange", path, text)
}

/// Rail summaries: one per turn start, with the last agent prose loaded in
/// `items` (if any).
pub fn summaries(starts: &[TurnStart], items: &[ThreadItem]) -> Vec<TurnSummary> {
    starts
        .iter()
        .map(|start| {
            let mut loaded = false;
            let mut last = "";
            for item in items.iter().filter(|i| i.turn_id == start.id) {
                loaded = true;
                if item.item_type == "agentMessage" {
                    last = &item.text;
                }
            }
            TurnSummary {
                turn_id: start.id.clone(),
                user_text: start.text.clone(),
                assistant_text: last.to_string(),
                loaded,
            }
        })
        .collect()
}

/// Sidebar metadata for a session.
pub fn session_meta(session: &Session) -> ThreadMeta {
    let title = session.title.clone().filter(|t| !t.trim().is_empty());
    ThreadMeta {
        id: session.id.clone(),
        preview: title.clone().unwrap_or_default(),
        name: title,
        cwd: session.location.directory.clone(),
        updated_at: session.updated_ms() / 1000,
    }
}

/// A model offered to the picker, or `None` when disabled.
pub fn model_info(model: &Value, default: Option<&str>) -> Option<ModelInfo> {
    if model["enabled"] == false {
        return None;
    }
    let id = format!("{}/{}", model["providerID"].as_str()?, model["id"].as_str()?);
    let variants = model["variants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["id"].as_str().map(str::to_string))
        .collect();
    Some(ModelInfo {
        display_name: model["name"].as_str().unwrap_or(&id).to_string(),
        description: model["family"].as_str().unwrap_or("").to_string(),
        supported_reasoning_efforts: variants,
        default_reasoning_effort: None,
        supported_service_tiers: Vec::new(),
        default_service_tier: None,
        is_default: default == Some(id.as_str()),
        id,
    })
}

/// The approval card event for a pending permission.
pub fn permission_event(permission: &Permission, directory: &str) -> AppEvent {
    let mut command = permission.action.clone();
    if !permission.resources.is_empty() {
        command.push(' ');
        command.push_str(&permission.resources.join(" "));
    }
    let raw = json!({
        "threadId": permission.session_id,
        "command": command,
        "cwd": directory,
        "reason": permission.message,
        "persistsProject": permission.save.as_ref().is_some_and(|s| !s.is_empty()),
        "save": permission.save,
    });
    AppEvent {
        kind: "item/commandExecution/requestApproval".to_string(),
        thread_id: Some(permission.session_id.clone()),
        item_id: None,
        item_type: None,
        title: Some(command),
        text: permission.message.clone(),
        images: Vec::new(),
        request_id: Some(permission.id.clone()),
        raw: raw.to_string(),
    }
}

/// The user-input card event for a pending form. Unsupported fields become a
/// note asking the user to finish the form in OpenCode (or cancel it).
pub fn form_event(form: &Form) -> AppEvent {
    let mut questions = Vec::new();
    let mut unsupported = false;
    for field in &form.fields {
        if field["hidden"] == true {
            continue;
        }
        if !forms::field_supported(field) {
            unsupported = true;
            continue;
        }
        let key = str_of(field.get("key"));
        let header = field["title"].as_str().unwrap_or(key);
        let question = str_of(field.get("description"));
        let (options, is_other, multi): (Vec<Value>, bool, bool) = match field["type"].as_str() {
            Some("boolean") => {
                (vec![json!({"label": "是 / Yes"}), json!({"label": "否 / No"})], false, false)
            },
            Some("multiselect") => (options_of(field), field["custom"] == true, true),
            Some("string") if field["options"].is_array() => {
                (options_of(field), field["custom"] == true, false)
            },
            _ => (Vec::new(), true, false),
        };
        questions.push(json!({
            "id": key,
            "header": header,
            "question": question,
            "isOther": is_other,
            "isSecret": false,
            "options": options,
            "multiSelect": multi,
        }));
    }
    if unsupported || questions.is_empty() {
        questions.push(json!({
            "id": "__unsupported",
            "header": form.title,
            "question": "此表单包含暂不支持的字段，请在 OpenCode 中完成，或取消。 / This form has fields Pocket cannot collect; complete it in OpenCode or cancel.",
            "isOther": false,
            "options": [],
            "unsupported": true,
        }));
    }
    let raw = json!({"threadId": form.session_id, "title": form.title, "questions": questions});
    AppEvent {
        kind: "item/tool/requestUserInput".to_string(),
        thread_id: Some(form.session_id.clone()),
        item_id: None,
        item_type: None,
        title: Some(form.title.clone()),
        text: None,
        images: Vec::new(),
        request_id: Some(form.id.clone()),
        raw: raw.to_string(),
    }
}

fn options_of(field: &Value) -> Vec<Value> {
    field["options"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| json!({"label": o["label"].as_str().or_else(|| o["value"].as_str()), "description": o["description"]}))
        .collect()
}

/// Convert UI answers (question id → chosen labels / free text) into a typed
/// native form answer. Labels map back to option values.
pub fn form_answer(form: &Form, answers: &HashMap<String, Vec<String>>) -> Value {
    let mut out = Map::new();
    for field in &form.fields {
        let key = str_of(field.get("key"));
        let Some(chosen) = answers.get(key).filter(|a| !a.is_empty()) else {
            continue;
        };
        let value_of = |label: &str| {
            field["options"]
                .as_array()
                .and_then(|options| {
                    options
                        .iter()
                        .find(|o| o["label"] == label || o["value"] == label)
                })
                .and_then(|o| o["value"].as_str())
                .unwrap_or(label)
                .to_string()
        };
        let first = chosen[0].trim();
        let value = match field["type"].as_str() {
            Some("boolean") => Value::Bool(
                first.starts_with('是') || first.eq_ignore_ascii_case("yes") || first == "true",
            ),
            Some("number") => first
                .parse::<f64>()
                .map_or_else(|_| Value::String(first.to_string()), |n| json!(n)),
            Some("integer") => first
                .parse::<i64>()
                .map_or_else(|_| Value::String(first.to_string()), |n| json!(n)),
            Some("multiselect") => Value::Array(
                chosen
                    .iter()
                    .flat_map(|c| c.split('\n'))
                    .map(|c| Value::String(value_of(c.trim())))
                    .collect(),
            ),
            _ => Value::String(value_of(first)),
        };
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(value: Value) -> Message {
        serde_json::from_value(value).expect("message")
    }

    fn window() -> Vec<Message> {
        vec![
            msg(json!({"id": "msg_u1", "type": "user", "time": {"created": 1000}, "text": "hi",
                "files": [{"mime": "image/png", "data": "AAA", "source": {"type": "inline"}}]})),
            msg(
                json!({"id": "msg_a1", "type": "assistant", "time": {"created": 1100, "completed": 3000},
                "content": [
                    {"type": "reasoning", "text": "think"},
                    {"type": "text", "text": "answer"},
                    {"type": "tool", "id": "call_1", "name": "shell",
                     "state": {"status": "completed", "input": {"command": "ls"},
                               "content": [{"type": "text", "text": "a\nb"}], "metadata": {"exit": 0}}},
                    {"type": "tool", "id": "call_2", "name": "edit",
                     "state": {"status": "completed", "input": {"path": "x.rs", "oldString": "a", "newString": "b"},
                               "metadata": {"files": [{"file": "x.rs", "patch": "--- x.rs\n+++ x.rs\n@@ -1 +1 @@\n-a\n+b"}]}}},
                    {"type": "tool", "id": "call_3", "name": "subagent",
                     "state": {"status": "completed", "input": {"agent": "explore", "description": "look"},
                               "metadata": {"sessionID": "ses_child"}, "content": [{"type": "text", "text": "done"}]}},
                    {"type": "tool", "id": "call_4", "name": "grep",
                     "state": {"status": "error", "input": {"pattern": "foo"}, "error": {"message": "boom"}}}
                ]}),
            ),
            msg(
                json!({"id": "msg_idle", "type": "idle", "time": {"created": 3001}, "outcome": "succeeded"}),
            ),
            msg(
                json!({"id": "msg_u2", "type": "user", "time": {"created": 4000}, "text": "again"}),
            ),
            msg(json!({"id": "msg_a2", "type": "assistant", "time": {"created": 4100},
                "content": [{"type": "text", "text": "partial"}]})),
        ]
    }

    #[test]
    fn a_window_maps_to_turn_grouped_items() {
        let items = map_window(&window(), &[], Some("msg_u2"));
        let shape: Vec<(&str, &str, &str)> = items
            .iter()
            .map(|i| (i.id.as_str(), i.item_type.as_str(), i.turn_id.as_str()))
            .collect();
        assert_eq!(shape, vec![
            ("msg_u1", "userMessage", "msg_u1"),
            ("msg_a1:r0", "reasoning", "msg_u1"),
            ("msg_a1:t0", "agentMessage", "msg_u1"),
            ("msg_a1:call_1", "commandExecution", "msg_u1"),
            ("msg_a1:call_2", "fileChange", "msg_u1"),
            ("msg_a1:call_3", "collabAgentToolCall", "msg_u1"),
            ("msg_a1:call_4", "dynamicToolCall", "msg_u1"),
            ("msg_u2", "userMessage", "msg_u2"),
            ("msg_a2:t0", "agentMessage", "msg_u2"),
        ]);
        assert_eq!(items[0].images, vec!["data:image/png;base64,AAA".to_string()]);
        assert_eq!(items[3].title, "ls");
        assert_eq!(items[3].text, "a\nb\n[exit 0]");
        assert!(items[4].text.contains("+b"));
        assert!(items[5].text.contains("ses_child"));
        assert!(items[6].text.ends_with("[error] boom"));
        assert_eq!(items[1].turn_completed_at, Some(3));
        assert_eq!(items[1].turn_duration_ms, Some(2000));
        assert_eq!(items[8].turn_completed_at, None, "the running turn has no completion");
    }

    #[test]
    fn leading_items_use_the_known_turn_start() {
        let starts = vec![TurnStart {
            id: "msg_u0".into(),
            created_ms: 10,
            text: "earlier".into(),
        }];
        let tail = vec![msg(json!({"id": "msg_a0", "type": "assistant", "time": {"created": 20},
            "content": [{"type": "text", "text": "x"}]}))];
        assert_eq!(map_window(&tail, &starts, None)[0].turn_id, "msg_u0");
        let items = map_window(&window(), &[], None);
        let rail = summaries(&[TurnStart::from_message(&window()[0]).expect("user")], &items);
        assert_eq!(rail[0].assistant_text, "answer");
        assert!(rail[0].loaded);
    }

    #[test]
    fn forms_round_trip_through_the_question_card() {
        let form: Form = serde_json::from_value(json!({
            "id": "frm_1", "sessionID": "ses_1", "title": "Pick",
            "fields": [
                {"key": "ok", "type": "boolean", "title": "Proceed?"},
                {"key": "n", "type": "integer"},
                {"key": "tags", "type": "multiselect", "options": [{"value": "a", "label": "Alpha"}, {"value": "b", "label": "Beta"}]},
                {"key": "mail", "type": "string", "format": "email"}
            ]
        }))
        .expect("form");
        let event = form_event(&form);
        let raw: Value = serde_json::from_str(&event.raw).expect("raw");
        let questions = raw["questions"].as_array().expect("questions");
        assert_eq!(questions.len(), 4, "three supported fields plus the unsupported note");
        assert_eq!(questions[2]["multiSelect"], true);
        assert_eq!(questions[3]["unsupported"], true);
        let answers = HashMap::from([
            ("ok".to_string(), vec!["是 / Yes".to_string()]),
            ("n".to_string(), vec!["3".to_string()]),
            ("tags".to_string(), vec!["Alpha".to_string(), "Beta".to_string()]),
        ]);
        assert_eq!(form_answer(&form, &answers), json!({"ok": true, "n": 3, "tags": ["a", "b"]}));
    }

    #[test]
    fn permissions_and_models_map_to_existing_cards() {
        let permission: Permission = serde_json::from_value(json!({
            "id": "per_1", "sessionID": "ses_1", "action": "shell", "resources": ["rm -rf x"],
            "save": ["rm *"], "message": "why"
        }))
        .expect("permission");
        let event = permission_event(&permission, "/w");
        assert_eq!(event.request_id.as_deref(), Some("per_1"));
        let raw: Value = serde_json::from_str(&event.raw).expect("raw");
        assert_eq!(raw["command"], "shell rm -rf x");
        assert_eq!(raw["persistsProject"], true);

        let model = json!({"id": "m", "providerID": "p", "name": "M", "enabled": true, "variants": [{"id": "high"}]});
        let info = model_info(&model, Some("p/m")).expect("enabled");
        assert_eq!(info.id, "p/m");
        assert!(info.is_default);
        assert_eq!(info.supported_reasoning_efforts, vec!["high".to_string()]);
        assert!(
            model_info(&json!({"id": "m", "providerID": "p", "enabled": false}), None).is_none()
        );
    }
}

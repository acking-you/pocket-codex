//! Pure mapping from hub shapes to the app-server shapes the session UI
//! renders (TRD §4.5).

use std::collections::HashMap;

use pocket_codex_core::acp::{
    pcx::stop_reason as pcx_stop, ConfigOption, ContentBlock, HubItem, PermissionOption,
    SessionInfo, ToolCall, ToolCallContent, TurnInfo,
};
use serde_json::{json, Map, Value};

use crate::engine::app_session::{
    encode_plan, ModelInfo, ThreadItem, ThreadMeta, ThreadRuntimeConfig, TurnSummary,
};

/// Largest image carried into a ThreadItem.
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Text limit of synthesized diffs and tool JSON.
const MAX_TEXT: usize = 256 * 1024;

/// UI turn id of hub turn `turn`.
pub fn turn_id(turn: u32) -> String {
    format!("t{turn}")
}

/// Hub turn of a UI turn id.
pub fn parse_turn(turn_id: &str) -> Option<u32> {
    turn_id.strip_prefix('t')?.parse().ok()
}

fn cap(mut text: String) -> String {
    if text.len() > MAX_TEXT {
        let mut cut = MAX_TEXT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n…");
    }
    text
}

/// Text, links and images of message content.
fn message_parts(content: &[ContentBlock]) -> (String, Vec<String>) {
    let mut text = String::new();
    let mut images = Vec::new();
    let mut oversized = false;
    for block in content {
        match block {
            ContentBlock::Text(t) => text.push_str(&t.text),
            ContentBlock::ResourceLink(l) => {
                if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                    text.push(' ');
                }
                text.push_str(&format!("@{}", l.name));
            },
            ContentBlock::Resource(r) => {
                let uri = r.resource.get("uri").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                    text.push(' ');
                }
                text.push_str(&format!("@{uri}"));
            },
            ContentBlock::Image(i) => {
                if i.data.len() > MAX_IMAGE_BYTES {
                    oversized = true;
                } else {
                    images.push(format!("data:{};base64,{}", i.mime_type, i.data));
                }
            },
            ContentBlock::Audio(_) | ContentBlock::Unknown(_) => {},
        }
    }
    if oversized {
        text.push_str("[图片过大，未显示]");
    }
    (text, images)
}

fn tool_text(tool: &ToolCall) -> String {
    let mut out: Vec<String> = Vec::new();
    for content in &tool.content {
        match content {
            ToolCallContent::Content(c) => {
                if let Some(t) = c.content.as_text() {
                    out.push(t.to_string());
                }
            },
            ToolCallContent::Terminal(t) => out.push(format!("[terminal {}]", t.terminal_id)),
            _ => {},
        }
    }
    out.join("\n")
}

fn command_of(tool: &ToolCall) -> Option<String> {
    match tool.raw_input.as_ref()?.get("command")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

fn execute_text(tool: &ToolCall) -> String {
    let mut text = tool_text(tool);
    if let Some(output) = &tool.raw_output {
        for key in ["output", "stdout", "stderr"] {
            if let Some(s) = output
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(s);
            }
        }
    }
    if tool.status.as_deref() == Some("failed") {
        text.push_str("\n[error]");
    }
    if let Some(code) = tool
        .raw_output
        .as_ref()
        .and_then(|o| o.get("exitCode"))
        .and_then(Value::as_i64)
    {
        text.push_str(&format!("\n[exit {code}]"));
    }
    text
}

fn diffs(tool: &ToolCall) -> Vec<(String, Option<String>, String)> {
    tool.content
        .iter()
        .filter_map(|c| match c {
            ToolCallContent::Diff(d) => {
                Some((d.path.clone(), d.old_text.clone(), d.new_text.clone()))
            },
            _ => None,
        })
        .collect()
}

fn unified(path: &str, old: Option<&str>, new: &str) -> String {
    let old_lines: Vec<&str> = old.map(|o| o.lines().collect()).unwrap_or_default();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for line in old_lines {
        out.push_str(&format!("-{line}\n"));
    }
    for line in new_lines {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

/// `(item_type, title, text)` of a tool item.
fn tool_shape(tool: &ToolCall) -> (&'static str, String, String) {
    match tool.kind.as_deref() {
        Some("execute") => (
            "commandExecution",
            command_of(tool).unwrap_or_else(|| tool.title.clone()),
            execute_text(tool),
        ),
        Some("edit" | "delete" | "move") => {
            let diffs = diffs(tool);
            let title = match diffs.len() {
                0 => tool.title.clone(),
                1 => diffs[0].0.clone(),
                n => format!("{n} files"),
            };
            let text = diffs
                .iter()
                .map(|(p, o, n)| unified(p, o.as_deref(), n))
                .collect::<String>();
            ("fileChange", title, cap(text))
        },
        Some("fetch") => (
            "webSearch",
            tool.title.clone(),
            tool.raw_input
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        Some("think") => ("reasoning", tool.title.clone(), tool_text(tool)),
        _ => {
            let body = json!({
                "input": tool.raw_input,
                "content": tool_text(tool),
                "output": tool.raw_output,
                "locations": tool.locations.iter().map(|l| &l.path).collect::<Vec<_>>(),
            });
            let title = tool
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| tool.title.clone());
            ("dynamicToolCall", title, cap(body.to_string()))
        },
    }
}

/// Whether a tool item is still running.
pub fn tool_running(item: &HubItem) -> bool {
    item.tool
        .as_ref()
        .is_some_and(|t| matches!(t.status.as_deref(), None | Some("pending" | "in_progress")))
}

/// Map one hub item (§4.5.1).
pub fn thread_item(item: &HubItem, turns: &[TurnInfo]) -> ThreadItem {
    let info = turns.iter().find(|t| t.turn == item.turn);
    let completed = info.and_then(|t| t.completed_at_ms);
    let duration = info.and_then(|t| Some(t.completed_at_ms? - t.started_at_ms?));
    let mut out = ThreadItem {
        id: item.id.clone(),
        item_type: String::new(),
        title: String::new(),
        text: String::new(),
        questions_json: None,
        images: Vec::new(),
        turn_id: turn_id(item.turn),
        turn_completed_at: completed.map(|ms| ms / 1000),
        turn_duration_ms: duration,
    };
    match item.kind.as_str() {
        "user" | "agent" | "thought" => {
            let (text, images) = message_parts(&item.content);
            out.item_type = match item.kind.as_str() {
                "user" => "userMessage",
                "agent" => "agentMessage",
                _ => "reasoning",
            }
            .into();
            out.text = text;
            if item.kind != "thought" {
                out.images = images;
            }
        },
        "tool" => {
            if let Some(tool) = &item.tool {
                let (kind, title, text) = tool_shape(tool);
                out.item_type = kind.into();
                out.title = title;
                out.text = text;
            } else {
                out.item_type = "dynamicToolCall".into();
            }
        },
        "plan" => {
            out.item_type = "plan".into();
            let steps: Vec<Value> = item
                .plan
                .iter()
                .flatten()
                .map(|e| json!({"step": e.content, "status": e.status}))
                .collect();
            out.text = encode_plan(&json!({ "plan": steps }));
        },
        _ => {
            out.item_type = "dynamicToolCall".into();
            out.title = "Pocket-Codex".into();
            out.text = message_parts(&item.content).0;
        },
    }
    out
}

/// Map a window of hub items.
pub fn thread_items(items: &[HubItem], turns: &[TurnInfo]) -> Vec<ThreadItem> {
    items.iter().map(|i| thread_item(i, turns)).collect()
}

/// The turn rail.
pub fn summaries(turns: &[TurnInfo], items: &[HubItem]) -> Vec<TurnSummary> {
    turns
        .iter()
        .map(|t| TurnSummary {
            turn_id: turn_id(t.turn),
            user_text: t.user_preview.clone(),
            assistant_text: t.agent_preview.clone(),
            loaded: items.iter().any(|i| i.turn == t.turn),
        })
        .collect()
}

/// A listed session.
pub fn thread_meta(info: &SessionInfo) -> ThreadMeta {
    ThreadMeta {
        id: info.session_id.clone(),
        preview: info.title.clone().unwrap_or_default(),
        name: info.title.clone(),
        cwd: info.cwd.clone(),
        updated_at: info
            .updated_at
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map_or(0, |t| t.timestamp()),
    }
}

/// UI turn status and error of a stop reason.
pub fn turn_status(stop: &str) -> (&'static str, Option<String>) {
    match stop {
        "end_turn" => ("completed", None),
        "cancelled" => ("interrupted", None),
        pcx_stop::AGENT_EXITED => ("failed", Some("the agent process exited".into())),
        pcx_stop::ERROR => ("failed", Some("the agent reported an error".into())),
        other => ("failed", Some(other.replace('_', " "))),
    }
}

/// Role of a config option: `model` | `effort` | `mode` | `other`.
pub fn config_role(option: &ConfigOption) -> &'static str {
    match option.category.as_deref() {
        Some("model") => "model",
        Some("thought_level") => "effort",
        Some("mode") => "mode",
        Some(_) => "other",
        None => match option.id.as_str() {
            "model" => "model",
            "effort" | "reasoning_effort" | "thought_level" => "effort",
            "mode" => "mode",
            _ => "other",
        },
    }
}

/// The option with `role`.
pub fn option_with_role<'a>(options: &'a [ConfigOption], role: &str) -> Option<&'a ConfigOption> {
    let preferred = |o: &&ConfigOption| config_role(o) == role && o.kind == "select";
    options
        .iter()
        .find(preferred)
        .or_else(|| options.iter().find(|o| config_role(o) == role))
}

fn value_str(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// Models offered by the model option (§4.5.3).
pub fn model_list(options: &[ConfigOption]) -> Vec<ModelInfo> {
    let Some(model) = option_with_role(options, "model") else { return Vec::new() };
    let effort = option_with_role(options, "effort");
    let efforts: Vec<String> = effort
        .map(|e| e.flat_options().iter().map(|o| o.value.clone()).collect())
        .unwrap_or_default();
    let current = value_str(&model.current_value);
    model
        .flat_options()
        .into_iter()
        .map(|o| ModelInfo {
            id: o.value.clone(),
            display_name: o.name.clone(),
            description: o.description.clone().unwrap_or_default(),
            supported_reasoning_efforts: efforts.clone(),
            default_reasoning_effort: effort.and_then(|e| value_str(&e.current_value)),
            supported_service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: current.as_deref() == Some(o.value.as_str()),
        })
        .collect()
}

/// Effective model / effort / mode of a session.
pub fn runtime_config(options: &[ConfigOption]) -> ThreadRuntimeConfig {
    let current =
        |role: &str| option_with_role(options, role).and_then(|o| value_str(&o.current_value));
    let plan = option_with_role(options, "mode").and_then(|o| value_str(&o.current_value));
    ThreadRuntimeConfig {
        model: current("model"),
        reasoning_effort: current("effort"),
        collaboration_mode: plan
            .map(|m| if m == "plan" { "plan".into() } else { "default".into() }),
        confirmed_by_update: true,
        ..ThreadRuntimeConfig::default()
    }
}

/// Whether a mode option offers `plan`.
pub fn has_plan_mode(options: &[ConfigOption]) -> bool {
    options
        .iter()
        .filter(|o| config_role(o) == "mode")
        .any(|o| o.flat_options().iter().any(|v| v.value == "plan"))
}

/// The approval event of a permission request (§4.5.4).
pub fn permission_event(
    session: &str,
    request_id: &str,
    params: &Value,
    cwd: &str,
) -> crate::engine::app_session::AppEvent {
    let tool = &params["toolCall"];
    let kind = tool["kind"].as_str().unwrap_or("");
    let file_kind = matches!(kind, "edit" | "delete" | "move");
    let title = tool["title"].as_str().unwrap_or("").to_string();
    let mut command = title.clone();
    if let Some(cmd) = tool["rawInput"]["command"].as_str() {
        if !command.contains(cmd) {
            if !command.is_empty() {
                command.push(' ');
            }
            command.push_str(cmd);
        }
    }
    let text = tool["content"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|c| c["content"]["text"].as_str())
        .map(str::to_string);
    let mut changes: Vec<Value> = Vec::new();
    for location in tool["locations"].as_array().into_iter().flatten() {
        if let Some(path) = location["path"].as_str() {
            changes.push(json!({ "path": path }));
        }
    }
    for content in tool["content"].as_array().into_iter().flatten() {
        if content["type"] == "diff" {
            if let Some(path) = content["path"].as_str() {
                if !changes.iter().any(|c| c["path"] == path) {
                    changes.push(json!({ "path": path }));
                }
            }
        }
    }
    let options: Vec<Value> = params["options"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| json!({"optionId": o["optionId"], "name": o["name"], "kind": o["kind"]}))
        .collect();
    let raw = json!({
        "threadId": session,
        "command": command,
        "cwd": cwd,
        "reason": text,
        "changes": changes,
        "acpOptions": options,
    });
    crate::engine::app_session::AppEvent {
        kind: if file_kind {
            "item/fileChange/requestApproval"
        } else {
            "item/commandExecution/requestApproval"
        }
        .into(),
        thread_id: Some(session.to_string()),
        item_id: tool["toolCallId"].as_str().map(|id| format!("tc:{id}")),
        item_type: None,
        title: Some(title),
        text,
        images: Vec::new(),
        request_id: Some(request_id.to_string()),
        raw: raw.to_string(),
    }
}

/// The ACP answer of an approval decision (§4.5.4).
pub fn approval_answer(options: &[PermissionOption], decision: &str) -> Option<Value> {
    if decision == "cancel" {
        return Some(json!({"outcome": {"outcome": "cancelled"}}));
    }
    let find = |kinds: &[&str]| options.iter().find(|o| kinds.contains(&o.kind.as_str()));
    let chosen = match decision {
        "accept" => find(&["allow_once"]),
        "acceptForSession" => find(&["allow_always"]),
        "decline" => find(&["reject_once"]).or_else(|| find(&["reject_always"])),
        _ => None,
    }?;
    Some(json!({"outcome": {"outcome": "selected", "optionId": chosen.option_id}}))
}

fn schema_props(schema: &Value) -> Vec<(String, Value)> {
    let props: Vec<(String, Value)> = schema["properties"]
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let (mut first, rest): (Vec<_>, Vec<_>) = props
        .into_iter()
        .partition(|(k, _)| required.contains(&k.as_str()));
    first.extend(rest);
    first
}

fn enum_options(prop: &Value) -> Option<Vec<Value>> {
    if let Some(values) = prop["enum"].as_array() {
        let titles = prop["enumNames"].as_array();
        return Some(
            values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let label = titles
                        .and_then(|t| t.get(i))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| value_str(v))
                        .unwrap_or_default();
                    json!({"label": label})
                })
                .collect(),
        );
    }
    let one_of = prop["oneOf"]
        .as_array()
        .or_else(|| prop["anyOf"].as_array())?;
    Some(
        one_of
            .iter()
            .map(|o| {
                let label = o["title"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value_str(&o["const"]));
                json!({"label": label.unwrap_or_default()})
            })
            .collect(),
    )
}

/// The question card of a form elicitation (§4.5.5).
pub fn form_event(
    session: Option<&str>,
    request_id: &str,
    message: &str,
    schema: &Value,
) -> crate::engine::app_session::AppEvent {
    let mut questions = Vec::new();
    let mut unsupported = false;
    for (key, prop) in schema_props(schema) {
        let header = prop["title"].as_str().unwrap_or(&key).to_string();
        let question = prop["description"].as_str().unwrap_or("").to_string();
        let (options, is_other, multi) = match prop["type"].as_str() {
            Some("string") => match enum_options(&prop) {
                Some(options) => (options, false, false),
                None => (Vec::new(), true, false),
            },
            Some("number" | "integer") => (Vec::new(), true, false),
            Some("boolean") => {
                (vec![json!({"label": "是 / Yes"}), json!({"label": "否 / No"})], false, false)
            },
            Some("array") => match enum_options(&prop["items"]) {
                Some(options) => (options, false, true),
                None => {
                    unsupported = true;
                    continue;
                },
            },
            _ => {
                unsupported = true;
                continue;
            },
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
            "header": message,
            "question": "此表单包含暂不支持的字段，请在主机上完成，或取消。 / This form has fields Pocket cannot collect; complete it on the host or cancel.",
            "isOther": false,
            "options": [],
            "unsupported": true,
        }));
    }
    let raw = json!({"threadId": session, "title": message, "questions": questions});
    crate::engine::app_session::AppEvent {
        kind: "item/tool/requestUserInput".into(),
        thread_id: session.map(str::to_string),
        item_id: None,
        item_type: None,
        title: Some(message.to_string()),
        text: None,
        images: Vec::new(),
        request_id: Some(request_id.to_string()),
        raw: raw.to_string(),
    }
}

/// Map label for a schema option back to its value.
fn option_value(prop: &Value, label: &str) -> Value {
    if let Some(values) = prop["enum"].as_array() {
        let titles = prop["enumNames"].as_array();
        for (i, v) in values.iter().enumerate() {
            let shown = titles
                .and_then(|t| t.get(i))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| value_str(v));
            if shown.as_deref() == Some(label) {
                return v.clone();
            }
        }
    }
    if let Some(options) = prop["oneOf"]
        .as_array()
        .or_else(|| prop["anyOf"].as_array())
    {
        for o in options {
            let shown = o["title"]
                .as_str()
                .map(str::to_string)
                .or_else(|| value_str(&o["const"]));
            if shown.as_deref() == Some(label) {
                return o["const"].clone();
            }
        }
    }
    Value::String(label.to_string())
}

/// The ACP content of form answers; `Ok(None)` for an empty answer (cancel).
pub fn form_content(
    schema: &Value,
    answers: &HashMap<String, Vec<String>>,
) -> Result<Option<Value>, String> {
    if answers
        .values()
        .all(|a| a.iter().all(|s| s.trim().is_empty()))
    {
        return Ok(None);
    }
    let mut out = Map::new();
    for (key, prop) in schema_props(schema) {
        let Some(chosen) = answers.get(&key).filter(|a| !a.is_empty()) else { continue };
        let first = chosen[0].trim();
        let value = match prop["type"].as_str() {
            Some("number") => json!(first
                .parse::<f64>()
                .map_err(|_| format!("`{key}` must be a number"))?),
            Some("integer") => json!(first
                .parse::<i64>()
                .map_err(|_| format!("`{key}` must be an integer"))?),
            Some("boolean") => Value::Bool(
                first.starts_with('是') || first.eq_ignore_ascii_case("yes") || first == "true",
            ),
            Some("array") => Value::Array(
                chosen
                    .iter()
                    .flat_map(|c| c.split('\n'))
                    .filter(|c| !c.trim().is_empty())
                    .map(|c| option_value(&prop["items"], c.trim()))
                    .collect(),
            ),
            _ => option_value(&prop, first),
        };
        out.insert(key, value);
    }
    Ok(Some(Value::Object(out)))
}

#[cfg(test)]
mod tests {
    use pocket_codex_core::acp::Transcript;

    use super::*;

    fn fold(updates: Vec<Value>) -> Transcript {
        let mut t = Transcript::new("g");
        for u in updates {
            t.apply(&serde_json::from_value(u).expect("update"), None);
        }
        t
    }

    #[test]
    fn hub_item_mapping_table() {
        let big = "A".repeat(MAX_IMAGE_BYTES + 1);
        let t = fold(vec![
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "look at"}}),
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "resource_link", "uri": "file:///a.rs", "name": "a.rs"}}),
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "image", "data": "AAA", "mimeType": "image/png"}}),
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "image", "data": big, "mimeType": "image/png"}}),
            json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}}),
            json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "answer"}}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "x1", "title": "Run", "kind": "execute", "status": "failed",
                "rawInput": {"command": ["ls", "-la"]}, "rawOutput": {"stdout": "out", "exitCode": 2},
                "content": [{"type": "terminal", "terminalId": "term-1"}]}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "e1", "title": "Edit", "kind": "edit", "status": "completed",
                "content": [{"type": "diff", "path": "/w/a.rs", "oldText": "a", "newText": "b"}]}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "e2", "title": "Edit two", "kind": "move", "status": "completed",
                "content": [{"type": "diff", "path": "/w/a", "newText": "x"}, {"type": "diff", "path": "/w/b", "newText": "y"}]}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "f1", "title": "Fetch docs", "kind": "fetch", "rawInput": {"url": "https://x"}}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "k1", "title": "Thinking", "kind": "think",
                "content": [{"type": "content", "content": {"type": "text", "text": "deep"}}]}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "o1", "title": "Other", "name": "mcp_tool", "kind": "other"}),
            json!({"sessionUpdate": "plan", "entries": [{"content": "Step 1", "priority": "high", "status": "completed"}, {"content": "Step 2", "priority": "low", "status": "in_progress"}]}),
        ]);
        let mut transcript = t;
        transcript.push_notice("dropped", None);
        let items = thread_items(transcript.items(), transcript.turns());
        let shape: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.item_type.as_str(), i.title.as_str()))
            .collect();
        assert_eq!(shape, vec![
            ("userMessage", ""),
            ("reasoning", ""),
            ("agentMessage", ""),
            ("commandExecution", "ls -la"),
            ("fileChange", "/w/a.rs"),
            ("fileChange", "2 files"),
            ("webSearch", "Fetch docs"),
            ("reasoning", "Thinking"),
            ("dynamicToolCall", "mcp_tool"),
            ("plan", ""),
            ("dynamicToolCall", "Pocket-Codex"),
        ]);
        assert_eq!(items[0].text, "look at @a.rs[图片过大，未显示]");
        assert_eq!(items[0].images, vec!["data:image/png;base64,AAA".to_string()]);
        assert_eq!(items[3].text, "[terminal term-1]\nout\n[error]\n[exit 2]");
        assert_eq!(items[4].text, "--- a//w/a.rs\n+++ b//w/a.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n");
        assert!(items[5].text.contains("@@ -1,0 +1,1 @@\n+x"));
        assert_eq!(items[6].text, r#"{"url":"https://x"}"#);
        assert_eq!(items[7].text, "deep");
        assert!(items[8].text.contains("\"input\""));
        assert!(items[9].text.contains("- [x] Step 1") && items[9].text.contains("- [~] Step 2"));
        assert_eq!(items[10].text, "dropped");
        assert!(items.iter().all(|i| i.turn_id == "t1"));
        assert_eq!(items[0].turn_completed_at, None, "replayed turns carry no times");
    }

    #[test]
    fn stop_reason_mapping() {
        assert_eq!(turn_status("end_turn"), ("completed", None));
        assert_eq!(turn_status("cancelled"), ("interrupted", None));
        for stop in
            ["max_tokens", "max_turn_requests", "refusal", "_pcx_agent_exited", "_pcx_error"]
        {
            let (status, error) = turn_status(stop);
            assert_eq!(status, "failed", "{stop}");
            assert!(error.is_some());
        }
    }

    #[test]
    fn config_option_roles() {
        let option = |id: &str, category: Option<&str>| ConfigOption {
            id: id.into(),
            category: category.map(str::to_string),
            kind: "select".into(),
            ..ConfigOption::default()
        };
        assert_eq!(config_role(&option("x", Some("model"))), "model");
        assert_eq!(config_role(&option("x", Some("thought_level"))), "effort");
        assert_eq!(config_role(&option("x", Some("mode"))), "mode");
        assert_eq!(config_role(&option("x", Some("model_config"))), "other");
        assert_eq!(config_role(&option("model", None)), "model");
        assert_eq!(config_role(&option("reasoning_effort", None)), "effort");
        assert_eq!(config_role(&option("mode", None)), "mode");
        assert_eq!(config_role(&option("verbosity", None)), "other");
        let options: Vec<ConfigOption> = serde_json::from_value(json!([
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "m2",
             "options": [{"value": "m1", "name": "One"}, {"group": "g", "name": "G", "options": [{"value": "m2", "name": "Two"}]}]},
            {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "high",
             "options": [{"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default",
             "options": [{"value": "default", "name": "Default"}, {"value": "plan", "name": "Plan"}]}
        ]))
        .expect("options");
        let models = model_list(&options);
        assert_eq!(models.len(), 2);
        assert!(models[1].is_default && !models[0].is_default);
        assert_eq!(models[0].supported_reasoning_efforts, vec!["low".to_string(), "high".into()]);
        assert_eq!(models[0].default_reasoning_effort.as_deref(), Some("high"));
        assert!(models[0].supported_service_tiers.is_empty());
        let config = runtime_config(&options);
        assert_eq!(
            (config.model.as_deref(), config.reasoning_effort.as_deref()),
            (Some("m2"), Some("high"))
        );
        assert_eq!(config.collaboration_mode.as_deref(), Some("default"));
        assert!(has_plan_mode(&options));
    }

    #[test]
    fn approvals_and_forms_round_trip() {
        let options: Vec<PermissionOption> = serde_json::from_value(json!([
            {"optionId": "a1", "name": "Allow", "kind": "allow_once"},
            {"optionId": "a2", "name": "Always", "kind": "allow_always"},
            {"optionId": "r2", "name": "Never", "kind": "reject_always"}
        ]))
        .expect("options");
        assert_eq!(approval_answer(&options, "accept").expect("a")["outcome"]["optionId"], "a1");
        assert_eq!(
            approval_answer(&options, "acceptForSession").expect("s")["outcome"]["optionId"],
            "a2"
        );
        assert_eq!(approval_answer(&options, "decline").expect("d")["outcome"]["optionId"], "r2");
        assert_eq!(
            approval_answer(&options, "cancel").expect("c")["outcome"]["outcome"],
            "cancelled"
        );
        assert!(approval_answer(&options[..1], "decline").is_none());
        let schema = json!({"type": "object", "required": ["n"], "properties": {
            "color": {"type": "string", "enum": ["r", "g"], "enumNames": ["Red", "Green"]},
            "n": {"type": "integer", "title": "Count"},
            "ok": {"type": "boolean"},
            "tags": {"type": "array", "items": {"type": "string", "enum": ["a", "b"]}},
            "free": {"type": "string"}
        }});
        let event = form_event(Some("s"), "r1", "Pick", &schema);
        let raw: Value = serde_json::from_str(&event.raw).expect("raw");
        let ids: Vec<&str> = raw["questions"]
            .as_array()
            .expect("q")
            .iter()
            .filter_map(|q| q["id"].as_str())
            .collect();
        assert_eq!(ids[0], "n", "required fields first");
        assert!(!ids.contains(&"__unsupported"));
        let answers = HashMap::from([
            ("color".to_string(), vec!["Green".to_string()]),
            ("n".to_string(), vec!["3".to_string()]),
            ("ok".to_string(), vec!["是 / Yes".to_string()]),
            ("tags".to_string(), vec!["a".to_string(), "b".to_string()]),
            ("free".to_string(), vec!["hello".to_string()]),
        ]);
        let content = form_content(&schema, &answers)
            .expect("ok")
            .expect("content");
        assert_eq!(
            content,
            json!({"color": "g", "n": 3, "ok": true, "tags": ["a", "b"], "free": "hello"})
        );
        let bad = HashMap::from([("n".to_string(), vec!["many".to_string()])]);
        assert!(form_content(&schema, &bad).is_err());
        assert_eq!(form_content(&schema, &HashMap::new()).expect("empty"), None);
        let odd = json!({"properties": {"o": {"type": "object"}}});
        let raw: Value =
            serde_json::from_str(&form_event(None, "r2", "Odd", &odd).raw).expect("raw");
        assert_eq!(raw["questions"][0]["id"], "__unsupported");
    }
}

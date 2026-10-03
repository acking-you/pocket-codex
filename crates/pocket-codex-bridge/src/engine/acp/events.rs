//! Hub messages → [`AppEvent`]s (TRD §4.5.2), applied to the connection's
//! [`Shared`] state on the event loop.

use pocket_codex_codex::client::Inbound;
use pocket_codex_core::acp::{
    pcx::{notifications, HubMeta},
    ContentBlock, HubItem, PermissionOption,
};
use serde_json::{json, Value};

use super::{
    mapping,
    state::{PendingKind, PendingReq, SessionView, Shared, MAX_BUFFERED},
};
use crate::engine::{
    app_events::{bare_item, event, hub_event, item_event},
    app_session::AppEvent,
};

/// What handling one inbound message produced.
#[derive(Default)]
pub struct Outcome {
    /// Events to broadcast.
    pub events: Vec<AppEvent>,
    /// Sessions to re-attach (missed notifications).
    pub reattach: Vec<String>,
}

/// `acp/capabilities` (the UI re-reads `app_capabilities`).
pub fn capabilities_event() -> AppEvent {
    hub_event("acp/capabilities", json!({}))
}

/// `acp/hub/state` with the hub metadata.
pub fn hub_state_event(meta: &HubMeta) -> AppEvent {
    hub_event("acp/hub/state", serde_json::to_value(meta).unwrap_or(Value::Null))
}

/// `serverRequest/resolved` for a hub request id.
pub fn resolved(session: Option<&str>, request_id: &str) -> AppEvent {
    match session {
        Some(s) => {
            event("serverRequest/resolved", s, json!({"threadId": s, "requestId": request_id}))
        },
        None => hub_event("serverRequest/resolved", json!({"requestId": request_id})),
    }
}

fn seq_of(method: &str, params: &Value) -> Option<u64> {
    if method == "session/update" {
        params["_meta"]["pcx"]["seq"].as_u64()
    } else {
        params["seq"].as_u64()
    }
}

/// Handle one inbound message.
pub fn on_inbound(shared: &mut Shared, inbound: &Inbound) -> Outcome {
    let params = inbound.params.clone().unwrap_or(Value::Null);
    if let Some(token) = &inbound.request_id {
        return Outcome {
            events: on_request(shared, token, &inbound.method, &params),
            reattach: Vec::new(),
        };
    }
    let method = inbound.method.as_str();
    let mut out = Outcome::default();
    match method {
        "$/cancel_request" => {
            let token = match &params["requestId"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let hit = shared
                .pending
                .iter()
                .find(|(_, p)| p.token == token)
                .map(|(id, p)| (id.clone(), p.session.clone()));
            if let Some((id, session)) = hit {
                shared.pending.remove(&id);
                out.events.push(resolved(session.as_deref(), &id));
            }
        },
        notifications::HUB_STATE => {
            if let Ok(meta) = serde_json::from_value::<HubMeta>(params.clone()) {
                let caps_changed = shared.meta.as_ref().map(|m| &m.caps) != Some(&meta.caps);
                out.events.push(hub_state_event(&meta));
                shared.meta = Some(meta);
                if caps_changed {
                    out.events.push(capabilities_event());
                }
            }
        },
        notifications::SESSIONS_CHANGED => out
            .events
            .push(hub_event("acp/sessions/changed", json!({}))),
        notifications::REQUEST_RESOLVED if params.get("sessionId").is_none() => {
            if let Some(id) = params["requestId"].as_str() {
                shared.pending.remove(id);
                out.events.push(resolved(None, id));
            }
        },
        _ => {
            let Some(session) = params["sessionId"].as_str().map(str::to_string) else {
                return out;
            };
            let Some(view) = shared.sessions.get_mut(&session) else { return out };
            let Some(seq) = seq_of(method, &params) else {
                out.events.extend(apply(shared, &session, method, &params));
                return out;
            };
            if view.syncing {
                if view.buffered.len() >= MAX_BUFFERED {
                    view.overflowed = true;
                    view.buffered.clear();
                } else if !view.overflowed {
                    view.buffered.push((seq, method.to_string(), params));
                }
                return out;
            }
            if seq <= view.seq {
                return out;
            }
            if seq > view.seq + 1 {
                out.reattach.push(session.clone());
            }
            view.seq = seq;
            out.events.extend(apply(shared, &session, method, &params));
        },
    }
    out
}

fn on_request(shared: &mut Shared, token: &str, method: &str, params: &Value) -> Vec<AppEvent> {
    let Some(hub_id) = params["_meta"]["pcx"]["requestId"]
        .as_str()
        .map(str::to_string)
    else {
        return Vec::new();
    };
    let conn_gen = shared.conn_gen;
    if let Some(existing) = shared.pending.get_mut(&hub_id) {
        // Re-sent on a new connection: answer with the new token.
        existing.token = token.to_string();
        existing.conn_gen = conn_gen;
        return Vec::new();
    }
    let session = params["sessionId"].as_str().map(str::to_string);
    match method {
        "session/request_permission" => {
            let Some(session) = session else { return Vec::new() };
            let options: Vec<PermissionOption> =
                serde_json::from_value(params["options"].clone()).unwrap_or_default();
            let cwd = shared
                .sessions
                .get(&session)
                .map(|v| v.cwd.clone())
                .unwrap_or_default();
            shared.pending.insert(hub_id.clone(), PendingReq {
                token: token.to_string(),
                session: Some(session.clone()),
                kind: PendingKind::Permission(options),
                conn_gen,
            });
            vec![mapping::permission_event(&session, &hub_id, params, &cwd)]
        },
        "elicitation/create" => {
            let message = params["message"].as_str().unwrap_or("").to_string();
            if params["mode"] == "url" {
                shared.pending.insert(hub_id.clone(), PendingReq {
                    token: token.to_string(),
                    session: session.clone(),
                    kind: PendingKind::Url,
                    conn_gen,
                });
                let url = params["url"].as_str().unwrap_or("").to_string();
                let host = reqwest::Url::parse(&url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
                    .unwrap_or_default();
                let raw =
                    json!({"threadId": session, "message": message, "url": url, "host": host});
                let mut event = match &session {
                    Some(s) => event("acp/elicitation/url", s, raw),
                    None => hub_event("acp/elicitation/url", raw),
                };
                event.request_id = Some(hub_id);
                event.title = Some(message);
                vec![event]
            } else {
                let schema = params["requestedSchema"].clone();
                shared.pending.insert(hub_id.clone(), PendingReq {
                    token: token.to_string(),
                    session: session.clone(),
                    kind: PendingKind::Form(schema.clone()),
                    conn_gen,
                });
                vec![mapping::form_event(session.as_deref(), &hub_id, &message, &schema)]
            }
        },
        _ => Vec::new(),
    }
}

/// Apply one session notification (already ordered) to its view.
pub fn apply(shared: &mut Shared, session: &str, method: &str, params: &Value) -> Vec<AppEvent> {
    let mut events = Vec::new();
    if method == notifications::REQUEST_RESOLVED {
        if let Some(id) = params["requestId"].as_str() {
            shared.pending.remove(id);
            events.push(resolved(Some(session), id));
        }
        return events;
    }
    if method == notifications::SESSION_LOADED || method == notifications::SESSION_LOAD_FAILED {
        let result = if method == notifications::SESSION_LOADED {
            Ok(())
        } else {
            Err(params["error"]
                .as_str()
                .unwrap_or("the session could not be loaded")
                .to_string())
        };
        if let Some(view) = shared.sessions.get_mut(session) {
            view.loading = false;
            view.load_failed = result.clone().err();
        }
        for waiter in shared.loads.remove(session).unwrap_or_default() {
            let _ = waiter.send(result.clone());
        }
        return events;
    }
    let Some(view) = shared.sessions.get_mut(session) else { return events };
    match method {
        "session/update" => events.extend(update(view, session, params)),
        notifications::TURN_STARTED => {
            let turn = params["turn"].as_u64().unwrap_or(0) as u32;
            view.running = true;
            view.active_turn = Some(turn);
            view.completed.clear();
            let info = view.turn_mut(turn);
            info.started_at_ms = params["startedAtMs"].as_i64();
            info.user_item_id = params["userItemId"].as_str().map(str::to_string);
            let id = mapping::turn_id(turn);
            events.push(event(
                "turn/started",
                session,
                json!({"threadId": session, "turnId": id, "turn": {"id": id, "status": "inProgress"}}),
            ));
        },
        notifications::TURN_COMPLETED => {
            let turn = params["turn"].as_u64().unwrap_or(0) as u32;
            let stop = params["stopReason"]
                .as_str()
                .unwrap_or("end_turn")
                .to_string();
            let info = view.turn_mut(turn);
            info.completed_at_ms = params["completedAtMs"].as_i64();
            info.stop_reason = Some(stop.clone());
            let turns = view.turns.clone();
            let pending: Vec<HubItem> = view
                .items
                .iter()
                .filter(|i| i.turn == turn && matches!(i.kind.as_str(), "agent" | "thought"))
                .filter(|i| !view.completed.contains(&i.id))
                .cloned()
                .collect();
            for item in pending {
                let mapped = mapping::thread_item(&item, &turns);
                view.completed.insert(item.id.clone());
                events.push(item_event(
                    "item/completed",
                    session,
                    &mapped,
                    Some(mapped.text.clone()),
                ));
            }
            view.running = false;
            view.active_turn = None;
            events.push(turn_completed(session, turn, &stop, params["error"].as_str()));
        },
        notifications::SESSION_STATE => {
            let changed = params["updatedAt"].as_str() != view.updated_at.as_deref();
            if let Some(t) = params["title"].as_str() {
                view.title = Some(t.to_string());
            }
            if let Some(u) = params["updatedAt"].as_str() {
                view.updated_at = Some(u.to_string());
            }
            if changed && params["running"] != true && !view.running {
                events.push(event("acp/session/changed", session, json!({"threadId": session})));
            }
        },
        notifications::SESSION_GENERATION => {
            if let Some(g) = params["generation"].as_str() {
                view.generation = g.to_string();
            }
            view.clear_items();
            events.push(event("acp/session/generation", session, json!({"threadId": session})));
        },
        notifications::QUEUE_FAILED => {
            events.push(event(
                "acp/queue/failed",
                session,
                json!({"threadId": session, "reason": params["reason"], "prompts": params["prompts"]}),
            ));
        },
        _ => {},
    }
    events
}

/// `turn/completed` for a hub stop reason.
pub fn turn_completed(session: &str, turn: u32, stop: &str, error: Option<&str>) -> AppEvent {
    let (status, fallback) = mapping::turn_status(stop);
    let id = mapping::turn_id(turn);
    let mut turn_raw = json!({"id": id, "status": status});
    if status == "failed" {
        let message = error.map(str::to_string).or(fallback).unwrap_or_default();
        turn_raw["error"] = json!({ "message": message });
    }
    event("turn/completed", session, json!({"threadId": session, "turn": turn_raw}))
}

fn update(view: &mut SessionView, session: &str, params: &Value) -> Vec<AppEvent> {
    let pcx = &params["_meta"]["pcx"];
    let update = &params["update"];
    let mut events = Vec::new();
    if let Some(generation) = pcx["generation"].as_str() {
        if !view.generation.is_empty() && generation != view.generation {
            view.generation = generation.to_string();
            view.clear_items();
            events.push(event("acp/session/generation", session, json!({"threadId": session})));
            return events;
        }
        if view.generation.is_empty() {
            view.generation = generation.to_string();
        }
    }
    let turn = pcx["turn"].as_u64().unwrap_or(0) as u32;
    match update["sessionUpdate"].as_str().unwrap_or("") {
        tag
        @ ("tool_call" | "tool_call_update" | "plan" | "_pcx_notice" | "user_message_chunk") => {
            let Some(item) = serde_json::from_value::<HubItem>(pcx["item"].clone()).ok() else {
                return events;
            };
            view.upsert(item.clone());
            if tag == "user_message_chunk" {
                // The UI shows its own prompt optimistically and ignores echoes.
                return events;
            }
            let mapped = mapping::thread_item(&item, &view.turns);
            let done = tag == "plan" || tag == "_pcx_notice" || !mapping::tool_running(&item);
            let kind = if done { "item/completed" } else { "item/started" };
            if done {
                view.completed.insert(item.id.clone());
            }
            events.push(item_event(kind, session, &mapped, Some(mapped.text.clone())));
        },
        tag @ ("agent_message_chunk" | "agent_thought_chunk") => {
            let Some(id) = pcx["itemId"].as_str().map(str::to_string) else { return events };
            let Ok(block) = serde_json::from_value::<ContentBlock>(update["content"].clone())
            else {
                return events;
            };
            let (kind, item_type, delta_kind) = if tag == "agent_message_chunk" {
                ("agent", "agentMessage", "item/agentMessage/delta")
            } else {
                ("thought", "reasoning", "item/reasoning/textDelta")
            };
            let known = view.item(&id).is_some();
            view.append_chunk(&id, kind, turn, &block);
            let mut bare = bare_item(id.clone(), item_type);
            bare.turn_id = mapping::turn_id(turn);
            if !known || pcx["created"] == true {
                events.push(item_event("item/started", session, &bare, Some(String::new())));
            }
            if let Some(text) = block.as_text().filter(|t| !t.is_empty()) {
                events.push(item_event(delta_kind, session, &bare, Some(text.to_string())));
            }
        },
        "usage_update" => {
            if let Ok(usage) = serde_json::from_value(update.clone()) {
                view.usage = Some(usage);
            }
            events.push(event(
                "thread/tokenUsage/updated",
                session,
                json!({"threadId": session, "tokenUsage": {"last": {"totalTokens": update["used"]}, "modelContextWindow": update["size"]}}),
            ));
        },
        "session_info_update" => {
            if let Some(title) = update["title"].as_str() {
                view.title = Some(title.to_string());
                events.push(event(
                    "thread/name/updated",
                    session,
                    json!({"threadId": session, "name": title}),
                ));
            }
            if let Some(at) = update["updatedAt"].as_str() {
                view.updated_at = Some(at.to_string());
            }
        },
        "config_option_update" => {
            if let Ok(options) = serde_json::from_value(update["configOptions"].clone()) {
                view.config_options = options;
            }
            events.push(event("acp/config/updated", session, json!({"threadId": session})));
        },
        "current_mode_update" => {
            if let (Some(modes), Some(mode)) =
                (view.modes.as_mut(), update["currentModeId"].as_str())
            {
                modes.current_mode_id = mode.to_string();
            }
            events.push(event("acp/config/updated", session, json!({"threadId": session})));
        },
        "available_commands_update" => {
            if let Ok(commands) = serde_json::from_value(update["availableCommands"].clone()) {
                view.commands = commands;
            }
            events.push(event("acp/config/updated", session, json!({"threadId": session})));
        },
        _ => {},
    }
    events
}

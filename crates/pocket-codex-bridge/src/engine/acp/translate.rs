//! Gateway events → the item and event shapes the shared session UI renders.
//!
//! Only what ACP actually carries is produced: no Codex approval policy,
//! sandbox, Guardian review, turn timing or model catalog is synthesized.
//! Agent permission requests become `acp/permission/requested` (answered
//! with the agent's own option ids), not Codex approval kinds.
//!
//! Recovery (a reconnect or a `reset`) never patches the UI with deltas it
//! cannot vouch for: a rebuilt fold is announced with the full text of each
//! item (which replaces what the UI shows), and a turn that ended while
//! this controller was away is completed from the host's final fold. Turns
//! that ended are completed *before* their successors are announced, and
//! every item event names its turn, so a late completion can never end or
//! regroup a newer turn.
//!
//! Content the host could not keep (an oversized image, diff or input) or
//! that this controller could not read (`unavailable`) appears as a
//! `contentOmitted` item next to where it belongs, never as silence. A
//! notice is state of the item it belongs to: when a later update of that
//! tool call or plan replaces the content with one that fits, the notice is
//! sent again empty, which removes it.
//!
//! # Identifiers
//!
//! Folded items carry the host's ids, which start with a host turn id
//! (`<uuid>:…`, `replay:<n>:…`) or with `tool:` followed by the agent's own
//! tool call id — any string. Items synthesized here (notices, gaps) use ids
//! starting with `#` ([`synthetic_id`]), which no folded id does, so an
//! agent can never name a tool call that collides with one.

use std::collections::{HashMap, HashSet};

use pocket_codex_host_svc::acp::fold::{Change, Item, ItemKind, Turn};
use serde_json::{json, Value};

use super::Shared;
use crate::engine::app_session::{encode_plan, format_content_diff, AppEvent, ThreadItem};

/// Event kind of an agent permission request.
pub const PERMISSION_REQUESTED: &str = "acp/permission/requested";
/// Event kind of a session settings / state change.
pub const SESSION_UPDATED: &str = "acp/session/updated";
/// Event kind of a host phase / generation / reachability change.
pub const HOST_STATE: &str = "acp/host/state";
/// Most live folds kept per connection (running turns are always kept).
const MAX_LIVE: usize = 16;

pub(super) fn event(kind: &str, thread: Option<&str>, raw: Value) -> AppEvent {
    AppEvent {
        kind: kind.to_string(),
        thread_id: thread.map(str::to_string),
        item_id: None,
        item_type: None,
        title: None,
        text: None,
        images: Vec::new(),
        request_id: None,
        raw: raw.to_string(),
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

fn diff_text(path: &str, old: Option<&str>, new: &str) -> String {
    match old {
        None => format_content_diff(path, new, true),
        Some(old) => {
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
        },
    }
}

/// The shared-UI item for one folded ACP item.
pub fn app_item(item: &Item, turn: &str) -> ThreadItem {
    let (item_type, title, text) = match item.kind {
        ItemKind::UserMessage => ("userMessage", String::new(), item.text.clone()),
        ItemKind::AgentMessage => ("agentMessage", String::new(), item.text.clone()),
        ItemKind::Thought => ("reasoning", String::new(), item.text.clone()),
        ItemKind::Plan => {
            let plan: Vec<Value> = item
                .plan
                .iter()
                .flatten()
                .map(|entry| json!({"step": entry.content, "status": entry.status}))
                .collect();
            ("plan", String::new(), encode_plan(&json!({"plan": plan})))
        },
        ItemKind::ToolCall => tool_parts(item),
    };
    ThreadItem {
        id: item.id.clone(),
        item_type: item_type.to_string(),
        title,
        text,
        questions_json: None,
        images: item.images.clone(),
        turn_id: turn.to_string(),
        // ACP carries no turn timing; nothing is invented.
        turn_completed_at: None,
        turn_duration_ms: None,
    }
}

fn tool_parts(item: &Item) -> (&'static str, String, String) {
    let tool = item.tool.clone().unwrap_or_default();
    let mut text = item.text.clone();
    if tool.terminals > 0 {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("[terminal output is not available in this client]");
    }
    if tool.status == "failed" {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("[failed]");
    }
    match tool.kind.as_str() {
        "execute" => ("commandExecution", item.title.clone(), text),
        "edit" | "delete" | "move" if !tool.diffs.is_empty() => {
            let title = match tool.diffs.as_slice() {
                [one] => one.path.clone(),
                many => format!("{} files", many.len()),
            };
            let body = tool
                .diffs
                .iter()
                .map(|d| diff_text(&d.path, d.old_text.as_deref(), &d.new_text))
                .collect::<Vec<_>>()
                .join("\n");
            ("fileChange", title, body)
        },
        "search" | "fetch" => ("webSearch", item.title.clone(), text),
        _ => {
            let name = tool.name.clone().unwrap_or_else(|| tool.kind.clone());
            let detail = json!({"tool": name, "input": tool.raw_input, "output": text});
            ("dynamicToolCall", item.title.clone(), detail.to_string())
        },
    }
}

/// The id of a synthesized item of `kind` for `owner` (an item, turn or
/// thread id). Disjoint from every folded item id (see the module docs), and
/// one-to-one: different owners or kinds never share an id.
pub fn synthetic_id(kind: &str, owner: &str) -> String {
    format!("#{kind}:{owner}")
}

/// The marker shown where a turn's earlier items were evicted.
pub fn gap_item(id: String, turn: &str) -> ThreadItem {
    ThreadItem {
        id,
        item_type: "historyGap".into(),
        title: String::new(),
        text: "Earlier history is not available from this agent host.".into(),
        questions_json: None,
        images: Vec::new(),
        turn_id: turn.to_string(),
        turn_completed_at: None,
        turn_duration_ms: None,
    }
}

/// Item type of a notice for content that is not shown (see the module
/// docs); its text lists the reasons, comma separated.
pub const CONTENT_OMITTED: &str = "contentOmitted";
/// Reason of a notice for a turn this controller could not read.
const UNAVAILABLE: &str = "unavailable";

fn omission_item(id: String, turn: &str, reasons: String) -> ThreadItem {
    ThreadItem {
        id,
        item_type: CONTENT_OMITTED.into(),
        title: String::new(),
        text: reasons,
        questions_json: None,
        images: Vec::new(),
        turn_id: turn.to_string(),
        turn_completed_at: None,
        turn_duration_ms: None,
    }
}

/// The notice for what `item` dropped (images, diffs, input, …), if any.
pub fn omission_notice(item: &Item, turn: &str) -> Option<ThreadItem> {
    (!item.omitted.is_empty())
        .then(|| omission_item(synthetic_id("omitted", &item.id), turn, item.omitted.join(",")))
}

/// The notice for a turn whose items could not be read from the host.
pub fn unavailable_item(turn: &str) -> ThreadItem {
    omission_item(synthetic_id("unavailable", turn), turn, UNAVAILABLE.into())
}

/// Whether later updates replace `item`'s content (so an omission can
/// clear).
fn replaceable(item: &Item) -> bool {
    matches!(item.kind, ItemKind::ToolCall | ItemKind::Plan)
}

/// `item`'s notice as owner state (see the module docs): its reasons, or
/// an empty notice that removes an earlier one; `None` for an item whose
/// omissions never clear and which has none.
fn notice_state(thread: &str, item: &Item, turn: &str) -> Option<AppEvent> {
    let notice = match omission_notice(item, turn) {
        Some(notice) => notice,
        None if replaceable(item) => {
            omission_item(synthetic_id("omitted", &item.id), turn, String::new())
        },
        None => return None,
    };
    Some(item_event("item/completed", thread, &notice, Some(notice.text.clone())))
}

/// Removals of notices that `turn`'s items no longer have (after a
/// recovery, an earlier notice may be stale).
fn cleared_notices(thread: &str, turn: &Turn) -> Vec<AppEvent> {
    turn.items
        .iter()
        .filter(|item| item.omitted.is_empty())
        .filter_map(|item| notice_state(thread, item, &turn.id))
        .collect()
}

/// The UI items of a folded turn: a gap where items were evicted, the
/// retained items, and a notice after each item that dropped content.
pub fn turn_items(turn: &Turn) -> Vec<ThreadItem> {
    let mut items = Vec::new();
    let mut gap = turn.omitted > 0;
    for item in &turn.items {
        if gap && item.kind != ItemKind::UserMessage {
            items.push(gap_item(synthetic_id("gap", &turn.id), &turn.id));
            gap = false;
        }
        items.push(app_item(item, &turn.id));
        items.extend(omission_notice(item, &turn.id));
    }
    if gap {
        items.push(gap_item(synthetic_id("gap", &turn.id), &turn.id));
    }
    items
}

fn item_event(kind: &str, thread: &str, item: &ThreadItem, text: Option<String>) -> AppEvent {
    let raw = json!({
        "threadId": thread, "turnId": item.turn_id, "itemId": item.id,
        "item": {"id": item.id, "type": item.item_type},
    });
    AppEvent {
        kind: kind.to_string(),
        thread_id: Some(thread.to_string()),
        item_id: Some(item.id.clone()),
        item_type: Some(item.item_type.clone()),
        title: Some(item.title.clone()),
        text,
        images: item.images.clone(),
        request_id: None,
        raw: raw.to_string(),
    }
}

fn tool_done(item: &Item) -> bool {
    item.tool
        .as_ref()
        .is_some_and(|tool| matches!(tool.status.as_str(), "completed" | "failed"))
}

fn is_message(item: &Item) -> bool {
    matches!(item.kind, ItemKind::AgentMessage | ItemKind::Thought)
}

/// Item events for folded changes.
pub fn changes(thread: &str, turn: &str, changes: Vec<Change>) -> Vec<AppEvent> {
    let mut out = Vec::new();
    for change in changes {
        match change {
            Change::Started(item) if is_message(&item) => {
                let mapped = app_item(&item, turn);
                out.push(item_event("item/started", thread, &mapped, Some(String::new())));
                if !item.text.is_empty() {
                    out.push(delta_event(thread, &mapped, item.text.clone()));
                }
                out.extend(notice_event(thread, &item, turn));
            },
            Change::Delta {
                item_id,
                kind,
                delta,
            } => {
                let item_type =
                    if kind == ItemKind::Thought { "reasoning" } else { "agentMessage" };
                let mapped = ThreadItem {
                    id: item_id,
                    item_type: item_type.to_string(),
                    title: String::new(),
                    text: String::new(),
                    questions_json: None,
                    images: Vec::new(),
                    turn_id: turn.to_string(),
                    turn_completed_at: None,
                    turn_duration_ms: None,
                };
                out.push(delta_event(thread, &mapped, delta));
            },
            change @ (Change::Started(_) | Change::Updated(_)) => {
                let (item, updated) = match change {
                    Change::Updated(item) => (item, true),
                    Change::Started(item) => (item, false),
                    Change::Delta {
                        ..
                    } => continue,
                };
                let mapped = app_item(&item, turn);
                let done = item.kind == ItemKind::Plan
                    || item.kind == ItemKind::UserMessage
                    || tool_done(&item)
                    || item.kind == ItemKind::AgentMessage;
                let kind = if done { "item/completed" } else { "item/started" };
                out.push(item_event(kind, thread, &mapped, Some(mapped.text.clone())));
                // An update replaces the item's content, so its notice is
                // re-stated (and removed when nothing is omitted any more).
                if updated {
                    out.extend(notice_state(thread, &item, turn));
                } else {
                    out.extend(notice_event(thread, &item, turn));
                }
            },
        }
    }
    out
}

/// The notice event for what `item` dropped, if anything.
fn notice_event(thread: &str, item: &Item, turn: &str) -> Option<AppEvent> {
    let notice = omission_notice(item, turn)?;
    Some(item_event("item/completed", thread, &notice, Some(notice.text.clone())))
}

fn delta_event(thread: &str, item: &ThreadItem, delta: String) -> AppEvent {
    let method = if item.item_type == "reasoning" {
        "item/reasoning/textDelta"
    } else {
        "item/agentMessage/delta"
    };
    item_event(method, thread, item, Some(delta))
}

/// Full-text events for a fold rebuilt from history while its turn runs:
/// each replaces what the UI shows for that item.
pub fn recovered(thread: &str, turn: &Turn) -> Vec<AppEvent> {
    let mut out: Vec<AppEvent> = turn_items(turn)
        .into_iter()
        .filter(|mapped| mapped.item_type != "userMessage")
        .map(|mapped| {
            let done = mapped.item_type == "historyGap"
                || mapped.item_type == CONTENT_OMITTED
                || turn
                    .items
                    .iter()
                    .find(|item| item.id == mapped.id)
                    .is_some_and(|item| item.kind == ItemKind::Plan || tool_done(item));
            let kind = if done { "item/completed" } else { "item/started" };
            item_event(kind, thread, &mapped, Some(mapped.text.clone()))
        })
        .collect();
    out.extend(cleared_notices(thread, turn));
    out
}

/// Completion events for a turn that ended: every retained item with its
/// final full text, then `turn/completed` with the recorded outcome. A turn
/// whose items could not be read says so instead of ending silently empty.
fn completion(
    thread: &str,
    turn_id: &str,
    turn: Option<&Turn>,
    outcome: Option<&Value>,
) -> Vec<AppEvent> {
    let items = match turn {
        Some(turn) => turn_items(turn),
        None => vec![unavailable_item(turn_id)],
    };
    let mut out: Vec<AppEvent> = items
        .into_iter()
        .filter(|mapped| mapped.item_type != "userMessage")
        .map(|mapped| item_event("item/completed", thread, &mapped, Some(mapped.text.clone())))
        .collect();
    out.push(turn_completed(thread, turn_id, outcome));
    out
}

/// Completion of a turn that ended while this controller was away, from
/// the host's history answer (when it could still be read).
pub fn finished(
    thread: &str,
    turn_id: &str,
    history: Option<&Value>,
    outcome: Option<&Value>,
) -> Vec<AppEvent> {
    let turn = history
        .and_then(|history| history["window"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|turn| serde_json::from_value::<Turn>(turn.clone()).ok())
        .find(|turn| turn.id == turn_id);
    let mut out = completion(thread, turn_id, turn.as_ref(), outcome);
    // Updates missed while away may have cleared notices shown before.
    if let Some(turn) = &turn {
        let end = out.pop();
        out.extend(cleared_notices(thread, turn));
        out.extend(end);
    }
    out
}

/// Install `turn`'s fold from a history answer and announce it.
pub fn install(shared: &mut Shared, thread: &str, turn: &str, history: &Value) -> Vec<AppEvent> {
    if !shared.live.install(thread, history) {
        return Vec::new();
    }
    if history["session"]["runningTurnId"] == turn {
        shared.running.insert(thread.to_string(), turn.to_string());
    }
    shared
        .live
        .turn(thread, turn)
        .map(|fold| recovered(thread, fold))
        .unwrap_or_default()
}

/// `turn/completed` for a host outcome (`{"kind": ...}`), mapping honestly:
/// a natural end completes, a cancellation interrupts, and every other stop
/// reason or failure is shown as an error with its reason.
pub fn turn_completed(thread: &str, turn: &str, outcome: Option<&Value>) -> AppEvent {
    let (status, message) = match outcome {
        Some(outcome) => match (str_of(outcome, "kind"), str_of(outcome, "stopReason")) {
            ("stopped", "end_turn") => ("completed", None),
            ("stopped", "cancelled") | ("cancelled", _) => ("interrupted", None),
            ("stopped", "max_tokens") => {
                ("failed", Some("The agent stopped: it reached its token limit.".to_string()))
            },
            ("stopped", "max_turn_requests") => (
                "failed",
                Some("The agent stopped: it reached its request limit for this turn.".to_string()),
            ),
            ("stopped", "refusal") => (
                "failed",
                Some(
                    "The agent refused to continue; this message will not be part of the next \
                     prompt."
                        .to_string(),
                ),
            ),
            ("stopped", other) => ("failed", Some(format!("The agent stopped ({other})."))),
            ("failed", _) => ("failed", Some(str_of(outcome, "message").to_string())),
            ("agentExited", _) => {
                ("failed", Some("The agent process exited before finishing this turn.".to_string()))
            },
            _ => ("failed", Some("The agent ended this turn in an unrecognized way.".to_string())),
        },
        None => (
            "failed",
            Some(
                "This turn's outcome is unknown after the connection to the agent host was \
                 restored."
                    .to_string(),
            ),
        ),
    };
    let mut turn_raw = json!({"id": turn, "status": status});
    if let Some(message) = message {
        turn_raw["error"] = json!({"message": message});
    }
    event("turn/completed", Some(thread), json!({"threadId": thread, "turn": turn_raw}))
}

fn turn_started(thread: &str, turn: &str) -> AppEvent {
    event(
        "turn/started",
        Some(thread),
        json!({"threadId": thread, "turnId": turn, "turn": {"id": turn, "status": "inProgress"}}),
    )
}

/// The request event for a pending permission body.
pub fn permission_event(body: &Value) -> AppEvent {
    let thread = str_of(body, "sessionId");
    let title = body["toolCall"]["title"].as_str().unwrap_or("").to_string();
    let raw = json!({
        "threadId": thread,
        "turnId": body["turnId"],
        "toolCall": body["toolCall"],
        "options": body["options"],
    });
    AppEvent {
        kind: PERMISSION_REQUESTED.to_string(),
        thread_id: Some(thread.to_string()),
        item_id: None,
        item_type: None,
        title: Some(title),
        text: None,
        images: Vec::new(),
        request_id: body["handle"].as_str().map(str::to_string),
        raw: raw.to_string(),
    }
}

fn resolved(thread: &str, handle: &str) -> AppEvent {
    event("serverRequest/resolved", Some(thread), json!({"threadId": thread, "requestId": handle}))
}

/// The host can no longer be reached through this connection.
pub fn host_unreachable() -> AppEvent {
    event(HOST_STATE, None, json!({"connected": false}))
}

/// The event stream failed: nothing the UI holds can be vouched for any
/// more. Capabilities fall back to the unnegotiated set, pending
/// permissions leave the UI (they return on recovery), and the UI is told.
pub fn disconnected(shared: &mut Shared) -> Vec<AppEvent> {
    let was_healthy = std::mem::replace(&mut shared.healthy, false);
    let mut out: Vec<AppEvent> = shared
        .permissions
        .drain()
        .map(|(handle, body)| resolved(str_of(&body, "sessionId"), &handle))
        .collect();
    if was_healthy {
        out.push(host_unreachable());
    }
    out
}

/// Translate one gateway event, updating the connection's view.
pub fn gateway_event(shared: &mut Shared, body: &Value) -> Vec<AppEvent> {
    let thread = str_of(body, "sessionId").to_string();
    let turn = str_of(body, "turnId").to_string();
    match str_of(body, "type") {
        "turn_started" => {
            shared.running.insert(thread.clone(), turn.clone());
            shared.live.apply(body);
            vec![turn_started(&thread, &turn)]
        },
        "update" => {
            let folded = shared.live.apply(body);
            changes(&thread, body["turnId"].as_str().unwrap_or(""), folded)
        },
        "turn_ended" => {
            shared.running.remove(&thread);
            let out = completion(
                &thread,
                &turn,
                shared.live.turn(&thread, &turn),
                Some(&body["outcome"]),
            );
            let running = &shared.running;
            shared
                .live
                .bound(MAX_LIVE, |session| running.contains_key(session));
            out
        },
        "permission" => {
            if let Some(handle) = body["handle"].as_str() {
                shared.permissions.insert(handle.to_string(), body.clone());
            }
            vec![permission_event(body)]
        },
        "permission_resolved" => {
            let handle = str_of(body, "handle").to_string();
            shared.permissions.remove(&handle);
            vec![resolved(&thread, &handle)]
        },
        "session_state" => session_state(shared, body),
        "host_state" => {
            shared.info["phase"] = body["phase"].clone();
            shared.info["authRequired"] = body["authRequired"].clone();
            vec![event(
                HOST_STATE,
                None,
                json!({"phase": body["phase"], "authRequired": body["authRequired"], "connected": true}),
            )]
        },
        _ => Vec::new(),
    }
}

fn session_state(shared: &mut Shared, body: &Value) -> Vec<AppEvent> {
    let thread = str_of(body, "sessionId").to_string();
    let previous_title = shared.sessions.get(&thread).map(|old| old["title"].clone());
    shared.sessions.insert(thread.clone(), body.clone());
    let mut out = vec![event(SESSION_UPDATED, Some(&thread), body.clone())];
    if previous_title.is_some_and(|old| old != body["title"]) {
        out.push(event(
            "thread/name/updated",
            Some(&thread),
            json!({"threadId": thread, "name": body["title"]}),
        ));
    }
    out
}

/// What a recovery has to do after reconciling with a snapshot, in this
/// order: complete `ended`, announce `started`, rebuild `running`, then emit
/// `events` and finally `host_state`.
pub struct Plan {
    /// Session and permission events.
    pub events: Vec<AppEvent>,
    /// The host-state event, last.
    pub host_state: AppEvent,
    /// Generation of the snapshot.
    pub generation: u64,
    /// Turns that ended meanwhile: `(thread, turn, recorded outcome)`.
    pub ended: Vec<(String, String, Option<Value>)>,
    /// Running turns this view had not seen begin.
    pub started: Vec<(String, String)>,
    /// Turns running now, whose folds must be rebuilt.
    pub running: Vec<(String, String)>,
}

/// Reconcile with an authoritative snapshot after a connect, reconnect or
/// reset. Live folds are dropped (the caller rebuilds the running ones from
/// history); turns that ended meanwhile are handed to the caller with their
/// recorded outcome — never an assumed success; permissions and session
/// state are replaced. A snapshot says nothing about the event stream, so
/// the view's health is left to whoever opens the stream.
pub fn reconcile(shared: &mut Shared, snapshot: &Value) -> Plan {
    let mut events = Vec::new();
    let generation = snapshot["generation"].as_u64().unwrap_or(0);
    let host_id = str_of(&snapshot["info"], "hostId").to_string();
    let replaced = host_id != shared.host_id;
    shared.host_id = host_id;
    shared.generation = generation;
    shared.info = snapshot["info"].clone();
    shared.live.clear();
    shared.unsynced.clear();
    if replaced {
        shared.oldest.clear();
    }
    let recent: HashMap<(String, String), Value> = snapshot["recentTurns"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            (
                (str_of(r, "sessionId").to_string(), str_of(r, "turnId").to_string()),
                r["outcome"].clone(),
            )
        })
        .collect();
    let mut running = HashMap::new();
    let mut sessions = HashMap::new();
    for session in snapshot["sessions"].as_array().into_iter().flatten() {
        let id = str_of(session, "sessionId").to_string();
        if let Some(turn) = session["runningTurnId"].as_str() {
            running.insert(id.clone(), turn.to_string());
        }
        sessions.insert(id, session.clone());
    }
    let previous = std::mem::take(&mut shared.running);
    let ended: Vec<(String, String, Option<Value>)> = previous
        .iter()
        .filter(|(thread, turn)| running.get(*thread) != Some(*turn))
        .map(|(thread, turn)| {
            let outcome = recent.get(&(thread.clone(), turn.clone())).cloned();
            (thread.clone(), turn.clone(), outcome)
        })
        .collect();
    let mut started: Vec<(String, String)> = running
        .iter()
        .filter(|(thread, turn)| previous.get(*thread) != Some(*turn))
        .map(|(thread, turn)| (thread.clone(), turn.clone()))
        .collect();
    started.sort();
    let mut running_list: Vec<(String, String)> = running
        .iter()
        .map(|(thread, turn)| (thread.clone(), turn.clone()))
        .collect();
    running_list.sort();
    shared.running = running;
    let pending: HashMap<String, Value> = snapshot["permissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["handle"].as_str()?.to_string(), p.clone())))
        .collect();
    let kept: HashSet<&String> = pending.keys().collect();
    for (handle, body) in &shared.permissions {
        if !kept.contains(handle) {
            events.push(resolved(str_of(body, "sessionId"), handle));
        }
    }
    for (handle, body) in &pending {
        if !shared.permissions.contains_key(handle) {
            events.push(permission_event(body));
        }
    }
    shared.permissions = pending;
    shared.sessions = sessions;
    for body in shared.sessions.values() {
        events.push(event(SESSION_UPDATED, Some(str_of(body, "sessionId")), body.clone()));
    }
    let host_state = event(
        HOST_STATE,
        None,
        json!({
            "phase": shared.info["phase"], "authRequired": shared.info["authRequired"],
            "generation": generation, "connected": true, "replaced": replaced,
        }),
    );
    Plan {
        events,
        host_state,
        generation,
        ended,
        started,
        running: running_list,
    }
}

/// `turn/started` for a running turn a recovery found.
pub fn started(thread: &str, turn: &str) -> AppEvent {
    turn_started(thread, turn)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pocket_codex_host_svc::acp::fold::Transcript;

    use super::*;

    fn kinds(events: &[AppEvent]) -> Vec<&str> {
        events.iter().map(|e| e.kind.as_str()).collect()
    }

    /// What the session screen shows, applying events the way it does:
    /// deltas append, anything else with text replaces.
    #[derive(Default)]
    struct Screen {
        items: HashMap<String, String>,
        order: Vec<String>,
    }

    impl Screen {
        fn apply(&mut self, events: &[AppEvent]) {
            for event in events {
                let (Some(id), Some(kind)) = (&event.item_id, &event.item_type) else { continue };
                let text = event.text.clone().unwrap_or_default();
                // As the session screen does: an empty notice removes it.
                if kind == CONTENT_OMITTED && text.is_empty() {
                    self.items.remove(id);
                    self.order.retain(|known| known != id);
                    continue;
                }
                if !self.items.contains_key(id) {
                    self.order.push(id.clone());
                }
                let entry = self.items.entry(id.clone()).or_default();
                if event.kind.contains("delta") {
                    entry.push_str(&text);
                } else if !text.is_empty() {
                    *entry = text;
                }
            }
        }

        fn agent_items(&self) -> Vec<(String, String)> {
            self.order
                .iter()
                .map(|id| (id.clone(), self.items[id].clone()))
                .collect()
        }
    }

    fn chunk(text: &str) -> Value {
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
    }

    #[test]
    fn a_live_turn_maps_to_shared_item_events() {
        let mut shared = Shared::default();
        let started = gateway_event(
            &mut shared,
            &json!({"type": "turn_started", "sessionId": "s", "turnId": "t", "userText": "q", "seq": 1}),
        );
        assert_eq!(kinds(&started), vec!["turn/started"]);
        let first = gateway_event(
            &mut shared,
            &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": 2,
            "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "He"}}}),
        );
        assert_eq!(kinds(&first), vec!["item/started", "item/agentMessage/delta"]);
        assert_eq!(first[0].item_id.as_deref(), Some("t:m1"), "the host's id");
        let tool = gateway_event(
            &mut shared,
            &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": 3,
            "update": {"sessionUpdate": "tool_call", "toolCallId": "c", "title": "ls", "kind": "execute", "status": "in_progress"}}),
        );
        assert_eq!(tool[0].kind, "item/started");
        assert_eq!(tool[0].item_type.as_deref(), Some("commandExecution"));
        let done = gateway_event(
            &mut shared,
            &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": 4,
            "update": {"sessionUpdate": "tool_call_update", "toolCallId": "c", "status": "completed"}}),
        );
        assert_eq!(done[0].kind, "item/completed");
        let ended = gateway_event(
            &mut shared,
            &json!({"type": "turn_ended", "sessionId": "s", "turnId": "t", "seq": 5,
            "outcome": {"kind": "stopped", "stopReason": "end_turn"}}),
        );
        assert_eq!(kinds(&ended), vec!["item/completed", "item/completed", "turn/completed"]);
        assert!(ended[2].raw.contains("\"completed\""));
        assert!(shared.running.is_empty());
    }

    #[test]
    fn outcomes_are_never_upgraded_to_success() {
        for (outcome, status) in [
            (json!({"kind": "stopped", "stopReason": "cancelled"}), "interrupted"),
            (json!({"kind": "cancelled"}), "interrupted"),
            (json!({"kind": "stopped", "stopReason": "refusal"}), "failed"),
            (json!({"kind": "agentExited"}), "failed"),
            (json!({"kind": "failed", "message": "boom"}), "failed"),
        ] {
            let event = turn_completed("s", "t", Some(&outcome));
            let raw: Value = serde_json::from_str(&event.raw).expect("json");
            assert_eq!(raw["turn"]["status"], status, "{outcome}");
        }
        let unknown: Value =
            serde_json::from_str(&turn_completed("s", "t", None).raw).expect("json");
        assert_eq!(unknown["turn"]["status"], "failed");
    }

    #[test]
    fn permissions_carry_the_handle_and_exact_options() {
        let mut shared = Shared::default();
        let asked = gateway_event(
            &mut shared,
            &json!({"type": "permission", "handle": "h1", "sessionId": "s",
            "turnId": "t", "toolCall": {"title": "Run"}, "options": [{"optionId": "opt:1 ✓", "name": "Allow", "kind": "allow_once"}]}),
        );
        assert_eq!(asked[0].kind, PERMISSION_REQUESTED);
        assert_eq!(asked[0].request_id.as_deref(), Some("h1"));
        assert!(asked[0].raw.contains("opt:1 ✓"));
        let gone = gateway_event(
            &mut shared,
            &json!({"type": "permission_resolved", "handle": "h1", "sessionId": "s"}),
        );
        assert_eq!(gone[0].kind, "serverRequest/resolved");
        assert!(shared.permissions.is_empty());
    }

    #[test]
    fn reconcile_plans_recorded_outcomes_and_restores_pending_requests() {
        let mut shared = Shared {
            generation: 3,
            ..Shared::default()
        };
        shared.running.insert("s".into(), "t".into());
        shared.running.insert("u".into(), "lost".into());
        shared
            .permissions
            .insert("old".into(), json!({"handle": "old", "sessionId": "s"}));
        let plan = reconcile(
            &mut shared,
            &json!({
                "generation": 4, "seq": 9, "info": {"phase": {"state": "ready"}, "hostId": "h"},
                "sessions": [{"sessionId": "v", "runningTurnId": "n"}],
                "permissions": [{"handle": "new", "sessionId": "v", "toolCall": {}, "options": []}],
                "recentTurns": [{"sessionId": "s", "turnId": "t", "outcome": {"kind": "agentExited"}}],
            }),
        );
        let mut ended = plan.ended.clone();
        ended.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(ended.len(), 2);
        assert_eq!(ended[0].2, Some(json!({"kind": "agentExited"})));
        assert_eq!(ended[1].2, None, "an unknown outcome is not assumed");
        assert_eq!(plan.running, vec![("v".to_string(), "n".to_string())]);
        assert_eq!(plan.started, vec![("v".to_string(), "n".to_string())]);
        let events = &plan.events;
        assert!(
            events.iter().all(|e| e.kind != "turn/started"),
            "successors are announced by the caller, after completions"
        );
        assert!(events.iter().any(|e| e.kind == "serverRequest/resolved"));
        assert!(events
            .iter()
            .any(|e| e.kind == PERMISSION_REQUESTED && e.request_id.as_deref() == Some("new")));
        assert_eq!(shared.generation, 4);
        assert!(!shared.healthy, "a snapshot is not an open event stream");
    }

    /// The host folds every update; a controller joins after `joined`
    /// updates, recovers through history at that watermark, then applies
    /// the stream from an older watermark. The screen must end up exactly
    /// with the host's items, ids and text.
    #[test]
    fn joining_mid_turn_shows_exactly_the_hosts_items() {
        let updates = [
            chunk("He"),
            chunk("llo"),
            json!({"sessionUpdate": "tool_call", "toolCallId": "c", "title": "ls", "kind": "execute", "status": "completed"}),
            chunk("Done"),
            chunk(" now"),
        ];
        let joined = 2;
        let mut host = Transcript::default();
        host.begin_turn("t", "q", &[]);
        let mut events = vec![json!({"type": "turn_started", "sessionId": "s", "turnId": "t",
            "userText": "q", "seq": 1})];
        let mut at_join = None;
        for (n, update) in updates.iter().enumerate() {
            host.apply(Some("t"), update);
            events.push(json!({"type": "update", "sessionId": "s", "turnId": "t",
                "update": update, "seq": n as u64 + 2}));
            if n + 1 == joined {
                at_join = Some(host.turns[0].clone());
            }
        }
        let history = json!({"window": [at_join.expect("joined")], "truncated": false,
            "seq": joined as u64 + 1, "generation": 1,
            "session": {"runningTurnId": "t"}});

        let mut shared = Shared::default();
        let mut screen = Screen::default();
        screen.apply(&install(&mut shared, "s", "t", &history));
        for event in &events {
            screen.apply(&gateway_event(&mut shared, event));
        }
        screen.apply(&gateway_event(
            &mut shared,
            &json!({"type": "turn_ended", "sessionId": "s", "turnId": "t", "seq": 99,
                "outcome": {"kind": "stopped", "stopReason": "end_turn"}}),
        ));
        let expected: Vec<(String, String)> = host.turns[0]
            .items
            .iter()
            .filter(|item| item.kind != ItemKind::UserMessage)
            .map(|item| {
                let mapped = app_item(item, "t");
                (mapped.id, mapped.text)
            })
            .collect();
        assert_eq!(screen.agent_items(), expected);
        assert_eq!(screen.items["t:m1"], "Hello", "no missing or duplicated prefix");
    }

    #[test]
    fn a_turn_that_ended_during_a_gap_completes_with_its_final_text() {
        let mut host = Transcript::default();
        host.begin_turn("t", "q", &[]);
        host.apply(Some("t"), &chunk("full answer"));
        let history = json!({"window": [host.turns[0]], "generation": 2});
        let events = finished(
            "s",
            "t",
            Some(&history),
            Some(&json!({"kind": "stopped", "stopReason": "end_turn"})),
        );
        assert_eq!(kinds(&events), vec!["item/completed", "turn/completed"]);
        assert_eq!(events[0].text.as_deref(), Some("full answer"));
        assert_eq!(events[0].item_id.as_deref(), Some("t:m1"));
        // Without history the outcome is still reported, not invented, and
        // the missing items are said to be missing.
        let unread = finished("s", "t", None, None);
        assert_eq!(kinds(&unread), vec!["item/completed", "turn/completed"]);
        assert_eq!(unread[0].item_type.as_deref(), Some(CONTENT_OMITTED));
        assert_eq!(unread[0].text.as_deref(), Some(UNAVAILABLE));
    }

    #[test]
    fn item_events_name_their_turn_and_dropped_content_gets_a_notice() {
        let mut shared = Shared::default();
        gateway_event(
            &mut shared,
            &json!({"type": "turn_started", "sessionId": "s", "turnId": "t", "userText": "q", "seq": 1}),
        );
        let oversized = "A".repeat(pocket_codex_host_svc::acp::fold::MAX_ITEM_IMAGE_BYTES + 1);
        let image = gateway_event(
            &mut shared,
            &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": 2,
                "update": {"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "image", "mimeType": "image/png", "data": oversized}}}),
        );
        let notice = image
            .iter()
            .find(|e| e.item_type.as_deref() == Some(CONTENT_OMITTED))
            .expect("a visible notice for the dropped image");
        assert_eq!(notice.text.as_deref(), Some("images"));
        assert_eq!(notice.item_id.as_deref(), Some("#omitted:t:m1"));
        let text = gateway_event(
            &mut shared,
            &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": 3,
                "update": chunk("caption")}),
        );
        assert_eq!(kinds(&text), vec!["item/agentMessage/delta"], "text keeps streaming");
        assert_eq!(text[0].text.as_deref(), Some("caption"));
        for event in image.iter().chain(&text) {
            let raw: Value = serde_json::from_str(&event.raw).expect("json");
            assert_eq!(raw["turnId"], "t", "{} names its turn", event.kind);
        }
        // A reload shows the same notice in place.
        let fold = shared.live.turn("s", "t").expect("fold").clone();
        let types: Vec<String> = turn_items(&fold).into_iter().map(|i| i.item_type).collect();
        assert_eq!(types, vec!["userMessage", "agentMessage", CONTENT_OMITTED]);
    }

    /// Tool `x` drops an oversized diff while another tool is legally named
    /// `x:omitted`. Every row stays separately addressable; when `x`'s diff
    /// is replaced by one that fits, its notice is re-sent empty (which the
    /// UI removes), and the other tool is never touched by it.
    #[test]
    fn notices_never_collide_with_agent_ids_and_clear_when_content_fits() {
        let mut shared = Shared::default();
        gateway_event(
            &mut shared,
            &json!({"type": "turn_started", "sessionId": "s", "turnId": "t", "userText": "q", "seq": 1}),
        );
        let diff = |text: String| json!([{"type": "diff", "path": "/w/a.txt", "newText": text}]);
        let mut seq = 1;
        let mut update = |shared: &mut Shared, update: Value| {
            seq += 1;
            gateway_event(
                shared,
                &json!({"type": "update", "sessionId": "s", "turnId": "t", "seq": seq,
                    "update": update}),
            )
        };
        let oversized = pocket_codex_host_svc::acp::fold::MAX_ITEM_TEXT + 1;
        let x = update(
            &mut shared,
            json!({"sessionUpdate": "tool_call", "toolCallId": "x", "title": "Edit", "kind": "edit",
                "status": "in_progress", "content": diff("n".repeat(oversized))}),
        );
        let other = update(
            &mut shared,
            json!({"sessionUpdate": "tool_call", "toolCallId": "x:omitted", "title": "List",
                "kind": "read", "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "listing"}}]}),
        );
        let mut screen = Screen::default();
        screen.apply(&x);
        screen.apply(&other);
        let typed = |events: &[AppEvent], id: &str| {
            events
                .iter()
                .filter(|e| e.item_id.as_deref() == Some(id))
                .map(|e| e.item_type.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        };
        assert_eq!(typed(&x, "#omitted:tool:x"), vec![CONTENT_OMITTED]);
        assert_eq!(screen.items["#omitted:tool:x"], "diffs");
        assert_eq!(typed(&other, "tool:x:omitted"), vec!["dynamicToolCall"]);
        assert!(other
            .iter()
            .all(|e| e.item_id.as_deref() != Some("#omitted:tool:x")));
        assert!(screen.items["tool:x:omitted"].contains("listing"));

        // The replacement fits: the fold clears the omission, and so does
        // the notice — without a reconnect or a history read.
        let small = update(
            &mut shared,
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "x", "status": "completed",
                "content": diff("small".into())}),
        );
        let cleared = small
            .iter()
            .find(|e| e.item_id.as_deref() == Some("#omitted:tool:x"))
            .expect("the notice is re-stated");
        assert_eq!(cleared.item_type.as_deref(), Some(CONTENT_OMITTED));
        assert_eq!(cleared.text.as_deref(), Some(""), "empty: the notice is removed");
        assert!(small
            .iter()
            .all(|e| e.item_id.as_deref() != Some("tool:x:omitted")));
        let fold = shared.live.turn("s", "t").expect("fold").clone();
        assert!(
            turn_items(&fold)
                .iter()
                .all(|i| i.item_type != CONTENT_OMITTED),
            "a reload shows no notice either"
        );

        // Updating the other tool restates only its own (empty) notice.
        let touched = update(
            &mut shared,
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "x:omitted",
                "title": "List again"}),
        );
        assert!(touched
            .iter()
            .all(|e| e.item_id.as_deref() != Some("tool:x")
                && e.item_id.as_deref() != Some("#omitted:tool:x")));
        assert_eq!(typed(&touched, "tool:x:omitted"), vec!["dynamicToolCall"]);
    }

    #[test]
    fn a_failed_stream_withdraws_pending_permissions_and_says_so_once() {
        let mut shared = Shared {
            healthy: true,
            ..Shared::default()
        };
        shared
            .permissions
            .insert("h".into(), json!({"handle": "h", "sessionId": "s"}));
        let events = disconnected(&mut shared);
        assert_eq!(kinds(&events), vec!["serverRequest/resolved", HOST_STATE]);
        assert!(!shared.healthy);
        assert!(shared.permissions.is_empty());
        assert!(disconnected(&mut shared).is_empty(), "reported once");
    }

    #[test]
    fn a_replaced_host_is_a_fresh_view() {
        let mut shared = Shared::default();
        reconcile(
            &mut shared,
            &json!({"generation": 1, "seq": 5, "info": {"hostId": "a"},
            "sessions": [{"sessionId": "s", "runningTurnId": "t"}]}),
        );
        shared.oldest.insert("s".into(), "t".into());
        let plan = reconcile(
            &mut shared,
            &json!({"generation": 1, "seq": 2, "info": {"hostId": "b"},
            "sessions": []}),
        );
        assert_eq!(plan.ended.len(), 1, "the old host's turn is reported, not kept");
        assert_eq!(plan.ended[0].2, None);
        assert!(shared.oldest.is_empty());
        let raw: Value = serde_json::from_str(&plan.host_state.raw).expect("json");
        assert_eq!(raw["replaced"], true);
    }

    #[test]
    fn evicted_items_show_a_gap_inside_their_turn() {
        let mut turn: Turn = serde_json::from_value(json!({
            "id": "t", "replayed": false, "outcome": null, "omitted": 3, "nextItem": 9,
            "items": [
                {"id": "t:u", "kind": "user_message", "title": "", "text": "q"},
                {"id": "t:m8", "kind": "agent_message", "title": "", "text": "last"},
            ],
        }))
        .expect("turn");
        let items = turn_items(&turn);
        let types: Vec<&str> = items.iter().map(|i| i.item_type.as_str()).collect();
        assert_eq!(types, vec!["userMessage", "historyGap", "agentMessage"]);
        turn.omitted = 0;
        assert_eq!(turn_items(&turn).len(), 2);
    }
}

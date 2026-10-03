//! Messages from the agent: folding `session/update`, registering pending
//! requests, local fs requests, and cleanup when the process exits.

use std::sync::Arc;

use pocket_codex_core::acp::{
    methods,
    pcx::{
        auth_status, notifications, stop_reason, FailedPrompt, QueueFailedParams,
        RequestResolvedParams, TurnCompletedParams, UpdateMeta,
    },
    rpc::{code, RequestId, RpcError, RpcMessage},
    Applied, ElicitationRequest, InitializeResponse, ReadTextFileRequest, RequestPermissionRequest,
    SessionNotification, SessionUpdate, WriteTextFileRequest,
};
use serde_json::{json, Value};
use tracing::debug;

use super::{
    auth,
    hub::{lock, now_ms, AcpHub, GatewayOutcome, HubState},
    peer::Inbound,
    pending::{Pending, PendingKind},
    session::{prompt_text, Phase},
};

/// Notice inserted when a line from the agent exceeded the size limit.
const OVERSIZED_NOTICE: &str = "一条来自 agent 的消息超过 16 MiB，已丢弃";

impl AcpHub {
    /// Entry point of the peer read loop for run `run_id`.
    pub(super) fn on_agent(self: &Arc<Self>, run_id: u64, message: Inbound) {
        let mut st = lock(&self.state);
        if st.run.as_ref().map(|r| r.id) != Some(run_id) {
            return;
        }
        match message {
            Inbound::Notification {
                method,
                params,
            } => match method.as_str() {
                methods::SESSION_UPDATE => on_update(&mut st, params),
                methods::ELICITATION_COMPLETE => on_elicitation_complete(&mut st, &params),
                notifications::INTERNAL_OVERSIZED => on_oversized(&mut st),
                _ => debug!(%method, "ignoring agent notification"),
            },
            Inbound::Request {
                id,
                method,
                params,
            } => match method.as_str() {
                methods::SESSION_REQUEST_PERMISSION => on_permission(&mut st, run_id, id, params),
                methods::ELICITATION_CREATE => on_elicitation(&mut st, run_id, id, params),
                methods::FS_READ_TEXT_FILE | methods::FS_WRITE_TEXT_FILE => {
                    drop(st);
                    self.spawn_fs(run_id, id, method, params);
                },
                _ => respond(
                    &st,
                    run_id,
                    id,
                    Err(RpcError::new(
                        code::METHOD_NOT_FOUND,
                        format!("{method} is not supported"),
                    )),
                ),
            },
        }
    }

    fn spawn_fs(self: &Arc<Self>, run_id: u64, id: RequestId, method: String, params: Value) {
        let hub = self.clone();
        tokio::spawn(async move {
            let session = params
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            let cwd = session.as_deref().and_then(|s| hub.session_cwd(s));
            let result = tokio::task::spawn_blocking(move || {
                let Some(cwd) = cwd else {
                    return Err(super::error::AcpError::NotFound("unknown session".into()));
                };
                if method == methods::FS_READ_TEXT_FILE {
                    let request: ReadTextFileRequest = serde_json::from_value(params)
                        .map_err(|e| super::error::AcpError::InvalidParams(e.to_string()))?;
                    super::fs::read_text_file(&cwd, &request)
                } else {
                    let request: WriteTextFileRequest = serde_json::from_value(params)
                        .map_err(|e| super::error::AcpError::InvalidParams(e.to_string()))?;
                    super::fs::write_text_file(&cwd, &request)
                }
            })
            .await
            .unwrap_or_else(|e| Err(super::error::AcpError::Internal(e.to_string())));
            let st = lock(&hub.state);
            respond(&st, run_id, id, result.map_err(|e| e.to_rpc()));
        });
    }
}

/// Answer an agent request if `run_id` is still the current process.
pub(super) fn respond(st: &HubState, run_id: u64, id: RequestId, result: Result<Value, RpcError>) {
    if let Some(run) = st.run.as_ref().filter(|r| r.id == run_id) {
        let _ = run.peer.respond_now(id, result);
    }
}

/// `params` with `_meta.pcx` set to `pcx`.
pub(super) fn with_pcx(params: &Value, pcx: Value) -> Value {
    let mut params = match params {
        Value::Object(map) => Value::Object(map.clone()),
        _ => json!({}),
    };
    match params.get_mut("_meta") {
        Some(Value::Object(meta)) => {
            meta.insert("pcx".into(), pcx);
        },
        _ => params["_meta"] = json!({ "pcx": pcx }),
    }
    params
}

fn on_update(st: &mut HubState, params: Value) {
    let note: SessionNotification = match serde_json::from_value(params.clone()) {
        Ok(note) => note,
        Err(e) => {
            debug!("ignoring malformed session/update: {e}");
            return;
        },
    };
    let mut caps_changed = false;
    let Some(session) = st.sessions.get_mut(&note.session_id) else {
        debug!(session = %note.session_id, "session/update for an unknown session");
        return;
    };
    if let Some(replay) = session.replay.as_mut() {
        replay.apply(&note.update, None);
        apply_session_fields(session, &note.update);
        return;
    }
    session.last_activity = tokio::time::Instant::now();
    match &note.update {
        SessionUpdate::ConfigOptionUpdate {
            ..
        } => caps_changed |= !st.caps.config_options,
        SessionUpdate::AvailableCommandsUpdate {
            ..
        } => caps_changed |= !st.caps.commands,
        SessionUpdate::CurrentModeUpdate {
            ..
        } => caps_changed |= !st.caps.modes,
        _ => {},
    }
    let Some(session) = st.sessions.get_mut(&note.session_id) else { return };
    apply_session_fields(session, &note.update);
    let Some(transcript) = session.transcript.as_mut() else { return };
    let applied = transcript.apply(&note.update, Some(now_ms()));
    let generation = transcript.generation().to_string();
    let (item_id, created, turn, item) = match &applied {
        Applied::Item {
            id,
            created,
            ..
        } => {
            let item = transcript.item(id).cloned();
            let turn = item.as_ref().map_or(transcript.current_turn(), |i| i.turn);
            let full = matches!(
                note.update,
                SessionUpdate::ToolCall(_)
                    | SessionUpdate::ToolCallUpdate(_)
                    | SessionUpdate::Plan { .. }
            );
            (Some(id.clone()), Some(*created), turn, item.filter(|_| full))
        },
        Applied::Session => (None, None, transcript.current_turn(), None),
        Applied::Ignored => return,
    };
    if caps_changed {
        match &note.update {
            SessionUpdate::ConfigOptionUpdate {
                ..
            } => st.caps.config_options = true,
            SessionUpdate::AvailableCommandsUpdate {
                ..
            } => st.caps.commands = true,
            SessionUpdate::CurrentModeUpdate {
                ..
            } => st.caps.modes = true,
            _ => {},
        }
        st.broadcast_hub_state();
    }
    let session_id = note.session_id.clone();
    st.notify_session(&session_id, methods::SESSION_UPDATE, |seq| {
        let meta = UpdateMeta {
            seq,
            item_id,
            created,
            turn,
            generation,
            item,
        };
        with_pcx(&params, serde_json::to_value(meta).unwrap_or(Value::Null))
    });
}

/// Session-level fields carried by an update.
fn apply_session_fields(session: &mut super::session::HubSession, update: &SessionUpdate) {
    match update {
        SessionUpdate::ConfigOptionUpdate {
            config_options,
        } => {
            session.config_options = config_options.clone();
        },
        SessionUpdate::CurrentModeUpdate {
            current_mode_id,
        } => {
            if let Some(modes) = session.modes.as_mut() {
                modes.current_mode_id = current_mode_id.clone();
            }
        },
        SessionUpdate::AvailableCommandsUpdate {
            available_commands,
        } => {
            session.commands = available_commands.clone();
        },
        SessionUpdate::UsageUpdate(usage) => session.usage = Some(usage.clone()),
        SessionUpdate::SessionInfoUpdate {
            title,
            updated_at,
        } => {
            if let Some(title) = title {
                session.title = title.clone();
            }
            if let Some(updated_at) = updated_at {
                session.updated_at = updated_at.clone();
            }
        },
        _ => {},
    }
}

fn on_oversized(st: &mut HubState) {
    let mut targets: Vec<String> = st
        .sessions
        .values()
        .filter(|s| s.running())
        .map(|s| s.id.clone())
        .collect();
    if targets.is_empty() {
        let latest = st
            .sessions
            .values()
            .filter(|s| s.transcript.is_some())
            .max_by_key(|s| s.last_activity)
            .map(|s| s.id.clone());
        targets.extend(latest);
    }
    for id in targets {
        let Some(session) = st.sessions.get_mut(&id) else { continue };
        let Some(transcript) = session.transcript.as_mut() else { continue };
        let item_id = transcript.push_notice(OVERSIZED_NOTICE, Some(now_ms()));
        let item = transcript.item(&item_id).cloned();
        let generation = transcript.generation().to_string();
        let turn = item.as_ref().map_or(0, |i| i.turn);
        st.notify_session(&id, methods::SESSION_UPDATE, |seq| {
            let meta = UpdateMeta {
                seq,
                item_id: Some(item_id.clone()),
                created: Some(true),
                turn,
                generation,
                item,
            };
            json!({
                "sessionId": id,
                "update": {"sessionUpdate": "_pcx_notice", "text": OVERSIZED_NOTICE},
                "_meta": {"pcx": meta}
            })
        });
    }
}

fn on_permission(st: &mut HubState, run_id: u64, id: RequestId, params: Value) {
    let request: RequestPermissionRequest = match serde_json::from_value(params.clone()) {
        Ok(request) => request,
        Err(e) => {
            respond(st, run_id, id, Err(RpcError::new(code::INVALID_PARAMS, e.to_string())));
            return;
        },
    };
    let kind = PendingKind::Permission {
        tool_call: Box::new(request.tool_call),
        options: request.options,
    };
    let session = Some(request.session_id).filter(|s| st.sessions.contains_key(s));
    register(st, run_id, id, methods::SESSION_REQUEST_PERMISSION, params, session, kind);
}

fn on_elicitation(st: &mut HubState, run_id: u64, id: RequestId, params: Value) {
    let request: ElicitationRequest = match serde_json::from_value(params.clone()) {
        Ok(request) => request,
        Err(e) => {
            respond(st, run_id, id, Err(RpcError::new(code::INVALID_PARAMS, e.to_string())));
            return;
        },
    };
    let kind = match request.mode.as_str() {
        "url" => PendingKind::Url {
            message: request.message.clone(),
            url: request.url.clone().unwrap_or_default(),
            elicitation_id: request.elicitation_id.clone().unwrap_or_default(),
        },
        _ => PendingKind::Form {
            message: request.message.clone(),
            schema: request
                .requested_schema
                .clone()
                .unwrap_or_else(|| json!({})),
        },
    };
    let session = request.session_id.filter(|s| st.sessions.contains_key(s));
    register(st, run_id, id, methods::ELICITATION_CREATE, params, session, kind);
}

fn register(
    st: &mut HubState,
    run_id: u64,
    agent_request: RequestId,
    method: &str,
    params: Value,
    session_id: Option<String>,
    kind: PendingKind,
) {
    st.next_pending += 1;
    let id = format!("r{}", st.next_pending);
    let pending = Pending {
        id: id.clone(),
        session_id: session_id.clone(),
        kind,
        agent_request,
        run_id,
        method: method.to_string(),
        params,
        sent_to: Default::default(),
    };
    let mode = pending.mode();
    st.pending.insert(id.clone(), pending);
    let targets: Vec<u64> = match &session_id {
        Some(s) => st
            .sessions
            .get(s)
            .map(|s| s.subscribers.iter().copied().collect())
            .unwrap_or_default(),
        None => st
            .conns
            .iter()
            .filter(|(_, c)| c.initialized)
            .map(|(id, _)| *id)
            .collect(),
    };
    for conn in targets {
        if accepts(st, conn, mode) {
            deliver(st, &id, conn, None);
        }
    }
}

/// Whether `conn` declared the elicitation `mode` (permissions go to all).
pub(super) fn accepts(st: &HubState, conn: u64, mode: Option<&str>) -> bool {
    let Some(c) = st.conns.get(&conn) else { return false };
    match mode {
        None => true,
        Some("form") => c.form,
        Some(_) => c.url,
    }
}

/// Send pending `id` to `conn` under a fresh request id.
pub(super) fn deliver(st: &mut HubState, id: &str, conn: u64, rejected: Option<&str>) {
    let Some(pending) = st.pending.get(id) else { return };
    let method = pending.method.clone();
    let params = pending.controller_params(rejected);
    let Some(c) = st.conns.get_mut(&conn) else { return };
    let request_id = RequestId::Number(c.next_request);
    c.next_request += 1;
    c.outgoing.insert(request_id.clone(), id.to_string());
    if let Some(previous) = st
        .pending
        .get_mut(id)
        .and_then(|p| p.sent_to.insert(conn, request_id.clone()))
    {
        if let Some(c) = st.conns.get_mut(&conn) {
            c.outgoing.remove(&previous);
        }
    }
    st.send(conn, RpcMessage::Request {
        id: request_id,
        method,
        params,
    });
}

/// Remove pending `id`, answer the agent with `answer`, tell the other
/// recipients `$/cancel_request`, and broadcast `_pcx/request/resolved`.
pub(super) fn resolve(st: &mut HubState, id: &str, answer: Value, winner: Option<u64>) {
    let Some(pending) = st.pending.remove(id) else { return };
    respond(st, pending.run_id, pending.agent_request.clone(), Ok(answer));
    for (conn, request_id) in &pending.sent_to {
        if let Some(c) = st.conns.get_mut(conn) {
            c.outgoing.remove(request_id);
        }
        if Some(*conn) != winner {
            st.send(*conn, RpcMessage::Notification {
                method: methods::CANCEL_REQUEST.into(),
                params: json!({ "requestId": request_id }),
            });
        }
    }
    broadcast_resolved(st, &pending);
}

fn broadcast_resolved(st: &mut HubState, pending: &Pending) {
    match &pending.session_id {
        Some(session) => {
            let request_id = pending.id.clone();
            let session_id = session.clone();
            st.notify_session(session, notifications::REQUEST_RESOLVED, |seq| {
                serde_json::to_value(RequestResolvedParams {
                    session_id: Some(session_id),
                    request_id,
                    seq: Some(seq),
                })
                .unwrap_or(Value::Null)
            });
        },
        None => {
            let params = serde_json::to_value(RequestResolvedParams {
                session_id: None,
                request_id: pending.id.clone(),
                seq: None,
            })
            .unwrap_or(Value::Null);
            st.notify_all(notifications::REQUEST_RESOLVED, params);
        },
    }
}

fn on_elicitation_complete(st: &mut HubState, params: &Value) {
    let Some(elicitation) = params.get("elicitationId").and_then(Value::as_str) else { return };
    let ids: Vec<String> = st
        .pending
        .values()
        .filter(|p| matches!(&p.kind, PendingKind::Url { elicitation_id, .. } if elicitation_id == elicitation))
        .map(|p| p.id.clone())
        .collect();
    for id in ids {
        resolve(st, &id, json!({ "action": "accept" }), None);
    }
}

/// Cancel the pending requests of `session` (turn cancelled).
pub(super) fn cancel_pending(st: &mut HubState, session: &str) {
    let ids: Vec<(String, Value)> = st
        .pending
        .values()
        .filter(|p| p.session_id.as_deref() == Some(session))
        .map(|p| (p.id.clone(), p.cancel_response()))
        .collect();
    for (id, answer) in ids {
        resolve(st, &id, answer, None);
    }
}

/// Fail and clear the queue of `session`.
pub(super) fn fail_queue(st: &mut HubState, session: &str, reason: &str) {
    let Some(s) = st.sessions.get_mut(session) else { return };
    if s.queue.is_empty() {
        return;
    }
    let prompts: Vec<FailedPrompt> = s
        .queue
        .drain(..)
        .map(|q| FailedPrompt {
            submission_id: q.submission,
            text: prompt_text(&q.prompt),
        })
        .collect();
    let session_id = session.to_string();
    let reason = reason.to_string();
    st.notify_session(session, notifications::QUEUE_FAILED, |seq| {
        serde_json::to_value(QueueFailedParams {
            session_id,
            reason,
            prompts,
            seq,
        })
        .unwrap_or(Value::Null)
    });
}

/// End the live turn of `session` and broadcast `_pcx/turn/completed`.
pub(super) fn finish_turn(st: &mut HubState, session: &str, stop: &str, error: Option<String>) {
    let Some(s) = st.sessions.get_mut(session) else { return };
    let Phase::Running {
        turn,
        submission,
    } = s.phase.clone()
    else {
        return;
    };
    let now = now_ms();
    let started = s
        .transcript
        .as_ref()
        .and_then(|t| t.turn_info(turn))
        .and_then(|t| t.started_at_ms)
        .unwrap_or(now);
    if let Some(t) = s.transcript.as_mut() {
        t.end_live_turn(stop, now);
    }
    s.phase = if s.agent_loaded { Phase::Idle } else { Phase::Listed };
    s.baseline_pending = true;
    s.touch();
    let params = TurnCompletedParams {
        session_id: session.to_string(),
        turn,
        stop_reason: stop.to_string(),
        error: error.clone(),
        completed_at_ms: now,
        duration_ms: now - started,
        seq: 0,
    };
    st.notify_session(session, notifications::TURN_COMPLETED, |seq| {
        serde_json::to_value(TurnCompletedParams {
            seq,
            ..params
        })
        .unwrap_or(Value::Null)
    });
    if let Some(waiters) = st.turn_waiters.remove(&submission) {
        for waiter in waiters {
            let _ = waiter.send(match &error {
                None => Ok(stop.to_string()),
                Some(message) => Err(super::error::AcpError::AgentError {
                    code: code::INTERNAL_ERROR,
                    message: message.clone(),
                }),
            });
        }
    }
}

/// Reset sessions, turns, pending requests and queues after the agent
/// process went away (TRD §4.2.3 "Crashed").
pub(super) fn cleanup_after_exit(st: &mut HubState, reason: &str) {
    let ids: Vec<String> = st.sessions.keys().cloned().collect();
    for id in &ids {
        if st.sessions.get(id).is_some_and(|s| s.running()) {
            finish_turn(st, id, stop_reason::AGENT_EXITED, Some(reason.to_string()));
        }
        if let Some(s) = st.sessions.get_mut(id) {
            s.agent_loaded = false;
            s.replay = None;
            if !matches!(s.phase, Phase::Listed) {
                s.phase = Phase::Listed;
            }
        }
        fail_queue(st, id, "agent_exited");
    }
    let pending: Vec<Pending> = std::mem::take(&mut st.pending).into_values().collect();
    for p in &pending {
        for (conn, request_id) in &p.sent_to {
            if let Some(c) = st.conns.get_mut(conn) {
                c.outgoing.remove(request_id);
            }
            st.send(*conn, RpcMessage::Notification {
                method: methods::CANCEL_REQUEST.into(),
                params: json!({ "requestId": request_id }),
            });
        }
        broadcast_resolved(st, p);
    }
    st.list_cache = None;
}

/// Record `initialize` (and the gateway login result) in hub state.
pub(super) fn adopt_initialize(
    st: &mut HubState,
    init: &InitializeResponse,
    gateway: Option<&GatewayOutcome>,
) {
    let caps = &init.agent_capabilities;
    st.caps.list = caps.session_capabilities.list.is_some();
    st.caps.resume = caps.session_capabilities.resume.is_some();
    st.caps.close = caps.session_capabilities.close.is_some();
    st.caps.load = caps.load_session;
    st.caps.image = caps.prompt_capabilities.image;
    st.caps.embedded_context = caps.prompt_capabilities.embedded_context;
    st.caps.queue = true;
    st.caps.steer = false;
    st.caps.url_elicitation = true;
    st.agent_info = init.agent_info.clone();
    let (spec, configured) = match &st.run {
        Some(run) => (run.spec.clone(), run.gateway.clone()),
        None => (st.last_spec.clone(), None),
    };
    st.auth.methods = auth::classify(&init.auth_methods, &spec, configured.as_ref());
    st.auth_note = None;
    match gateway {
        Some(GatewayOutcome::Ok) => {
            st.auth.status = auth_status::OK.into();
            st.auth.message = None;
        },
        Some(GatewayOutcome::Failed(message)) => {
            st.auth.status = auth_status::REQUIRED.into();
            st.auth.message = Some(message.clone());
        },
        Some(GatewayOutcome::NoMethod(note)) => {
            st.auth.status = auth_status::UNKNOWN.into();
            st.auth.message = Some(note.clone());
            st.auth_note = Some(note.clone());
        },
        None => {
            st.auth.status = auth_status::UNKNOWN.into();
            st.auth.message = None;
        },
    }
}

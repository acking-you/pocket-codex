//! Sending, steering, interrupting and answering requests.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use pocket_codex_host_svc::opencode::{ModelRef, PermissionReply, PromptRequest};

use super::{conn, events, lock, mapping, ops::block, Pending};
use crate::engine::app_session::ThreadRuntimeConfig;

fn files_of(images: &[String]) -> Vec<(String, Option<String>)> {
    images
        .iter()
        .enumerate()
        .map(|(i, uri)| {
            let ext = uri
                .strip_prefix("data:image/")
                .and_then(|rest| rest.split(';').next())
                .unwrap_or("png");
            (uri.clone(), Some(format!("image-{}.{ext}", i + 1)))
        })
        .collect()
}

/// Apply a model / variant / plan change before prompting. Custom (non
/// build/plan) agents are left as they are.
fn apply_settings(
    service_key: &str,
    thread_id: &str,
    model: Option<&str>,
    collaboration_mode: Option<&str>,
    effort: Option<&str>,
) -> Result<()> {
    let (client, _, _) = conn(service_key)?;
    let session = block(client.session(thread_id))?;
    let wanted = match model {
        Some(m) => ModelRef::parse(m, effort.map(str::to_string)),
        None => session.model.clone().map(|mut m| {
            if let Some(effort) = effort {
                m.variant = Some(effort.to_string());
            }
            m
        }),
    };
    let model = match wanted {
        Some(wanted) if Some(&wanted) != session.model.as_ref() => {
            block(client.set_model(thread_id, &wanted))?;
            Some(wanted)
        },
        other => other.or_else(|| session.model.clone()),
    };
    let current = session.agent.as_deref();
    let agent = match collaboration_mode {
        Some("plan") if current != Some("plan") => Some("plan"),
        Some("default") if current == Some("plan") => Some("build"),
        _ => None,
    };
    if let Some(agent) = agent {
        block(client.set_agent(thread_id, agent))?;
    }
    let plan = agent.or(current) == Some("plan");
    let (_, shared, _) = conn(service_key)?;
    lock(&shared)
        .config
        .insert(thread_id.to_string(), ThreadRuntimeConfig {
            model: model.as_ref().map(ModelRef::qualified),
            model_provider: model.as_ref().map(|m| m.provider_id.clone()),
            reasoning_effort: model.as_ref().and_then(|m| m.variant.clone()),
            collaboration_mode: Some(if plan { "plan" } else { "default" }.to_string()),
            confirmed_by_update: true,
            ..ThreadRuntimeConfig::default()
        });
    Ok(())
}

/// Send a user message; it starts a turn when idle and queues otherwise.
/// `model` is `providerID/id`, `reasoning_effort` the model variant, and
/// `collaboration_mode` (`plan` / `default`) selects the plan or build agent.
pub fn turn_start(
    service_key: &str,
    thread_id: &str,
    text: String,
    images: Vec<String>,
    model: Option<String>,
    collaboration_mode: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    let effort = reasoning_effort.as_deref().filter(|e| !e.is_empty());
    let model = model.as_deref().filter(|m| !m.is_empty());
    apply_settings(service_key, thread_id, model, collaboration_mode.as_deref(), effort)?;
    let (client, shared, _) = conn(service_key)?;
    let request = PromptRequest {
        text,
        files: files_of(&images),
        steer: false,
    };
    let accepted = block(client.prompt(thread_id, &request))?;
    let mut s = lock(&shared);
    // A queued message opens its own turn later; only an idle session's turn
    // id is known now.
    if !s.active.contains(thread_id) {
        s.translator
            .turns
            .insert(thread_id.to_string(), accepted.id);
    }
    if let Some(window) = s.windows.get_mut(thread_id) {
        window.read = false;
    }
    Ok(())
}

/// Deliver a message into the running turn and return that turn's id.
pub fn turn_steer(
    service_key: &str,
    thread_id: &str,
    turn_id: Option<&str>,
    text: &str,
    images: &[String],
) -> Result<String> {
    let (client, shared, _) = conn(service_key)?;
    let expected = turn_id
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| super::ops::running_turn(&lock(&shared), thread_id))
        .ok_or_else(|| anyhow!("the active turn is not available; reload the thread"))?;
    let request = PromptRequest {
        text: text.to_string(),
        files: files_of(images),
        steer: true,
    };
    block(client.prompt(thread_id, &request))?;
    Ok(expected)
}

/// Interrupt the session's running execution. OpenCode needs no turn id.
pub fn turn_interrupt(service_key: &str, thread_id: &str, _turn_id: Option<String>) -> Result<()> {
    let (client, _, _) = conn(service_key)?;
    block(client.interrupt(thread_id))?;
    Ok(())
}

fn permission_reply(decision: &str) -> PermissionReply {
    match decision {
        "accept" => PermissionReply::Once,
        "acceptForSession" => PermissionReply::Always,
        _ => PermissionReply::Reject,
    }
}

fn take_pending(service_key: &str, request_id: &str) -> Result<Pending> {
    let (_, shared, _) = conn(service_key)?;
    let pending = lock(&shared).pending.get(request_id).cloned();
    pending.ok_or_else(|| anyhow!("this request is no longer pending; reopen the session"))
}

/// Drop an answered request and tell the UI to remove its card.
fn settle(service_key: &str, session: &str, request_id: &str) -> Result<()> {
    let (_, shared, tx) = conn(service_key)?;
    lock(&shared).pending.remove(request_id);
    let _ = tx.send(events::resolved(session, request_id));
    Ok(())
}

/// Answer a permission request: `accept` allows once, `acceptForSession`
/// persists OpenCode's offered project rule, anything else rejects.
pub fn respond_approval(service_key: &str, request_id: &str, decision: &str) -> Result<()> {
    let Pending::Permission {
        session,
    } = take_pending(service_key, request_id)?
    else {
        bail!("request {request_id} is a question, not an approval");
    };
    let (client, _, _) = conn(service_key)?;
    block(client.reply_permission(&session, request_id, permission_reply(decision)))?;
    settle(service_key, &session, request_id)
}

/// Answer a form. `answers_json` maps field keys to chosen labels / free
/// text, as the question card sends them; an empty object cancels the form.
pub fn respond_user_input(service_key: &str, request_id: &str, answers_json: &str) -> Result<()> {
    let answers: HashMap<String, Vec<String>> = serde_json::from_str(answers_json)
        .map_err(|e| anyhow!("parsing user-input answers: {e}"))?;
    let Pending::Form(form) = take_pending(service_key, request_id)? else {
        bail!("request {request_id} is an approval, not a question");
    };
    let (client, _, _) = conn(service_key)?;
    if answers.values().all(Vec::is_empty) {
        block(client.cancel_form(&form.session_id, &form.id))?;
    } else {
        let answer = mapping::form_answer(&form, &answers);
        block(client.reply_form(&form.session_id, &form.id, answer))?;
    }
    settle(service_key, &form.session_id, request_id)
}

/// Rename a session.
pub fn set_thread_name(service_key: &str, thread_id: &str, name: &str) -> Result<()> {
    let (client, _, _) = conn(service_key)?;
    block(client.rename(thread_id, name))
}

/// Queue a context compaction; `thread/compacted` follows when it ends.
pub fn compact(service_key: &str, thread_id: &str) -> Result<()> {
    let (client, _, _) = conn(service_key)?;
    block(client.compact(thread_id))
}

/// Working-tree changes at `cwd` as one unified diff; empty when clean.
pub fn git_diff(service_key: &str, cwd: &str) -> Result<String> {
    let (client, _, _) = conn(service_key)?;
    Ok(join_patches(&block(client.vcs_diff(cwd))?))
}

fn join_patches(files: &[pocket_codex_host_svc::opencode::FileDiff]) -> String {
    let mut out = String::new();
    for file in files.iter().filter(|f| !f.patch.trim().is_empty()) {
        if !file.patch.starts_with("diff --git") {
            out.push_str(&format!("diff --git a/{0} b/{0}\n", file.file));
        }
        out.push_str(&file.patch);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// The runtime configuration last read or applied for a thread (no network).
pub fn thread_runtime_config(service_key: &str, thread_id: &str) -> Option<ThreadRuntimeConfig> {
    let (_, shared, _) = conn(service_key).ok()?;
    let config = lock(&shared).config.get(thread_id).cloned();
    config
}

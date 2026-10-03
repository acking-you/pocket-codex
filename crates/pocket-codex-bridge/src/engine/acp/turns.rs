//! Prompts, cancellation, answers to permission and elicitation requests, and
//! authentication (TRD §4.4.3, §4.5.3–§4.5.5).

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use pocket_codex_core::acp::{
    methods,
    pcx::{methods as pcx_methods, AuthState},
    ContentBlock, ImageContent, TextContent,
};
use serde_json::{json, Value};

use super::{
    ctx, mapping,
    ops::{options_of, set_option},
    state::PendingKind,
};
use crate::engine::{app_session::ThreadRuntimeConfig, runtime};

/// Prompt blocks of a text and `data:` URL images (other references are
/// skipped; the UI only sends data URLs).
fn prompt_blocks(text: &str, images: &[String]) -> Vec<ContentBlock> {
    let mut out = Vec::new();
    if !text.is_empty() {
        out.push(ContentBlock::Text(TextContent {
            text: text.to_string(),
            ..TextContent::default()
        }));
    }
    for url in images {
        let Some(rest) = url.strip_prefix("data:") else { continue };
        let Some((mime, data)) = rest.split_once(";base64,") else { continue };
        out.push(ContentBlock::Image(ImageContent {
            data: data.to_string(),
            mime_type: mime.to_string(),
            ..ImageContent::default()
        }));
    }
    out
}

/// Apply the composer's model / effort / plan selections, then submit
/// (§4.5.3). Returns once the hub accepted (or queued) the prompt.
pub fn turn_start(
    service_key: &str,
    thread_id: &str,
    text: String,
    images: Vec<String>,
    model: Option<String>,
    collaboration_mode: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<()> {
    let ctx = ctx(service_key)?;
    runtime::runtime().block_on(async {
        let options = options_of(&ctx, Some(thread_id));
        let current = |role: &str| {
            mapping::option_with_role(&options, role)
                .map(|o| (o.id.clone(), o.current_value.as_str().map(str::to_string)))
        };
        for (role, wanted) in [("model", &model), ("effort", &reasoning_effort)] {
            let Some(wanted) = wanted.as_ref().filter(|w| !w.is_empty()) else { continue };
            if let Some((id, value)) = current(role) {
                if value.as_deref() != Some(wanted.as_str()) {
                    set_option(&ctx, thread_id, &id, json!(wanted)).await?;
                }
            }
        }
        let mode = options.iter().find(|o| {
            mapping::config_role(o) == "mode" && o.flat_options().iter().any(|v| v.value == "plan")
        });
        match (collaboration_mode.as_deref(), mode) {
            (Some("plan"), Some(mode)) if mode.current_value.as_str() != Some("plan") => {
                let previous = mode.current_value.as_str().map(str::to_string);
                if let Some(view) = ctx.shared().sessions.get_mut(thread_id) {
                    view.pre_plan_mode = previous;
                }
                set_option(&ctx, thread_id, &mode.id, json!("plan")).await?;
            },
            (Some("default"), Some(mode)) => {
                let previous = ctx
                    .shared()
                    .sessions
                    .get_mut(thread_id)
                    .and_then(|v| v.pre_plan_mode.take());
                if let Some(previous) = previous {
                    set_option(&ctx, thread_id, &mode.id, json!(previous)).await?;
                }
            },
            _ => {},
        }
        let prompt = prompt_blocks(&text, &images);
        if prompt.is_empty() {
            bail!("nothing to send");
        }
        let key = uuid::Uuid::new_v4().to_string();
        ctx.shared()
            .unacked
            .insert(key.clone(), (thread_id.to_string(), prompt.clone()));
        let params = json!({"sessionId": thread_id, "prompt": prompt, "clientSubmissionId": key});
        match ctx.call(pcx_methods::SESSION_SUBMIT, params).await {
            Ok(_) => {
                ctx.shared().unacked.remove(&key);
                Ok(())
            },
            // The connection dropped mid-request: the reconnect re-submits
            // under the same id and the hub de-duplicates it.
            Err(_) if ctx.client().is_err() => Ok(()),
            Err(e) => {
                ctx.shared().unacked.remove(&key);
                Err(e)
            },
        }
    })
}

/// Cancel the running turn (`session/cancel`).
pub fn turn_interrupt(service_key: &str, thread_id: &str, _turn_id: Option<String>) -> Result<()> {
    let ctx = ctx(service_key)?;
    runtime::runtime().block_on(async {
        ctx.client()?
            .notify(methods::SESSION_CANCEL, json!({"sessionId": thread_id}))
            .await
    })
}

/// Answer pending `request_id` and forget it (an invalid answer comes back
/// as a new request).
fn answer(
    service_key: &str,
    request_id: &str,
    build: impl FnOnce(&PendingKind) -> Result<Value>,
) -> Result<()> {
    let ctx = ctx(service_key)?;
    let (token, response) = {
        let s = ctx.shared();
        let pending = s
            .pending
            .get(request_id)
            .ok_or_else(|| anyhow!("this request is no longer pending"))?;
        (pending.token.clone(), build(&pending.kind)?)
    };
    runtime::runtime().block_on(ctx.respond(&token, response))?;
    ctx.shared().pending.remove(request_id);
    Ok(())
}

/// Approve or decline with the option matching `decision` (§4.5.4).
pub fn respond_approval(service_key: &str, request_id: &str, decision: &str) -> Result<()> {
    answer(service_key, request_id, |kind| match kind {
        PendingKind::Permission(options) => mapping::approval_answer(options, decision)
            .ok_or_else(|| anyhow!("the agent offers no option for `{decision}`")),
        _ => bail!("this request is not a permission request"),
    })
}

/// Answer a permission request with one of its options.
pub fn respond_permission_option(
    service_key: &str,
    request_id: &str,
    option_id: &str,
) -> Result<()> {
    answer(service_key, request_id, |kind| match kind {
        PendingKind::Permission(options) if options.iter().any(|o| o.option_id == option_id) => {
            Ok(json!({"outcome": {"outcome": "selected", "optionId": option_id}}))
        },
        PendingKind::Permission(_) => bail!("unknown option `{option_id}`"),
        _ => bail!("this request is not a permission request"),
    })
}

/// Answer a form elicitation; `{}` cancels (§4.5.5).
pub fn respond_user_input(service_key: &str, request_id: &str, answers_json: &str) -> Result<()> {
    let answers: HashMap<String, Vec<String>> =
        serde_json::from_str(answers_json).context("decoding the answers")?;
    answer(service_key, request_id, |kind| match kind {
        PendingKind::Form(schema) => {
            match mapping::form_content(schema, &answers).map_err(|e| anyhow!(e))? {
                None => Ok(json!({"action": "cancel"})),
                Some(content) => Ok(json!({"action": "accept", "content": content})),
            }
        },
        _ => bail!("this request is not a form"),
    })
}

/// Answer a URL elicitation.
pub fn respond_elicitation_url(service_key: &str, request_id: &str, accept: bool) -> Result<()> {
    answer(service_key, request_id, |kind| match kind {
        PendingKind::Url => Ok(json!({"action": if accept { "accept" } else { "decline" }})),
        _ => bail!("this request is not a URL elicitation"),
    })
}

/// Cached authentication state; `None` before connect.
pub fn auth_state(service_key: &str) -> Option<AuthState> {
    let ctx = ctx(service_key).ok()?;
    let s = ctx.shared();
    s.meta.as_ref().map(|m| m.auth.clone())
}

/// Start an agent-type login on the host; returns `inProgress`.
pub fn auth_authenticate(service_key: &str, method_id: &str) -> Result<AuthState> {
    let ctx = ctx(service_key)?;
    let value = runtime::runtime()
        .block_on(ctx.call(pcx_methods::AUTH_AUTHENTICATE, json!({"methodId": method_id})))?;
    serde_json::from_value(value).context("decoding the auth state")
}

/// Model / effort / mode of a session from the cached options.
pub fn thread_runtime_config(service_key: &str, thread_id: &str) -> Option<ThreadRuntimeConfig> {
    let ctx = ctx(service_key).ok()?;
    let options = options_of(&ctx, Some(thread_id));
    (!options.is_empty()).then(|| mapping::runtime_config(&options))
}

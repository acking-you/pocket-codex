//! Session listing, creation and config options (TRD §4.4.3, §4.5.3).

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use pocket_codex_core::acp::{
    methods,
    pcx::{methods as pcx_methods, HubDefaultsResult, RunningResult},
    AvailableCommand, ConfigOption, ListSessionsResponse, SetConfigOptionRequest,
    SetConfigOptionResponse,
};
use serde_json::{json, Value};

use super::{ctx, history, mapping, Ctx};
use crate::engine::{
    app_events::event,
    app_session::{ModelInfo, ThreadMeta},
    meta, runtime,
};

/// Sessions listed at most.
const MAX_SESSIONS: usize = 500;
/// `session/list` pages read at most.
const MAX_PAGES: usize = 20;

/// Every session the hub lists (newest first, at most 500).
pub fn thread_list(service_key: &str) -> Result<Vec<ThreadMeta>> {
    let ctx = ctx(service_key)?;
    runtime::runtime().block_on(async {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut params = json!({});
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }
            let page: ListSessionsResponse =
                serde_json::from_value(ctx.call(methods::SESSION_LIST, params).await?)
                    .context("decoding session/list")?;
            out.extend(page.sessions.iter().map(mapping::thread_meta));
            cursor = page.next_cursor;
            if cursor.is_none() || out.len() >= MAX_SESSIONS {
                break;
            }
        }
        out.truncate(MAX_SESSIONS);
        Ok(out)
    })
}

/// Sessions with a running turn or queued prompts.
pub fn running_sessions(service_key: &str) -> Result<Vec<String>> {
    let ctx = ctx(service_key)?;
    let value = runtime::runtime().block_on(ctx.call(pcx_methods::SESSIONS_RUNNING, json!({})))?;
    let running: RunningResult =
        serde_json::from_value(value).context("decoding sessions/running")?;
    Ok(running
        .sessions
        .into_iter()
        .filter(|s| s.running || s.queue > 0)
        .map(|s| s.session_id)
        .collect())
}

/// Options of `thread_id`, falling back to the hub defaults.
pub(super) fn options_of(ctx: &Ctx, thread_id: Option<&str>) -> Vec<ConfigOption> {
    let s = ctx.shared();
    let own = thread_id
        .and_then(|t| s.sessions.get(t))
        .map(|v| v.config_options.clone());
    match own.filter(|o| !o.is_empty()) {
        Some(options) => options,
        None => {
            let defaults = s
                .meta
                .as_ref()
                .map(|m| m.default_config_options.clone())
                .unwrap_or_default();
            if defaults.is_empty() {
                s.sessions
                    .values()
                    .find(|v| !v.config_options.is_empty())
                    .map(|v| v.config_options.clone())
                    .unwrap_or_default()
            } else {
                defaults
            }
        },
    }
}

/// Models offered by the agent's model option. With no session options
/// known yet (a new conversation before its first prompt), ask the hub.
pub fn model_list(service_key: &str) -> Result<Vec<ModelInfo>> {
    let ctx = ctx(service_key)?;
    let mut options = options_of(&ctx, None);
    if options.is_empty() {
        options = runtime::runtime().block_on(hub_defaults(&ctx));
    }
    Ok(mapping::model_list(&options))
}

/// `_pcx/hub/defaults`, kept in the hub metadata; empty when the hub has
/// none, is not ready, or predates the method.
async fn hub_defaults(ctx: &Ctx) -> Vec<ConfigOption> {
    let found = match ctx.call(pcx_methods::HUB_DEFAULTS, json!({})).await {
        Ok(value) => serde_json::from_value::<HubDefaultsResult>(value).unwrap_or_default(),
        Err(e) => {
            tracing::debug!(error = %format!("{e:#}"), "reading the hub's default options failed");
            return Vec::new();
        },
    };
    if !found.config_options.is_empty() {
        if let Some(meta) = ctx.shared().meta.as_mut() {
            meta.default_config_options
                .clone_from(&found.config_options);
        }
    }
    found.config_options
}

fn default_project(ctx: &Ctx) -> Option<String> {
    let config = match &ctx.meta_url {
        Some(base) => meta::project_config_at(base),
        None => meta::project_config(&ctx.service_key),
    };
    config
        .ok()
        .and_then(|c| c.default_project)
        .filter(|p| !p.trim().is_empty())
}

/// Create a session (cwd defaults to the host's default project).
pub fn thread_start(
    service_key: &str,
    model: Option<String>,
    cwd: Option<String>,
) -> Result<String> {
    let ctx = ctx(service_key)?;
    let cwd = match cwd.filter(|c| !c.trim().is_empty()) {
        Some(cwd) => cwd,
        None => default_project(&ctx)
            .ok_or_else(|| anyhow!("[acp.cwd_required] choose a project folder first"))?,
    };
    runtime::runtime().block_on(async {
        let created = ctx.call(methods::SESSION_NEW, json!({"cwd": cwd})).await?;
        let id = created["sessionId"]
            .as_str()
            .ok_or_else(|| anyhow!("session/new returned no sessionId"))?
            .to_string();
        history::attach(&ctx, &id, Some(&cwd), 0, false).await?;
        if let Some(model) = model.filter(|m| !m.is_empty()) {
            let options = options_of(&ctx, Some(&id));
            if let Some(option) = mapping::option_with_role(&options, "model") {
                if option.current_value.as_str() != Some(model.as_str()) {
                    set_option(&ctx, &id, &option.id, json!(model)).await?;
                }
            }
        }
        Ok(id)
    })
}

/// Attach an existing session (the hub re-sends its pending requests).
pub fn thread_resume(service_key: &str, thread_id: &str) -> Result<()> {
    let ctx = ctx(service_key)?;
    runtime::runtime().block_on(history::attach(&ctx, thread_id, None, history::TAIL, false))?;
    Ok(())
}

/// Config options of a session.
pub fn config_options(service_key: &str, thread_id: &str) -> Result<Vec<ConfigOption>> {
    let ctx = ctx(service_key)?;
    Ok(options_of(&ctx, Some(thread_id)))
}

/// Slash commands of a session.
pub fn slash_commands(service_key: &str, thread_id: &str) -> Result<Vec<AvailableCommand>> {
    let ctx = ctx(service_key)?;
    let s = ctx.shared();
    Ok(s.sessions
        .get(thread_id)
        .map(|v| v.commands.clone())
        .unwrap_or_default())
}

/// Set one option and record the options the agent returns.
pub(super) async fn set_option(
    ctx: &Arc<Ctx>,
    thread_id: &str,
    config_id: &str,
    value: Value,
) -> Result<()> {
    let request = SetConfigOptionRequest::new(thread_id, config_id, value);
    let response = ctx
        .call(methods::SESSION_SET_CONFIG_OPTION, serde_json::to_value(request)?)
        .await?;
    let response: SetConfigOptionResponse = serde_json::from_value(response).unwrap_or_default();
    if !response.config_options.is_empty() {
        if let Some(view) = ctx.shared().sessions.get_mut(thread_id) {
            view.config_options = response.config_options;
        }
        ctx.emit(vec![event("acp/config/updated", thread_id, json!({"threadId": thread_id}))]);
    }
    Ok(())
}

/// Set a select (`boolean == false`) or boolean option.
pub fn set_config_option(
    service_key: &str,
    thread_id: &str,
    config_id: &str,
    value: &str,
    boolean: bool,
) -> Result<()> {
    let ctx = ctx(service_key)?;
    let value =
        if boolean { Value::Bool(value == "true") } else { Value::String(value.to_string()) };
    runtime::runtime().block_on(set_option(&ctx, thread_id, config_id, value))
}

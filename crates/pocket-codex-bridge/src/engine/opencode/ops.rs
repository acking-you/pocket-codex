//! Thread, turn and request operations of the OpenCode engine.

use anyhow::{anyhow, Result};
use pocket_codex_host_svc::opencode::{Client, Error as OcError, ModelRef, SessionQuery};

use super::{conn, lock, mapping, Pending, Shared};
use crate::engine::{
    app_session::{ModelInfo, ThreadMeta},
    runtime,
};

/// Messages per history page.
pub(super) const PAGE: u32 = 60;
/// Most turn starts listed for the rail.
const MAX_STARTS_PAGES: usize = 3;
/// Most sessions listed in the sidebar.
const MAX_SESSIONS: usize = 500;

pub(super) fn oc(error: OcError) -> anyhow::Error {
    match error {
        OcError::SubmissionUnknown => anyhow!(
            "OpenCode may or may not have accepted this; refresh before sending again (submission \
             unknown)"
        ),
        other => anyhow!("{other}"),
    }
}

pub(super) fn block<T>(future: impl std::future::Future<Output = Result<T, OcError>>) -> Result<T> {
    runtime::runtime().block_on(future).map_err(oc)
}

/// Root sessions across every directory, newest first.
pub fn thread_list(service_key: &str) -> Result<Vec<ThreadMeta>> {
    let (client, shared, _) = conn(service_key)?;
    let mut out = Vec::new();
    let mut cursor = None;
    while out.len() < MAX_SESSIONS {
        let page = block(client.sessions(&SessionQuery {
            roots_only: true,
            limit: 200,
            cursor: cursor.take(),
            ..SessionQuery::default()
        }))?;
        {
            let mut s = lock(&shared);
            for session in &page.sessions {
                s.directories
                    .insert(session.id.clone(), session.location.directory.clone());
            }
        }
        out.extend(page.sessions.iter().map(mapping::session_meta));
        match page.next_cursor {
            Some(next) if !page.sessions.is_empty() => cursor = Some(next),
            _ => break,
        }
    }
    out.truncate(MAX_SESSIONS);
    Ok(out)
}

/// Ids of sessions currently executing.
pub fn running_sessions(service_key: &str) -> Result<Vec<String>> {
    let (client, shared, _) = conn(service_key)?;
    let active = block(client.active())?;
    lock(&shared).active.clone_from(&active);
    Ok(active.into_iter().collect())
}

/// Enabled models.
pub fn model_list(service_key: &str) -> Result<Vec<ModelInfo>> {
    let (client, _, _) = conn(service_key)?;
    Ok(block(client.models())?
        .iter()
        .filter_map(|m| mapping::model_info(m, None))
        .collect())
}

/// Create a session in `cwd`, optionally selecting a model. No permission
/// rules are sent: OpenCode's own configuration applies.
pub fn thread_start(
    service_key: &str,
    model: Option<String>,
    cwd: Option<String>,
) -> Result<String> {
    let (client, shared, _) = conn(service_key)?;
    let cwd = cwd
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| anyhow!("choose a project folder for the new OpenCode session"))?;
    let session = block(client.create_session(&cwd, None))?;
    if let Some(model) = model.as_deref().and_then(|m| ModelRef::parse(m, None)) {
        block(client.set_model(&session.id, &model))?;
    }
    lock(&shared).directories.insert(session.id.clone(), cwd);
    Ok(session.id)
}

/// Announce the session's pending permissions and forms (the UI clears its
/// cards when it opens a thread).
pub fn thread_resume(service_key: &str, thread_id: &str) -> Result<()> {
    let (client, shared, tx) = conn(service_key)?;
    let permissions = block(client.permissions(thread_id))?;
    let forms = block(client.forms(thread_id))?;
    let directory = lock(&shared)
        .directories
        .get(thread_id)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    {
        let mut s = lock(&shared);
        for permission in permissions {
            out.push(mapping::permission_event(&permission, &directory));
            s.pending
                .insert(permission.id.clone(), Pending::Permission {
                    session: permission.session_id.clone(),
                });
        }
        for form in forms.into_iter().filter(|f| f.session_id != "global") {
            out.push(mapping::form_event(&form));
            s.pending.insert(form.id.clone(), Pending::Form(form));
        }
    }
    for event in out {
        let _ = tx.send(event);
    }
    Ok(())
}

pub(super) async fn turn_starts(
    client: &Client,
    thread_id: &str,
) -> Result<(Vec<mapping::TurnStart>, bool), OcError> {
    let mut starts = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_STARTS_PAGES {
        let page = client
            .messages(thread_id, 200, cursor.as_deref(), Some("user"))
            .await?;
        let mut older: Vec<mapping::TurnStart> = page
            .messages
            .iter()
            .filter_map(mapping::TurnStart::from_message)
            .collect();
        older.append(&mut starts);
        starts = older;
        match page.older_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok((starts, true)),
        }
    }
    Ok((starts, false))
}

pub(super) fn running_turn(s: &Shared, thread_id: &str) -> Option<String> {
    s.active
        .contains(thread_id)
        .then(|| s.translator.turn_id(thread_id))
}

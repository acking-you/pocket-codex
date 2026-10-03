//! The config options a new session starts from (`_pcx/hub/defaults`).
//!
//! ACP agents report config options only in session responses, so the hub
//! keeps the latest ones in `defaults/<instance>.json` across restarts. When
//! nothing is known yet, one hidden probe session in `probe/` reads them;
//! the hub closes it and never lists it.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use pocket_codex_core::acp::{
    methods,
    pcx::{auth_status, HubDefaultsResult},
    ConfigOption, SessionInfo, SessionSetup,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use super::{
    error::AcpError,
    hub::{lock, AcpHub, HubState},
    install::store::{ensure_private_dir, write_private},
};

/// Probe sessions remembered (and hidden) at most.
const MAX_HIDDEN: usize = 32;
const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(30);

/// `defaults/<instance>.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultsFile {
    agent_id: String,
    #[serde(default)]
    config_options: Vec<ConfigOption>,
    /// Agent version a probe already ran for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probed_version: Option<String>,
    /// Probe sessions, never listed.
    #[serde(default)]
    hidden_sessions: Vec<String>,
}

/// Defaults bookkeeping behind the hub lock.
#[derive(Debug, Default)]
pub(super) struct Defaults {
    read: bool,
    probed_version: Option<String>,
    hidden: Vec<String>,
    written: Option<DefaultsFile>,
}

fn file_path(state_dir: &Path, instance: &str) -> PathBuf {
    state_dir.join("defaults").join(format!("{instance}.json"))
}

/// Working directory of probe sessions.
pub(super) fn probe_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("probe")
}

/// Load the saved defaults of the launched agent once (first launch).
pub(super) fn restore(st: &mut HubState, state_dir: &Path, instance: &str) {
    if std::mem::replace(&mut st.defaults.read, true) {
        return;
    }
    let Some(file) = std::fs::read(file_path(state_dir, instance))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DefaultsFile>(&bytes).ok())
    else {
        return;
    };
    if file.agent_id != st.last_spec.agent_id {
        return;
    }
    if st.default_config_options.is_empty() && !file.config_options.is_empty() {
        st.default_config_options = file.config_options.clone();
        st.caps.config_options = true;
    }
    st.defaults.probed_version = file.probed_version.clone();
    st.defaults.hidden = file.hidden_sessions.clone();
    st.defaults.written = Some(file);
}

/// Write the defaults when they changed since the last write.
pub(super) fn save(st: &mut HubState, state_dir: &Path, instance: &str) {
    if st.last_spec.agent_id.is_empty() {
        return;
    }
    let file = DefaultsFile {
        agent_id: st.last_spec.agent_id.clone(),
        config_options: st.default_config_options.clone(),
        probed_version: st.defaults.probed_version.clone(),
        hidden_sessions: st.defaults.hidden.clone(),
    };
    if st.defaults.written.as_ref() == Some(&file) {
        return;
    }
    let written = serde_json::to_vec_pretty(&file)
        .map_err(|e| AcpError::Internal(e.to_string()))
        .and_then(|bytes| write_private(&file_path(state_dir, instance), &bytes));
    match written {
        Ok(()) => st.defaults.written = Some(file),
        Err(e) => warn!("saving the ACP default config options failed: {e}"),
    }
}

/// A probe session (or one in the probe directory) that must not be listed.
pub(super) fn is_hidden(st: &HubState, info: &SessionInfo, probe_dir: &Path) -> bool {
    st.defaults.hidden.contains(&info.session_id) || Path::new(&info.cwd) == probe_dir
}

/// The answer without probing, or `None` when a probe should run.
fn known(st: &HubState) -> Result<Option<HubDefaultsResult>, AcpError> {
    st.ready_run()?;
    let answer = || {
        Some(HubDefaultsResult {
            config_options: st.default_config_options.clone(),
        })
    };
    if !st.default_config_options.is_empty()
        || !st.caps.close
        || st.auth.status == auth_status::REQUIRED
        || st.defaults.probed_version == st.last_spec.version
    {
        return Ok(answer());
    }
    Ok(None)
}

/// `_pcx/hub/defaults`: the known defaults, probing once when there are none.
pub(super) async fn defaults(hub: &Arc<AcpHub>) -> Result<HubDefaultsResult, AcpError> {
    if let Some(found) = known(&lock(&hub.state))? {
        return Ok(found);
    }
    let _probe = hub.probe.lock().await;
    if let Some(found) = known(&lock(&hub.state))? {
        return Ok(found);
    }
    probe(hub).await
}

async fn probe(hub: &Arc<AcpHub>) -> Result<HubDefaultsResult, AcpError> {
    let (peer, run_id) = {
        let st = lock(&hub.state);
        let run = st.ready_run()?;
        (run.peer.clone(), run.id)
    };
    let dir = probe_dir(&hub.options.state_dir);
    ensure_private_dir(&dir)?;
    let params = json!({ "cwd": dir.to_string_lossy(), "mcpServers": [] });
    let result = peer
        .request(methods::SESSION_NEW, params, Some(PROBE_TIMEOUT))
        .await;
    let mut st = lock(&hub.state);
    let value = match result {
        Ok(value) => value,
        Err(e) => {
            if e.is_auth_required() {
                st.auth_required(Some(e.detail()));
            }
            return Err(e);
        },
    };
    let setup: SessionSetup =
        serde_json::from_value(value).map_err(|e| AcpError::Internal(e.to_string()))?;
    if st.run.as_ref().map(|r| r.id) != Some(run_id) {
        return Err(AcpError::AgentUnavailable("the agent restarted".into()));
    }
    if let Some(id) = setup.session_id.clone() {
        st.defaults.hidden.push(id.clone());
        let excess = st.defaults.hidden.len().saturating_sub(MAX_HIDDEN);
        st.defaults.hidden.drain(..excess);
        if st.caps.close {
            let peer = peer.clone();
            tokio::spawn(async move {
                let close = json!({ "sessionId": id });
                let _ = peer
                    .request(methods::SESSION_CLOSE, close, Some(CLOSE_TIMEOUT))
                    .await;
            });
        }
    }
    st.defaults.probed_version = st.last_spec.version.clone();
    let mut changed = false;
    if st.default_config_options.is_empty() {
        if let Some(options) = setup.config_options.filter(|o| !o.is_empty()) {
            st.default_config_options = options;
            st.caps.config_options = true;
            changed = true;
        }
    }
    if setup.modes.is_some() {
        changed |= !std::mem::replace(&mut st.caps.modes, true);
    }
    save(&mut st, &hub.options.state_dir, &hub.options.instance);
    if changed {
        st.broadcast_hub_state();
    }
    Ok(HubDefaultsResult {
        config_options: st.default_config_options.clone(),
    })
}

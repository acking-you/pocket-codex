//! Force-resume a local session into the colocated codex app-server.
//!
//! Mirrors the bridge's `engine/sessions::force_resume`, but resumes over
//! **loopback** into the app-server hosted alongside this meta service (rather
//! than via a relay-resolved service key): evict every live process holding the
//! rollout open (except the protected pids), then `thread/resume`.

use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};
use pocket_codex_codex::{client::AppClient, rollout, takeover};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::sessions::Holder;

/// Outcome of a [`force_resume`], reporting exactly what the eviction + resume
/// did.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForceResumeOutcome {
    /// Holders that were successfully sent a kill.
    pub killed: Vec<Holder>,
    /// Holders the kill could not reach (already gone, denied, or respawned).
    pub survived: Vec<Holder>,
    /// Whether the rollout is still held open after the eviction attempt (the
    /// resume proceeds regardless — an accepted risk).
    pub still_held: bool,
    /// Whether the subsequent `thread/resume` succeeded.
    pub resumed: bool,
    /// The resume error message, when `resumed` is false.
    pub resume_error: Option<String>,
}

/// PIDs a takeover must never terminate: this process and whatever actually
/// serves the colocated app-server's listen port (the server we resume into).
/// Resolving the live listener — rather than trusting a recorded pid — means a
/// takeover can never kill the server it is about to resume into, even when
/// `codex` is an npm/node shim whose recorded pid is not the real listener.
pub fn protected_pids(app_ws_addr: SocketAddr) -> Vec<u32> {
    let mut pids = vec![std::process::id()];
    let listen_url = format!("ws://{app_ws_addr}");
    if let Some(pid) = pocket_codex_core::process::find_codex_app_server(&listen_url) {
        pids.push(pid);
    }
    pids
}

/// Evict every live holder of `thread_id`'s rollout (except the protected pids),
/// then `thread/resume` it into the app-server at `app_ws_addr` over loopback.
///
/// Re-checks liveness against the freshest state right before acting and refuses
/// to resume a rollout whose turn is running right now (that would make two
/// writers append to one file). The caller is responsible for gating on user
/// confirmation.
pub async fn force_resume(app_ws_addr: SocketAddr, thread_id: &str) -> Result<ForceResumeOutcome> {
    let tid = thread_id.to_string();
    // Inspect + evict touch the filesystem and process table (force_release
    // sends kills), so run them off the async runtime.
    let report = tokio::task::spawn_blocking(move || -> Result<takeover::ReleaseReport> {
        let path = rollout::rollout_path_for_thread(&tid)
            .map_err(|e| anyhow!("locating rollout: {e}"))?
            .ok_or_else(|| anyhow!("no rollout found for thread {tid}"))?;
        let live = takeover::inspect(&path).map_err(|e| anyhow!("inspecting rollout: {e}"))?;
        if matches!(live.safety, takeover::ResumeSafety::OwnedRunning) {
            return Err(anyhow!(
                "session is running in another client right now; wait for its turn to finish \
                 before resuming"
            ));
        }
        let protected = protected_pids(app_ws_addr);
        Ok(takeover::force_release(&path, &protected))
    })
    .await
    .context("force-release task panicked")??;

    let (resumed, resume_error) = match resume_into(app_ws_addr, thread_id).await {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{e:#}"))),
    };

    Ok(ForceResumeOutcome {
        killed: report.killed.into_iter().map(Holder::from).collect(),
        survived: report.survived.into_iter().map(Holder::from).collect(),
        still_held: report.still_held,
        resumed,
        resume_error,
    })
}

/// Open a fresh loopback app-server client, `initialize` (unlocking the
/// experimental API the rest of the app relies on), then `thread/resume`.
async fn resume_into(app_ws_addr: SocketAddr, thread_id: &str) -> Result<()> {
    let ws_url = format!("ws://{app_ws_addr}");
    let (client, _events) = AppClient::connect(&ws_url)
        .await
        .context("connecting colocated app-server")?;
    client
        .request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "pocket-codex-host-svc",
                    "title": "Pocket-Codex",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": { "experimentalApi": true },
            }),
        )
        .await
        .context("app-server initialize")?;
    client
        .request("thread/resume", json!({ "threadId": thread_id }))
        .await
        .context("thread/resume")?;
    Ok(())
}

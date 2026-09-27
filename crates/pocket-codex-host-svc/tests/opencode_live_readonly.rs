//! Opt-in, read-only verification against the user's already running OpenCode
//! service.
//!
//! Run with `PCX_LIVE_OPENCODE=1 cargo test -p pocket-codex-host-svc
//! --test opencode_live_readonly --ignored -- --nocapture`. The test never
//! creates a session, submits a prompt, answers a request, interrupts a turn,
//! or manages the upstream process.

use std::{collections::BTreeSet, env, time::Duration};

use futures::StreamExt;
use pocket_codex_host_svc::opencode::{
    connection::{Connection, EventStream},
    discovery::discover,
    OpenCodeGateway,
};

const DEFAULT_DIRECTORY: &str = "/Users/wangdejiang6/Downloads/pocket-iteration";

#[tokio::test]
#[ignore = "requires explicit opt-in and an existing local OpenCode service"]
async fn live_opencode_attached_gateway_is_read_only_and_preserves_identity() -> anyhow::Result<()>
{
    if env::var("PCX_LIVE_OPENCODE").as_deref() != Ok("1") {
        return Ok(());
    }
    let directory =
        env::var("PCX_OPENCODE_DIRECTORY").unwrap_or_else(|_| DEFAULT_DIRECTORY.to_owned());

    let native = discover(&directory).await?;
    let before = native.connect().await?;
    let direct = Connection::from(native.clone());

    let direct_sessions = direct.sessions(None).await?;
    if env::var("PCX_REQUIRE_LIVE_SESSION").as_deref() == Ok("1") {
        anyhow::ensure!(
            !direct_sessions.is_empty(),
            "the selected OpenCode directory has no sessions"
        );
    }
    let direct_ids: BTreeSet<_> = direct_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();

    let mut selected = None;
    for session in &direct_sessions {
        let page = direct.history(&session.id, 20, None).await?;
        if !page.messages.is_empty() {
            selected = Some((session.id.clone(), page));
            break;
        }
    }
    if env::var("PCX_REQUIRE_LIVE_SESSION").as_deref() == Ok("1") {
        anyhow::ensure!(selected.is_some(), "the selected OpenCode directory has no history");
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = listener.local_addr()?;
    let gateway_handle = OpenCodeGateway::new(direct.clone()).serve(listener)?;
    let gateway_origin = format!("http://{gateway_addr}");
    let gateway = Connection::connect(&gateway_origin, &directory, None).await?;

    let gateway_sessions = gateway.sessions(None).await?;
    let gateway_ids: BTreeSet<_> = gateway_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();
    anyhow::ensure!(
        direct_ids == gateway_ids,
        "gateway session identities differ from direct read"
    );

    if let Some((session_id, direct_history)) = selected {
        let direct_session = direct.session(&session_id).await?;
        let gateway_session = gateway.session(&session_id).await?;
        anyhow::ensure!(
            serde_json::to_value(&direct_session)? == serde_json::to_value(&gateway_session)?,
            "gateway session metadata differs from direct read"
        );

        let gateway_history = gateway.history(&session_id, 20, None).await?;
        anyhow::ensure!(
            serde_json::to_value(&direct_history)? == serde_json::to_value(&gateway_history)?,
            "gateway history differs from direct read"
        );
    }

    let direct_status = direct.status().await?;
    let gateway_status = gateway.status().await?;
    anyhow::ensure!(
        direct_status.len() == gateway_status.len(),
        "gateway status count differs from direct read"
    );
    let direct_permissions = direct.permissions().await?;
    let gateway_permissions = gateway.permissions().await?;
    anyhow::ensure!(
        serde_json::to_value(&direct_permissions)? == serde_json::to_value(&gateway_permissions)?,
        "gateway pending permissions differ from direct read"
    );
    let direct_questions = direct.questions().await?;
    let gateway_questions = gateway.questions().await?;
    anyhow::ensure!(
        serde_json::to_value(&direct_questions)? == serde_json::to_value(&gateway_questions)?,
        "gateway pending forms differ from direct read"
    );

    let direct_sse = native_events(&direct).await?;
    let gateway_sse = native_events(&gateway).await?;
    let _direct_events = observe_idle_sse(direct_sse).await?;
    let _gateway_events = observe_idle_sse(gateway_sse).await?;

    gateway_handle.stop().await;
    drop(gateway);
    let after = native.connect().await?;
    anyhow::ensure!(
        (before.version, before.pid) == (after.version, after.pid),
        "stopping the gateway changed upstream identity"
    );
    Ok(())
}

async fn native_events(connection: &Connection) -> anyhow::Result<EventStream> {
    Ok(connection.events().await?)
}

async fn observe_idle_sse(mut stream: EventStream) -> anyhow::Result<usize> {
    match tokio::time::timeout(Duration::from_secs(3), stream.next()).await {
        Ok(Some(Ok(_event))) => Ok(1),
        Ok(Some(Err(_error))) => {
            anyhow::bail!("OpenCode SSE returned a protocol or transport error")
        },
        Ok(None) | Err(_) => Ok(0),
    }
}

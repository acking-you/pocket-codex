//! Opt-in run against a real installed ACP agent (TRD §8.3), through the
//! same engine functions the App calls. Never runs unless asked:
//!
//! `PCX_ACP_LIVE=<catalog id> cargo test -p pocket_codex_bridge acp_live --
//! --nocapture --test-threads=1`
//!
//! Read-only by default: initialize, list, open the newest session (load or
//! resume), page back, read a turn and a `/history/v1` window. With
//! `PCX_ACP_LIVE_WRITE=1` it also creates one session under
//! `$TMPDIR/pocket-acp-e2e` and sends one short prompt.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use pocket_codex_core::history_sync::WindowQuery;
use pocket_codex_host_svc::{
    acp::{install, serve_ws, AcpHub, HubOptions, ProcessConnector},
    store::{ConfigStore, HostStore},
};
use reqwest::Url;
use tokio::{net::TcpListener, sync::broadcast};

use super::{connect_url, disconnect, subscribe_events};
use crate::engine::{acp_manage, app_session::AppEvent, runtime};

fn note(line: &str) {
    eprintln!("[acp-e2e] {line}");
}

fn wait_completed(rx: &mut broadcast::Receiver<AppEvent>, limit: Duration) -> Option<AppEvent> {
    let deadline = Instant::now() + limit;
    runtime::runtime().block_on(async {
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Ok(e)) if e.kind == "turn/completed" => return Some(e),
                Ok(Ok(e)) => note(&format!(
                    "  event {} {}",
                    e.kind,
                    e.text
                        .unwrap_or_default()
                        .chars()
                        .take(60)
                        .collect::<String>()
                )),
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {},
                _ => return None,
            }
        }
    })
}

#[test]
fn acp_live_end_to_end() {
    let Ok(agent) = std::env::var("PCX_ACP_LIVE") else {
        return;
    };
    runtime::init(std::env::temp_dir()).expect("runtime");
    let rt = runtime::runtime();
    let ctx = acp_manage::install_context().expect("install context");
    let spec = install::resolve_launch(&agent, &ctx).expect("installed agent");
    note(&format!("1 launch {} {:?}", spec.display_name, spec.version));
    let scratch = tempfile::tempdir().expect("scratch");
    let launch_id = agent.clone();
    let launch_ctx = ctx.clone();
    let options = HubOptions {
        instance: format!("live-{agent}"),
        launch: Arc::new(move || {
            let spec = install::resolve_launch(&launch_id, &launch_ctx)?;
            Ok((spec, install::gateway_auth(&launch_ctx, &launch_id)))
        }),
        state_dir: scratch.path().join("acp"),
        log_file: Some(scratch.path().join("agent.log")),
        connector: Arc::new(ProcessConnector),
        terminal: None,
    };
    let hub = rt.block_on(AcpHub::start(options)).expect("hub starts");
    let info = hub.info();
    note(&format!("2 ready caps={:?} auth={}", info.caps, info.auth.status));
    let ws = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
    let addr = ws.local_addr().expect("addr");
    rt.spawn(serve_ws(ws, hub.clone()));
    let store = Arc::new(
        rt.block_on(ConfigStore::open(scratch.path().join("threads.json")))
            .expect("store"),
    );
    let host = Arc::new(
        rt.block_on(HostStore::open(scratch.path().join("host.json")))
            .expect("host"),
    );
    let meta = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind meta");
    let meta_addr = meta.local_addr().expect("meta addr");
    rt.spawn(pocket_codex_host_svc::acp::serve_meta(
        meta,
        store,
        host,
        scratch.path().join("uploads"),
        hub.clone(),
    ));
    let key = format!("pcx:live:acp:{agent}");
    let meta_url = Url::parse(&format!("http://{meta_addr}")).ok();
    connect_url(&key, &format!("ws://{addr}/acp"), meta_url).expect("connect");
    let threads = super::thread_list(&key).expect("list");
    note(&format!("3 listed {} sessions", threads.len()));
    if let Some(newest) = threads.first() {
        let history = super::thread_read(&key, &newest.id).expect("read");
        note(&format!(
            "4 opened {} items={} turns={} hasOlder={}",
            newest.id,
            history.items.len(),
            history.turns.len(),
            history.has_older
        ));
        if history.has_older {
            let older = super::thread_older_page(&key, &newest.id).expect("older");
            note(&format!("5 older page {} items", older.items.len()));
        }
        if let Some(turn) = history.turns.first() {
            let page = super::thread_turn_page(&key, &newest.id, &turn.turn_id, false, false)
                .expect("turn");
            note(&format!("6 turn {} {} items", turn.turn_id, page.items.len()));
        }
        let query = WindowQuery {
            session: newest.id.clone(),
            collection: "items".into(),
            group: None,
            cursor: None,
            limit: 20,
            projection: None,
        };
        let window = rt
            .block_on(hub.history_window(&query))
            .expect("history window");
        note(&format!("7 history window {} documents", window.order.len()));
    }
    if std::env::var("PCX_ACP_LIVE_WRITE").as_deref() == Ok("1") {
        let dir = std::env::temp_dir().join("pocket-acp-e2e");
        std::fs::create_dir_all(&dir).expect("e2e dir");
        let session = super::thread_start(&key, None, Some(dir.to_string_lossy().into_owned()))
            .expect("start");
        let mut rx = subscribe_events(&key).expect("subscribe");
        super::turn_start(
            &key,
            &session,
            "Reply with the single word: ok".into(),
            Vec::new(),
            None,
            None,
            None,
        )
        .expect("prompt");
        let done = wait_completed(&mut rx, Duration::from_secs(180)).expect("turn completes");
        note(&format!("8 wrote session {session}: {}", done.raw));
    }
    disconnect(&key);
    rt.block_on(hub.shutdown(Duration::from_secs(5)));
    note("done");
}

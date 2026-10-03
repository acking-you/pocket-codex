//! Opt-in live check that Codex (`default`) and OpenCode (`opencode`) can be
//! hosted side by side and each opened; no model turn is sent.
//!
//! `PCX_OPENCODE_LIVE_SUPPORT=<App support dir> PCX_CODEX_BINARY=<codex path>
//!  cargo test -p pocket_codex_bridge opencode_live_dual -- --nocapture`

use super::live_tests::note;
use crate::engine::{app_session, runtime, serve, transport};

#[test]
fn opencode_live_dual() {
    let (Some(support), Some(codex)) =
        (std::env::var_os("PCX_OPENCODE_LIVE_SUPPORT"), std::env::var("PCX_CODEX_BINARY").ok())
    else {
        return;
    };
    runtime::init(support.into()).expect("runtime");
    let oc =
        crate::engine::serve_opencode::start(Some("opencode".into()), None).expect("host OpenCode");
    let cx = serve::serve_start(0, Some(codex), Some("default".into()), None, false)
        .expect("host Codex");
    let clash = crate::engine::serve_opencode::start(Some("default".into()), None);
    note(&format!(
        "9 OpenCode with the Codex name refused: {:?}",
        clash.err().map(|e| e.to_string())
    ));
    let rows = serve::serve_status();
    let providers: Vec<_> = rows
        .iter()
        .map(|r| (r.provider.clone(), r.name.clone(), r.alive))
        .collect();
    note(&format!("9 hosted={providers:?}"));
    let transport = transport::resolve_blocking().expect("transport");
    super::connect(oc.service_key.clone(), 0, &transport).expect("connect OpenCode");
    app_session::connect(cx.app_service_key.clone(), 0, &transport).expect("connect Codex");
    let oc_threads = super::thread_list(&oc.service_key)
        .expect("OpenCode sessions")
        .len();
    let cx_threads = app_session::thread_list(&cx.app_service_key)
        .expect("Codex threads")
        .len();
    note(&format!("9/10 OpenCode sessions={oc_threads} Codex threads={cx_threads}"));
    super::disconnect(&oc.service_key);
    app_session::disconnect(&cx.app_service_key);
    serve::serve_stop("default").expect("stop Codex");
    serve::serve_stop("opencode").expect("stop OpenCode");
    assert!(
        rows.iter().any(|r| r.provider == "codex") && rows.iter().any(|r| r.provider == "opencode")
    );
}

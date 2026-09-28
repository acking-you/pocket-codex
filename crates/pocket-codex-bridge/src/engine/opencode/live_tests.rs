//! Opt-in end-to-end run against the user's real OpenCode service, relay and
//! account, through the same engine functions the App calls.
//!
//! `PCX_OPENCODE_LIVE_SUPPORT=<App support dir> cargo test -p
//! pocket_codex_bridge  opencode_live -- --nocapture --test-threads=1`
//!
//! Sessions are created only under `$TMPDIR/pocket-opencode-e2e`; at most five
//! short prompts are sent.

use std::time::{Duration, Instant};

use tokio::sync::broadcast::Receiver;

use crate::engine::{app_session::AppEvent, runtime, serve, session_sync, transport};

pub(super) fn note(line: &str) {
    eprintln!("[opencode-e2e] {line}");
}

/// Wait (blocking) for an event matching `hit`, up to `limit`.
pub(super) fn wait(
    rx: &mut Receiver<AppEvent>,
    limit: Duration,
    mut hit: impl FnMut(&AppEvent) -> bool,
) -> Option<AppEvent> {
    let deadline = Instant::now() + limit;
    runtime::runtime().block_on(async {
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Ok(ev)) if hit(&ev) => return Some(ev),
                Ok(Ok(_)) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {},
                _ => return None,
            }
        }
    })
}

fn settle(key: &str, tid: &str) {
    for _ in 0..120 {
        if super::thread_read(key, tid)
            .map(|h| !h.running)
            .unwrap_or(true)
        {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn opencode_live_end_to_end() {
    let Some(support) = std::env::var_os("PCX_OPENCODE_LIVE_SUPPORT") else {
        return;
    };
    runtime::init(support.into()).expect("runtime");
    let hosted =
        crate::engine::serve_opencode::start(Some("opencode".into()), None).expect("host OpenCode");
    note(&format!(
        "1 host {} v{} verified={} startedService={}",
        hosted.service_key, hosted.version, hosted.verified, hosted.started_service
    ));
    assert!(hosted.verified);
    let mut registered = false;
    for _ in 0..60 {
        registered = serve::serve_status().iter().any(|s| {
            s.provider == "opencode"
                && s.name == "opencode"
                && s.app_registered
                && s.meta_registered
        });
        if registered {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    note(&format!("1 opencode+meta registered={registered}"));
    assert!(registered);

    let clash = serve::serve_start(0, None, Some("opencode".into()), None, false);
    note(&format!(
        "9 codex with the OpenCode name refused: {:?}",
        clash.as_ref().err().map(|e| e.to_string())
    ));
    assert!(clash.is_err());

    let key = hosted.service_key.clone();
    let transport = transport::resolve_blocking().expect("transport");
    // A remote controller path: probe through the relay, not loopback.
    let relay_probe = relay_probe(&key, &transport);
    note(&format!("3 relay probe reachable={} ({relay_probe:?})", relay_probe.is_none()));
    super::connect(key.clone(), 0, &transport).expect("connect");
    let threads = super::thread_list(&key).expect("list");
    let dirs: std::collections::BTreeSet<_> = threads.iter().map(|t| t.cwd.clone()).collect();
    note(&format!("3 sessions={} directories={}", threads.len(), dirs.len()));
    assert!(!threads.is_empty() && threads.iter().all(|t| !t.cwd.is_empty()));
    let first = &threads[0];
    let history = super::thread_read(&key, &first.id).expect("read");
    let older = if history.has_older {
        super::thread_older_page(&key, &first.id)
            .expect("older")
            .items
            .len()
    } else {
        0
    };
    note(&format!(
        "3 read items={} turns={} hasOlder={} olderItems={older}",
        history.items.len(),
        history.turns.len(),
        history.has_older
    ));
    assert!(!history.items.is_empty());
    let tid = live_turns(&key);
    stop_and_check(&key, &tid);
}

/// Handshake through a transient relay tunnel, as a remote controller would.
fn relay_probe(key: &str, transport: &transport::Transport) -> Option<String> {
    let (addr, handle) = match runtime::subscribe_transient(key.to_string(), 0, transport) {
        Ok(v) => v,
        Err(e) => return Some(format!("{e:#}")),
    };
    let client = pocket_codex_host_svc::opencode::Client::new(&format!("http://{addr}/"), None);
    let result = match client {
        Ok(client) => runtime::runtime()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(20), client.connect()).await
            })
            .map_err(|_| "timeout".to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
            .err(),
        Err(e) => Some(e.to_string()),
    };
    handle.abort();
    result
}

fn is_done(e: &AppEvent, tid: &str) -> bool {
    e.thread_id.as_deref() == Some(tid) && e.kind == "turn/completed"
}

fn live_turns(key: &str) -> String {
    let e2e = std::env::temp_dir().join("pocket-opencode-e2e");
    std::fs::create_dir_all(&e2e).expect("e2e dir");
    let cwd = e2e.display().to_string();
    let mut rx = super::subscribe_events(key).expect("events");
    let tid = super::thread_start(key, None, Some(cwd.clone())).expect("new session");
    note(&format!("4 created {tid} in {cwd}"));
    super::turn_start(key, &tid, "用一句话自我介绍".into(), vec![], None, None, None)
        .expect("prompt 1");
    let mut deltas = 0;
    let end = wait(&mut rx, Duration::from_secs(180), |e| {
        if e.thread_id.as_deref() == Some(tid.as_str()) && e.kind == "item/agentMessage/delta" {
            deltas += 1;
        }
        is_done(e, &tid)
    });
    let read = super::thread_read(key, &tid).expect("read 1");
    let reply: Vec<_> = read
        .items
        .iter()
        .filter(|i| i.item_type == "agentMessage")
        .map(|i| i.text.clone())
        .collect();
    note(&format!(
        "4 deltas={deltas} end={:?} running={} reply={:?}",
        end.map(|e| e.raw),
        read.running,
        reply.join(" ")
    ));
    assert!(deltas > 0 && !reply.is_empty() && !read.running);
    steer_queue_interrupt(key, &tid, &mut rx);
    permission(key, &tid, &cwd, &mut rx);
    tid
}

fn steer_queue_interrupt(key: &str, tid: &str, rx: &mut Receiver<AppEvent>) {
    super::turn_start(
        key,
        tid,
        "从1数到300，每行一个数字，不要省略。".into(),
        vec![],
        None,
        None,
        None,
    )
    .expect("prompt 2");
    let streaming = wait(rx, Duration::from_secs(120), |e| {
        e.thread_id.as_deref() == Some(tid) && e.kind == "item/agentMessage/delta"
    });
    assert!(streaming.is_some(), "the long turn never streamed");
    let steered = super::turn_steer(key, tid, None, "数完后再说一句：完毕", &[]);
    note(&format!("5 steer accepted={:?}", steered.as_ref().map_err(|e| e.to_string())));
    let queued =
        super::turn_start(key, tid, "用一句话说你刚才做了什么".into(), vec![], None, None, None);
    note(&format!("5 queued accepted={:?}", queued.as_ref().map_err(|e| e.to_string())));
    super::turn_interrupt(key, tid, None).expect("interrupt");
    let end = wait(rx, Duration::from_secs(60), |e| is_done(e, tid));
    let raw = end.map(|e| e.raw).unwrap_or_default();
    note(&format!("7 after interrupt: {raw}"));
    assert!(steered.is_ok() && queued.is_ok());
    assert!(raw.contains("interrupted"));
    settle(key, tid);
}

fn permission(key: &str, tid: &str, cwd: &str, rx: &mut Receiver<AppEvent>) {
    let prompt = format!("请用 shell 工具运行命令 `ls {cwd}` 并告诉我结果。");
    super::turn_start(key, tid, prompt, vec![], None, None, None).expect("prompt 5");
    let asked = wait(rx, Duration::from_secs(120), |e| {
        e.thread_id.as_deref() == Some(tid) && e.request_id.is_some()
    });
    match asked {
        None => note("6 no permission request (OpenCode allows this shell command)"),
        Some(request) => {
            let id = request.request_id.clone().unwrap_or_default();
            note(&format!("6 approval kind={} raw={}", request.kind, request.raw));
            super::respond_approval(key, &id, "decline").expect("decline");
            let resolved = wait(rx, Duration::from_secs(30), |e| {
                e.kind == "serverRequest/resolved" && e.raw.contains(&id)
            });
            note(&format!("6 declined; card resolved={}", resolved.is_some()));
        },
    }
    settle(key, tid);
    let read = super::thread_read(key, tid).expect("read after permission");
    note(&format!("6 session running={} items={}", read.running, read.items.len()));
    session_sync::save_history(key, tid, &read);
}

fn stop_and_check(key: &str, tid: &str) {
    super::disconnect(key);
    serve::serve_stop("opencode").expect("stop hosting");
    let hosted = serve::serve_status()
        .iter()
        .any(|s| s.provider == "opencode");
    let cached = session_sync::cached_history(key, tid).expect("cache");
    let home = std::env::var("HOME").unwrap_or_default();
    let status = std::process::Command::new(format!("{home}/.opencode/bin/opencode"))
        .args(["service", "status"])
        .output()
        .expect("opencode service status");
    let status = String::from_utf8_lossy(&status.stdout).trim().to_string();
    note(&format!(
        "8 hosted after stop={hosted} cachedItems={:?} cachedRunning={:?} serviceStatus={status}",
        cached.as_ref().map(|c| c.items.len()),
        cached.as_ref().map(|c| c.running)
    ));
    assert!(!hosted);
    assert!(cached.is_some_and(|c| !c.running && !c.items.is_empty()));
    assert!(status.starts_with("http"));
}

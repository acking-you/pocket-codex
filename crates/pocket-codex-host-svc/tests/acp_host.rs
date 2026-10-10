//! End-to-end tests of the host-owned ACP client against the scripted mock
//! agent (`examples/mock_acp_agent.rs`), run as a real subprocess with
//! different capability profiles, plus the `/acp/v1` gateway and the
//! controller-side replica over loopback.
#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use pocket_codex_host_svc::acp::{
    api,
    fold::Turn,
    spec::{self, AgentSpec},
    state::LogEntry,
    AgentHost, ClientError, GatewayClient, HostError, HostOptions, Identity, Replica, SessionStore,
    SourceId,
};
use serde_json::{json, Value};
use tokio::sync::broadcast::{self, error::RecvError};

const WAIT: Duration = Duration::from_secs(15);

/// The example binary Cargo builds next to this test.
fn mock_binary() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable");
    let dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("target dir");
    let mock = dir.join("examples").join("mock_acp_agent");
    assert!(
        mock.is_file(),
        "{} is missing: run the whole package test (`cargo test -p pocket-codex-host-svc`), which \
         builds examples",
        mock.display()
    );
    mock
}

/// The mock behind a path with spaces, to prove paths are not split.
fn spaced_mock(root: &Path) -> PathBuf {
    let dir = root.join("agent dir with spaces");
    std::fs::create_dir_all(&dir).expect("dir");
    let link = dir.join("mock agent");
    if !link.exists() {
        std::os::unix::fs::symlink(mock_binary(), &link).expect("symlink");
    }
    link
}

fn spec_of(root: &Path, profile: &str, extra: &[&str]) -> (AgentSpec, PathBuf) {
    let program = spaced_mock(root);
    let mut args = vec!["--profile".to_string(), profile.to_string()];
    args.extend(extra.iter().map(|arg| arg.to_string()));
    let spec = AgentSpec {
        profile_id: format!("custom-{profile}"),
        display_name: format!("Mock {profile}"),
        program: program.to_string_lossy().into_owned(),
        args,
    };
    let resolved = spec::resolve_program(&spec.program, None, None, &[]).expect("explicit path");
    (spec, resolved)
}

/// The store a host with this invocation uses.
fn store_of(root: &Path, name: &str, spec: &AgentSpec, program: &Path) -> SessionStore {
    let state = root.join("state");
    let source = SourceId::derive(&state, program, &spec.args).expect("source");
    SessionStore::open(&state, name, source)
}

async fn host_named(
    root: &Path,
    name: &str,
    profile: &str,
    extra: &[&str],
) -> (AgentHost, Result<(), HostError>) {
    let (spec, program) = spec_of(root, profile, extra);
    AgentHost::start(HostOptions {
        store: store_of(root, name, &spec, &program),
        spec,
        program,
        path: None,
    })
    .await
}

async fn host(root: &Path, profile: &str, extra: &[&str]) -> (AgentHost, Result<(), HostError>) {
    host_named(root, "mock", profile, extra).await
}

struct Events {
    backlog: std::collections::VecDeque<Arc<LogEntry>>,
    live: broadcast::Receiver<Arc<LogEntry>>,
}

impl Events {
    fn from(host: &AgentHost) -> Self {
        let snapshot = host.snapshot();
        let generation = snapshot["generation"].as_u64().expect("generation");
        let seq = snapshot["seq"].as_u64().expect("seq");
        let (backlog, live) = host.subscribe(generation, seq).expect("subscribe");
        Self {
            backlog: backlog.into(),
            live,
        }
    }

    async fn until(&mut self, mut pred: impl FnMut(&Value) -> bool) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                let entry = match self.backlog.pop_front() {
                    Some(entry) => entry,
                    None => match self.live.recv().await {
                        Ok(entry) => entry,
                        // Floods outrun this observer; it only waits for
                        // events that come later.
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => panic!("event log closed"),
                    },
                };
                let body: Value = serde_json::from_str(&entry.body).expect("json");
                if pred(&body) {
                    return body;
                }
            }
        })
        .await
        .expect("expected event in time")
    }
}

fn is(kind: &'static str) -> impl FnMut(&Value) -> bool {
    move |body| body["type"] == kind
}

fn message_texts(history: &Value) -> Vec<String> {
    history["window"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|turn| turn["items"].as_array().cloned().unwrap_or_default())
        .filter(|item| item["kind"] == "agent_message")
        .map(|item| item["text"].as_str().unwrap_or("").to_string())
        .collect()
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .expect("canonical")
        .to_string_lossy()
        .into_owned()
}

async fn new_session(host: &AgentHost, cwd: &Path) -> String {
    host.new_session(&cwd.to_string_lossy(), None)
        .await
        .expect("session")["sessionId"]
        .as_str()
        .expect("id")
        .to_string()
}

fn outcomes_of(host: &AgentHost, turn: &str) -> Vec<Value> {
    host.snapshot()["recentTurns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|recent| recent["turnId"] == turn)
        .map(|recent| recent["outcome"].clone())
        .collect()
}

async fn eventually(mut check: impl FnMut() -> bool, what: &str) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

async fn serve(host: &AgentHost) -> (GatewayClient, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(api::serve(listener, host.clone()));
    (GatewayClient::new(&format!("http://{addr}/")).expect("client"), server)
}

#[tokio::test]
async fn a_core_only_agent_runs_turns_and_receives_argv_verbatim() {
    let root = tempfile::tempdir().expect("tempdir");
    let literal = ["--literal", "$HOME;*", "two words", ""];
    let (host, started) = host(root.path(), "alpha", &literal).await;
    started.expect("started");
    let info = host.info();
    assert_eq!(info["phase"]["state"], "ready");
    assert_eq!(info["capabilities"]["loadSession"], false);
    assert_eq!(info["capabilities"]["listSessions"], false);
    let mut expected = vec!["--profile".to_string(), "alpha".to_string()];
    expected.extend(literal.iter().map(|s| s.to_string()));
    assert_eq!(info["agent"]["title"], serde_json::to_string(&expected).expect("json"));
    assert_eq!(info["authRequired"], false, "listing auth methods is not auth-required");

    let opened = host
        .new_session(&root.path().to_string_lossy(), None)
        .await
        .expect("session");
    let session = opened["sessionId"].as_str().expect("id").to_string();
    let mut events = Events::from(&host);
    let turn = host
        .prompt(&session, "hello", &[], None)
        .await
        .expect("admitted");
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], turn);
    assert_eq!(ended["outcome"], json!({"kind": "stopped", "stopReason": "end_turn"}));
    let history = host.history(&session, None, None, None).expect("history");
    assert_eq!(message_texts(&history), vec!["Hi there".to_string()]);
    assert_eq!(history["session"]["usage"], json!({"used": 10, "size": 100}));
    assert_eq!(
        host.prompt(&session, "", &["data:image/png;base64,AAAA".into()], None)
            .await,
        Err(HostError::Unsupported("this agent does not accept images".into()))
    );
    assert_eq!(host.open_session("never-seen", None).await, Err(HostError::UnknownSession));
    let listed = host.list_sessions(None).await.expect("list");
    assert_eq!(listed["source"], "host");
    assert_eq!(listed["reopen"], "none");
    assert_eq!(listed["sessions"][0]["sessionId"], session);
    assert_eq!(host.session_dir(&session), opened["cwd"].as_str().map(str::to_string));
    host.stop().await;
    assert_eq!(host.info()["phase"]["state"], "stopped");
}

#[tokio::test]
async fn a_second_agent_with_optional_features_lists_loads_and_configures() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "beta", &[]).await;
    started.expect("started");
    let caps = host.info()["capabilities"].clone();
    assert_eq!(caps["loadSession"], true);
    assert_eq!(caps["listSessions"], true);
    assert_eq!(caps["resumeSession"], true);
    assert_eq!(caps["image"], true);

    let listed = host.list_sessions(None).await.expect("list");
    assert_eq!(listed["source"], "agent");
    assert_eq!(listed["reopen"], "load");
    assert_eq!(listed["sessions"][0]["sessionId"], "old-1");
    let opened = host.open_session("old-1", None).await.expect("load");
    assert_eq!(opened["mode"], "load");
    let history = host.history("old-1", None, None, None).expect("history");
    assert_eq!(
        history["turns"].as_array().expect("turns").len(),
        2,
        "replay grouped by user message"
    );
    assert_eq!(message_texts(&history), vec![
        "first answer".to_string(),
        "second answer".to_string()
    ]);
    assert_eq!(history["truncated"], false);
    assert_eq!(
        host.session_dir("old-1"),
        Some(canonical(&std::env::temp_dir())),
        "an imported directory is canonical"
    );

    assert_eq!(opened["configOptions"][0]["currentValue"], "fast-1");
    assert!(matches!(
        host.set_config_option("old-1", "model", "invented", None)
            .await,
        Err(HostError::InvalidInput(_))
    ));
    let configured = host
        .set_config_option("old-1", "model", "large-2", None)
        .await
        .expect("grouped value");
    assert_eq!(configured["configOptions"][0]["currentValue"], "large-2");
    let stale = Identity {
        host_id: Some(host.host_id().to_string()),
        generation: host.snapshot()["generation"].as_u64().map(|g| g + 1),
    };
    assert_eq!(
        host.set_config_option("old-1", "model", "fast-1", Some(&stale))
            .await,
        Err(HostError::Stale),
        "a stale identity is refused before the agent sees it"
    );
    assert_eq!(host.set_mode("old-1", "code", Some(&stale)).await, Err(HostError::Stale));
    let moded = host.set_mode("old-1", "code", None).await.expect("mode");
    assert_eq!(moded["modes"]["currentModeId"], "code");
    assert!(host.set_mode("old-1", "invented", None).await.is_err());
    host.stop().await;
}

/// The agent names the session while its reopen is still pending, before
/// the admission is recorded. The title must be written with the admission,
/// not dropped as an update to an entry that does not exist yet.
#[tokio::test]
async fn a_title_reported_while_a_reopen_is_pending_is_persisted_with_it() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "beta", &[]).await;
    started.expect("started");
    host.list_sessions(None).await.expect("list");
    host.open_session("old-1", None).await.expect("load");
    assert_eq!(host.snapshot()["sessions"][0]["title"], "Loaded title");
    host.stop().await;
    let (spec, program) = spec_of(root.path(), "beta", &[]);
    let entry = store_of(root.path(), "mock", &spec, &program)
        .lookup("old-1")
        .expect("the admission was recorded");
    assert_eq!(entry.title.as_deref(), Some("Loaded title"));
}

/// A stored title, then a reopen during which the agent explicitly clears
/// it (`title: null`) before answering: the clear is persisted with the
/// admission instead of the old title coming back. (That an admission
/// without any title signal keeps the stored title is covered by the
/// store's own test.)
#[tokio::test]
async fn a_title_cleared_while_a_reopen_is_pending_stays_cleared() {
    let root = tempfile::tempdir().expect("tempdir");
    let extra = ["--clear-title"];
    let (spec, program) = spec_of(root.path(), "beta", &extra);
    let dir = canonical(&std::env::temp_dir());
    store_of(root.path(), "mock", &spec, &program).record(
        "old-1",
        &dir,
        Some("Old title".into()),
        None,
    );
    let (host, started) = host(root.path(), "beta", &extra).await;
    started.expect("started");
    host.open_session("old-1", None).await.expect("load");
    assert_eq!(host.snapshot()["sessions"][0]["title"], Value::Null);
    assert!(host.flush_store(WAIT).await);
    host.stop().await;
    let entry = store_of(root.path(), "mock", &spec, &program)
        .lookup("old-1")
        .expect("still recorded");
    assert_eq!(entry.title, None, "the explicit clear is persisted");
}

#[tokio::test]
async fn a_persisted_directory_always_wins_and_a_failed_load_grants_nothing() {
    let root = tempfile::tempdir().expect("tempdir");
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).expect("a");
    std::fs::create_dir_all(&b).expect("b");
    let b_arg = b.to_string_lossy().into_owned();
    let extra = ["--list-cwd", b_arg.as_str(), "--open-delay-ms", "600", "--fail-open"];
    // This source admitted `old-1` under A before.
    let (spec, program) = spec_of(root.path(), "beta", &extra);
    store_of(root.path(), "mock", &spec, &program).record("old-1", &canonical(&a), None, None);

    let (host, started) = host(root.path(), "beta", &extra).await;
    started.expect("started");
    // The agent now lists `old-1` under B.
    host.list_sessions(None).await.expect("list");
    let opening = tokio::spawn({
        let host = host.clone();
        async move { host.open_session("old-1", None).await }
    });
    eventually(|| host.snapshot()["sessions"][0]["pending"] == true, "the pending load").await;
    assert_eq!(host.session_dir("old-1"), Some(canonical(&a)), "never B, even while pending");
    assert_eq!(host.open_session("old-1", None).await, Err(HostError::Loading));
    assert_eq!(host.prompt("old-1", "hello", &[], None).await, Err(HostError::Loading));
    assert!(opening.await.expect("join").is_err(), "the load fails");
    assert_eq!(host.session_dir("old-1"), Some(canonical(&a)), "and B is still not granted");
    assert!(host.snapshot()["sessions"]
        .as_array()
        .expect("sessions")
        .is_empty());
    host.stop().await;
}

#[tokio::test]
async fn a_pending_resume_admits_nothing_until_it_succeeds() {
    let root = tempfile::tempdir().expect("tempdir");
    let b = root.path().join("b");
    std::fs::create_dir_all(&b).expect("b");
    let b_arg = b.to_string_lossy().into_owned();

    // A failing resume: no authority before, during or after.
    let failing = ["--list-cwd", b_arg.as_str(), "--open-delay-ms", "400", "--fail-open"];
    let (host, started) = host_named(root.path(), "failing", "gamma", &failing).await;
    started.expect("started");
    assert_eq!(host.info()["capabilities"]["loadSession"], false);
    host.list_sessions(None).await.expect("list");
    let opening = tokio::spawn({
        let host = host.clone();
        async move { host.open_session("old-1", None).await }
    });
    eventually(|| host.snapshot()["sessions"][0]["pending"] == true, "the pending resume").await;
    assert_eq!(host.session_dir("old-1"), None);
    assert!(opening.await.expect("join").is_err());
    assert_eq!(host.session_dir("old-1"), None, "a failed resume is revoked");
    host.stop().await;

    // A slow resume: blocked while pending, usable once confirmed.
    let slow = ["--list-cwd", b_arg.as_str(), "--open-delay-ms", "600"];
    let (host, started) = host_named(root.path(), "slow", "gamma", &slow).await;
    started.expect("started");
    host.list_sessions(None).await.expect("list");
    let opening = tokio::spawn({
        let host = host.clone();
        async move { host.open_session("old-1", None).await }
    });
    eventually(|| host.snapshot()["sessions"][0]["pending"] == true, "the pending resume").await;
    assert_eq!(host.session_dir("old-1"), None, "no authority while pending");
    assert_eq!(host.open_session("old-1", None).await, Err(HostError::Loading));
    assert_eq!(
        host.prompt("old-1", "hello", &[], None).await,
        Err(HostError::Loading),
        "another controller cannot prompt it yet"
    );
    let opened = opening.await.expect("join").expect("resumed");
    assert_eq!(opened["mode"], "resume");
    assert_eq!(host.session_dir("old-1"), Some(canonical(&b)));
    let history = host.history("old-1", None, None, None).expect("history");
    assert_eq!(history["truncated"], true, "resume does not claim complete history");
    let mut events = Events::from(&host);
    host.prompt("old-1", "hello", &[], None)
        .await
        .expect("usable now");
    events.until(is("turn_ended")).await;
    host.stop().await;
}

#[tokio::test]
async fn reusing_a_name_with_another_invocation_transfers_no_sessions_or_authority() {
    let root = tempfile::tempdir().expect("tempdir");
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).expect("work");
    let (first, started) = host(root.path(), "alpha", &["--model", "one"]).await;
    started.expect("started");
    let session = new_session(&first, &work).await;
    first.stop().await;

    // Same host name and profile id, different arguments.
    let (other, started) = host(root.path(), "alpha", &["--model", "two"]).await;
    started.expect("started");
    assert_eq!(other.session_dir(&session), None, "no file authority");
    let listed = other.list_sessions(None).await.expect("list");
    assert!(
        listed["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|s| s["sessionId"] != session.as_str()),
        "no sessions"
    );
    other.stop().await;

    // The original invocation still has them.
    let (again, started) = host(root.path(), "alpha", &["--model", "one"]).await;
    started.expect("started");
    assert_eq!(again.session_dir(&session), Some(canonical(&work)));
    again.stop().await;
}

#[tokio::test]
async fn permissions_keep_opaque_ids_reject_invalid_choices_and_resolve_once() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let mut events = Events::from(&host);
    host.prompt(&session, "permission", &[], None)
        .await
        .expect("admitted");
    let asked = events.until(is("permission")).await;
    let handle = asked["handle"].as_str().expect("handle").to_string();
    assert_eq!(asked["options"][0]["optionId"], "opt:allow ✓");
    assert_eq!(
        host.answer_permission(&handle, "opt:invented").await,
        Err(HostError::InvalidOption)
    );
    assert_eq!(
        host.snapshot()["permissions"][0]["handle"],
        handle,
        "invalid answer keeps it pending"
    );
    let (a, b) = tokio::join!(
        host.answer_permission(&handle, "opt:allow ✓"),
        host.answer_permission(&handle, "opt:deny")
    );
    assert!(a.is_ok() ^ b.is_ok(), "exactly one answer wins: {a:?} {b:?}");
    let winner = if a.is_ok() { "opt:allow ✓" } else { "opt:deny" };
    assert_eq!(host.answer_permission(&handle, winner).await, Err(HostError::AlreadyResolved));
    events.until(is("turn_ended")).await;
    let history = host.history(&session, None, None, None).expect("history");
    assert!(message_texts(&history).contains(&format!("selected:{winner}")));
    host.stop().await;
}

#[tokio::test]
async fn cancellation_answers_permissions_and_waits_for_the_real_stop_reason() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let mut events = Events::from(&host);
    host.prompt(&session, "permission", &[], None)
        .await
        .expect("admitted");
    let asked = events.until(is("permission")).await;
    assert!(host.cancel(&session, None, None).await.expect("cancel"));
    let resolved = events.until(is("permission_resolved")).await;
    assert_eq!(resolved["handle"], asked["handle"]);
    assert_eq!(resolved["outcome"], "cancelled");
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["outcome"]["stopReason"], "cancelled");
    let history = host.history(&session, None, None, None).expect("history");
    assert!(message_texts(&history).contains(&"cancelled".to_string()), "agent saw cancelled");

    // A slow turn stays busy after cancel until the agent answers, and the
    // trailing update sent after cancel is still folded.
    let turn = host
        .prompt(&session, "slow", &[], None)
        .await
        .expect("admitted");
    events
        .until(|b| b["type"] == "update" && b["turnId"] == turn)
        .await;
    assert_eq!(host.prompt(&session, "again", &[], None).await, Err(HostError::Busy));
    host.cancel(&session, Some(&turn), None)
        .await
        .expect("cancel");
    assert_eq!(
        host.prompt(&session, "again", &[], None).await,
        Err(HostError::Busy),
        "admission stays locked until the agent answers the cancelled prompt"
    );
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], turn);
    assert_eq!(ended["outcome"]["stopReason"], "cancelled");
    let history = host
        .history(&session, None, Some(&turn), None)
        .expect("turn");
    let items = history["window"][0]["items"]
        .as_array()
        .expect("items")
        .clone();
    assert!(items.iter().any(|i| i["title"] == "Trailing update"));
    host.stop().await;
}

/// `#[tokio::test]` runs on one thread, so nothing else runs between two
/// awaits that complete without yielding: the prompt task cannot dispatch
/// between admission and the cancel below.
#[tokio::test]
async fn cancel_and_dispatch_are_ordered_per_turn() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let mut events = Events::from(&host);

    // Cancelled right after admission: the agent never receives it.
    let early = host
        .prompt(&session, "slow", &[], None)
        .await
        .expect("admitted");
    assert!(host
        .cancel(&session, Some(&early), None)
        .await
        .expect("cancel"));
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], early);
    assert_eq!(ended["outcome"], json!({"kind": "cancelled"}));
    host.prompt(&session, "count", &[], None)
        .await
        .expect("admitted");
    events.until(is("turn_ended")).await;
    let history = host.history(&session, None, None, None).expect("history");
    assert_eq!(
        message_texts(&history),
        vec!["prompts:1".to_string()],
        "the cancelled prompt never reached the agent"
    );

    // Cancelled after dispatch: the cancel follows the prompt and the agent
    // reports its own stop reason.
    let sent = host
        .prompt(&session, "slow", &[], None)
        .await
        .expect("admitted");
    eventually(
        || host.snapshot()["sessions"][0]["turnDispatched"] == true,
        "the prompt to be dispatched",
    )
    .await;
    assert!(host
        .cancel(&session, Some(&sent), None)
        .await
        .expect("cancel"));
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], sent);
    assert_eq!(ended["outcome"], json!({"kind": "stopped", "stopReason": "cancelled"}));

    // A late cancel of an earlier turn leaves the next one running.
    let first = host
        .prompt(&session, "hello", &[], None)
        .await
        .expect("admitted");
    events.until(is("turn_ended")).await;
    let second = host
        .prompt(&session, "slow", &[], None)
        .await
        .expect("admitted");
    let current = Identity {
        host_id: Some(host.host_id().to_string()),
        generation: host.snapshot()["generation"].as_u64(),
    };
    assert!(!host
        .cancel(&session, Some(&first), Some(&current))
        .await
        .expect("stale cancel"));
    let other_generation = Identity {
        generation: current.generation.map(|g| g + 1),
        ..current.clone()
    };
    assert!(
        !host
            .cancel(&session, Some(&second), Some(&other_generation))
            .await
            .expect("other generation"),
        "a cancel for another generation does nothing"
    );
    let other_host = Identity {
        host_id: Some("another-host".into()),
        ..current.clone()
    };
    assert!(
        !host
            .cancel(&session, Some(&second), Some(&other_host))
            .await
            .expect("other host"),
        "a cancel for another host incarnation does nothing"
    );
    let snapshot = host.snapshot();
    assert_eq!(snapshot["sessions"][0]["runningTurnId"], second);
    assert_eq!(snapshot["sessions"][0]["cancelRequested"], false);
    assert!(host
        .cancel(&session, Some(&second), Some(&current))
        .await
        .expect("cancel"));
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], second);
    assert_eq!(outcomes_of(&host, &second).len(), 1);
    host.stop().await;
}

#[tokio::test]
async fn agent_file_requests_are_refused_without_reading() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let mut events = Events::from(&host);
    host.prompt(&session, "fs", &[], None)
        .await
        .expect("admitted");
    events.until(is("turn_ended")).await;
    let history = host.history(&session, None, None, None).expect("history");
    assert_eq!(message_texts(&history), vec!["fs:-32601".to_string()]);
    host.stop().await;
}

#[tokio::test]
async fn a_crash_ends_turns_as_exited_and_the_next_generation_is_isolated() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let before = host.snapshot()["generation"].as_u64().expect("generation");
    let mut events = Events::from(&host);
    let turn = host
        .prompt(&session, "crash", &[], None)
        .await
        .expect("admitted");
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], turn);
    assert_eq!(ended["outcome"]["kind"], "agentExited", "never reported as success");
    let snapshot = host.snapshot();
    assert!(snapshot["generation"].as_u64().expect("generation") > before);
    assert_eq!(snapshot["recentTurns"][0]["outcome"]["kind"], "agentExited");
    assert!(host.subscribe(before, 0).is_none(), "stale generation must resync");
    // The bounded restart brings up a fresh generation without the old session.
    eventually(|| host.info()["phase"]["state"] == "ready", "the restart").await;
    assert_eq!(host.prompt(&session, "hello", &[], None).await, Err(HostError::UnknownSession));
    host.stop().await;
}

#[tokio::test]
async fn a_leader_exit_is_detected_while_a_descendant_holds_the_output_open() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    let before = host.snapshot()["generation"].as_u64().expect("generation");
    let mut events = Events::from(&host);
    let turn = host
        .prompt(&session, "orphan", &[], None)
        .await
        .expect("admitted");
    let reported = events
        .until(|b| b["type"] == "update" && b["turnId"] == turn.as_str())
        .await;
    let descendant: i32 = reported["update"]["content"]["text"]
        .as_str()
        .and_then(|text| text.strip_prefix("orphan:"))
        .and_then(|pid| pid.parse().ok())
        .expect("descendant pid");
    assert!(descendant > 0);
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], turn);
    assert_eq!(ended["outcome"]["kind"], "agentExited");
    assert_eq!(outcomes_of(&host, &turn).len(), 1, "exactly one outcome");
    assert!(host.snapshot()["generation"].as_u64().expect("generation") > before);
    eventually(|| !alive(descendant), "the descendant to be cleaned up").await;
    eventually(|| host.info()["phase"]["state"] == "ready", "the bounded restart").await;
    host.stop().await;
}

#[tokio::test]
async fn unsupported_versions_and_auth_required_are_reported_honestly() {
    let root = tempfile::tempdir().expect("tempdir");
    let (old, started) = host(root.path(), "alpha", &["--version", "2"]).await;
    match started {
        Err(HostError::Start(reason)) => assert!(reason.contains("version 2"), "{reason}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(old.info()["phase"]["state"], "failed");
    old.stop().await;

    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &["--auth-required"]).await;
    started.expect("started");
    assert_eq!(
        host.new_session(&root.path().to_string_lossy(), None).await,
        Err(HostError::AuthRequired)
    );
    assert_eq!(host.info()["authRequired"], true);
    assert!(matches!(
        host.new_session("relative/dir", None).await,
        Err(HostError::InvalidInput(_))
    ));
    host.stop().await;
}

#[tokio::test]
async fn authentication_required_by_a_prompt_is_reported_with_one_outcome() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &["--auth-prompt"]).await;
    started.expect("started");
    let session = new_session(&host, root.path()).await;
    assert_eq!(host.info()["authRequired"], false);
    let mut events = Events::from(&host);
    let turn = host
        .prompt(&session, "hello", &[], None)
        .await
        .expect("admitted");
    // The sign-in state is published first, then the turn's one outcome.
    let state = events.until(is("host_state")).await;
    assert_eq!(state["authRequired"], true);
    let ended = events.until(is("turn_ended")).await;
    assert_eq!(ended["turnId"], turn);
    assert_eq!(ended["outcome"]["kind"], "failed");
    assert_eq!(host.info()["authRequired"], true);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(outcomes_of(&host, &turn).len(), 1, "exactly one outcome");
    host.stop().await;
}

#[tokio::test]
async fn the_gateway_resumes_from_the_snapshot_watermark_and_resets_stale_streams() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "beta", &[]).await;
    started.expect("started");
    let (client, server) = serve(&host).await;

    let info = client.info().await.expect("info");
    assert_eq!(info["api"], 1);
    let host_id = info["hostId"].as_str().expect("host id").to_string();
    let session = client
        .new_session(&root.path().to_string_lossy(), None)
        .await
        .expect("new")["sessionId"]
        .as_str()
        .expect("id")
        .to_string();
    let snapshot = client.snapshot().await.expect("snapshot");
    let generation = snapshot["generation"].as_u64().expect("generation");
    let watermark = snapshot["seq"].as_u64().expect("seq");
    let identity = Identity {
        host_id: Some(host_id.clone()),
        generation: Some(generation),
    };
    let mut stream = client
        .events(Some(&host_id), generation, watermark)
        .await
        .expect("events");
    let turn = client
        .prompt(&session, "hello", &[], Some(&identity))
        .await
        .expect("prompt");
    let mut seen = Vec::new();
    while let Some(event) = tokio::time::timeout(WAIT, stream.next())
        .await
        .expect("event in time")
    {
        let event = event.expect("event");
        assert!(event["seq"].as_u64().expect("seq") > watermark, "strictly after the watermark");
        seen.push(event["type"].as_str().unwrap_or("").to_string());
        if event["type"] == "turn_ended" {
            assert_eq!(event["turnId"], turn);
            break;
        }
    }
    assert!(seen.iter().any(|kind| kind == "turn_started"));
    assert!(seen.iter().any(|kind| kind == "update"));

    let mut stale = client
        .events(Some(&host_id), generation + 7, 0)
        .await
        .expect("stream");
    let reset = stale.next().await.expect("reset").expect("event");
    assert_eq!(reset["type"], "reset");
    let mut replaced = client
        .events(Some("another-host"), generation, watermark)
        .await
        .expect("stream");
    let reset = replaced.next().await.expect("reset").expect("event");
    assert_eq!(reset["type"], "reset", "a watermark of another host incarnation is refused");
    let next_generation = Identity {
        generation: Some(generation + 1),
        ..identity.clone()
    };
    match client
        .prompt(&session, "hello", &[], Some(&next_generation))
        .await
    {
        Err(ClientError::Host {
            status,
            code,
            ..
        }) => assert_eq!((status, code.as_str()), (409, "stale_host")),
        other => panic!("unexpected {other:?}"),
    }

    match client.answer("no-such-handle", "x").await {
        Err(ClientError::Host {
            status,
            code,
            ..
        }) => {
            assert_eq!((status, code.as_str()), (404, "unknown_permission"));
        },
        other => panic!("unexpected {other:?}"),
    }
    server.abort();
    host.stop().await;
}

#[tokio::test]
async fn idle_event_streams_release_their_slots_when_dropped() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let (client, server) = serve(&host).await;
    let info = client.info().await.expect("info");
    let host_id = info["hostId"].as_str().expect("host id").to_string();
    let generation = info["generation"].as_u64().expect("generation");
    // More open/drop cycles than there are slots, while the host stays idle.
    for cycle in 0..(api::MAX_STREAMS * 2) {
        let snapshot = client.snapshot().await.expect("snapshot");
        let seq = snapshot["seq"].as_u64().expect("seq");
        let deadline = tokio::time::Instant::now() + WAIT;
        let stream = loop {
            match client.events(Some(&host_id), generation, seq).await {
                Ok(stream) => break stream,
                Err(ClientError::Host {
                    status: 503, ..
                }) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                },
                Err(error) => panic!("cycle {cycle}: {error:?}"),
            }
        };
        drop(stream);
    }
    server.abort();
    host.stop().await;
}

/// Read `turn` of `session` from the host as a fold.
fn host_turn(host: &AgentHost, session: &str, turn: &str) -> Turn {
    let history = host.history(session, None, Some(turn), None).expect("turn");
    serde_json::from_value(history["window"][0].clone()).expect("turn")
}

fn assert_same_items(replica: &Turn, host: &Turn) {
    let ids = |turn: &Turn| {
        turn.items
            .iter()
            .map(|item| (item.id.clone(), item.text.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(replica), ids(host), "ids and text match the host");
    assert!(replica.items[0].id.ends_with(":u"), "the prompt is part of the fold");
}

/// Recover a controller the way the bridge does: snapshot, then the
/// running turn's fold with its watermark, then the events after it.
async fn follow_turn(
    client: &GatewayClient,
    host_id: &str,
    session: &str,
    turn: &str,
    cancel: bool,
) -> Turn {
    let snapshot = client.snapshot().await.expect("snapshot");
    let generation = snapshot["generation"].as_u64().expect("generation");
    let watermark = snapshot["seq"].as_u64().expect("seq");
    let mut replica = Replica::default();
    assert!(replica.needs_fold(session, Some(turn)));
    let history = client
        .history(session, None, Some(turn), None)
        .await
        .expect("history");
    assert!(replica.install(session, &history));
    let mut stream = client
        .events(Some(host_id), generation, watermark)
        .await
        .expect("events");
    if cancel {
        let identity = Identity {
            host_id: Some(host_id.to_string()),
            generation: Some(generation),
        };
        assert!(client
            .cancel(session, Some(turn), Some(&identity))
            .await
            .expect("cancel"));
    }
    loop {
        let event = tokio::time::timeout(WAIT, stream.next())
            .await
            .expect("event in time")
            .expect("open stream")
            .expect("event");
        assert_ne!(event["type"], "reset", "the recovered watermark is retained");
        replica.apply(&event);
        if event["type"] == "turn_ended" && event["turnId"] == turn {
            break;
        }
    }
    replica.turn(session, turn).expect("fold").clone()
}

#[tokio::test]
async fn a_controller_joining_mid_turn_ends_with_the_hosts_items() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let (client, server) = serve(&host).await;
    let host_id = client.info().await.expect("info")["hostId"]
        .as_str()
        .expect("host id")
        .to_string();
    let session = new_session(&host, root.path()).await;
    let mut events = Events::from(&host);
    let turn = host
        .prompt(&session, "stream", &[], None)
        .await
        .expect("admitted");
    // Join after several chunks have streamed.
    for _ in 0..5 {
        events
            .until(|b| b["type"] == "update" && b["turnId"] == turn.as_str())
            .await;
    }
    let followed = follow_turn(&client, &host_id, &session, &turn, false).await;
    let host_view = host_turn(&host, &session, &turn);
    assert_same_items(&followed, &host_view);
    assert!(host_view.items.len() >= 3, "message, tool call, message");
    assert!(followed.items[1].text.starts_with("part-0 "), "no missing prefix");
    server.abort();
    host.stop().await;
}

/// A stop issued while the agent is still negotiating (as on app quit right
/// after a start) must not wait for the handshake, must end the process,
/// and must leave a host that never launches again.
#[tokio::test]
async fn stopping_during_a_slow_handshake_cuts_the_launch_short_and_ends_the_agent() {
    let root = tempfile::tempdir().expect("tempdir");
    let pid_file = root.path().join("agent.pid");
    let pid_arg = pid_file.to_string_lossy().into_owned();
    let extra = ["--init-delay-ms", "10000", "--pid-file", pid_arg.as_str()];
    let (spec, program) = spec_of(root.path(), "alpha", &extra);
    let host = AgentHost::create(HostOptions {
        store: store_of(root.path(), "slow-start", &spec, &program),
        spec,
        program,
        path: None,
    });
    let launching = tokio::spawn({
        let host = host.clone();
        async move { host.launch().await }
    });
    let read_pid = || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|pid| pid.trim().parse::<i32>().ok())
    };
    eventually(|| read_pid().is_some(), "the agent to start").await;
    let pid = read_pid().expect("pid");
    assert!(alive(pid));
    let started = tokio::time::Instant::now();
    host.stop().await;
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "a stop does not wait out the 10 s handshake"
    );
    assert!(launching.await.expect("join").is_err(), "the launch was cut short");
    eventually(|| !alive(pid), "the agent process to be gone").await;
    assert_eq!(host.info()["phase"]["state"], "stopped");
    assert!(host.launch().await.is_err(), "a stopped host never launches again");
    assert_eq!(host.restart().await, Err(HostError::NotReady));
}

async fn bind_at(addr: std::net::SocketAddr) -> tokio::net::TcpListener {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return listener,
            Err(error) => {
                assert!(tokio::time::Instant::now() < deadline, "rebinding {addr}: {error}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            },
        }
    }
}

fn stale_host(result: Result<impl std::fmt::Debug, ClientError>) {
    match result {
        Err(ClientError::Host {
            status,
            code,
            ..
        }) => assert_eq!((status, code.as_str()), (409, "stale_host")),
        other => panic!("expected a stale-host refusal, got {other:?}"),
    }
}

/// A relay endpoint keeps its address when the host behind it is replaced;
/// the replacement starts at the same generation and its agent hands out the
/// same session ids. Requests carrying the old host's identity must be
/// refused before anything reaches the new agent.
#[tokio::test]
async fn a_replacement_host_behind_the_same_endpoint_refuses_the_old_identity() {
    let root = tempfile::tempdir().expect("tempdir");
    let cwd = root.path().to_string_lossy().into_owned();
    let (first, started) = host_named(root.path(), "first", "alpha", &[]).await;
    started.expect("started");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let origin = format!("http://{addr}/");
    let server = tokio::spawn(api::serve(listener, first.clone()));
    let client = GatewayClient::new(&origin).expect("client");
    let info = client.info().await.expect("info");
    let old = Identity {
        host_id: info["hostId"].as_str().map(str::to_string),
        generation: info["generation"].as_u64(),
    };
    let session = client.new_session(&cwd, Some(&old)).await.expect("new")["sessionId"]
        .as_str()
        .expect("id")
        .to_string();
    server.abort();
    let _ = server.await;
    first.stop().await;

    let (second, started) = host_named(root.path(), "second", "alpha", &[]).await;
    started.expect("started");
    let server = tokio::spawn(api::serve(bind_at(addr).await, second.clone()));
    let reused = new_session(&second, root.path()).await;
    assert_eq!(reused, session, "the new agent reuses the session id");
    assert_eq!(second.info()["generation"].as_u64(), old.generation, "and the generation");
    let client = GatewayClient::new(&origin).expect("client");
    stale_host(client.prompt(&session, "hello", &[], Some(&old)).await);
    stale_host(client.open_session(&session, Some(&old)).await);
    stale_host(client.new_session(&cwd, Some(&old)).await);
    assert_eq!(client.cancel(&session, None, Some(&old)).await, Ok(false));
    assert!(second.snapshot()["sessions"][0]["runningTurnId"].is_null());

    let fresh = Identity {
        host_id: Some(second.host_id().to_string()),
        generation: second.info()["generation"].as_u64(),
    };
    let mut events = Events::from(&second);
    client
        .prompt(&session, "count", &[], Some(&fresh))
        .await
        .expect("current identity");
    events.until(is("turn_ended")).await;
    let history = second.history(&session, None, None, None).expect("history");
    assert_eq!(
        message_texts(&history),
        vec!["prompts:1".to_string()],
        "the stale prompt never reached the new agent"
    );
    server.abort();
    second.stop().await;
}

#[tokio::test]
async fn a_replay_overflow_in_the_same_generation_recovers_the_full_text() {
    let root = tempfile::tempdir().expect("tempdir");
    let (host, started) = host(root.path(), "alpha", &[]).await;
    started.expect("started");
    let (client, server) = serve(&host).await;
    let host_id = client.info().await.expect("info")["hostId"]
        .as_str()
        .expect("host id")
        .to_string();
    let session = new_session(&host, root.path()).await;
    let before = client.snapshot().await.expect("snapshot");
    let generation = before["generation"].as_u64().expect("generation");
    let watermark = before["seq"].as_u64().expect("seq");
    let turn = host
        .prompt(&session, "flood", &[], None)
        .await
        .expect("admitted");
    eventually(
        || {
            host.history(&session, None, Some(&turn), None)
                .ok()
                .and_then(|h| h["window"][0]["items"][1]["text"].as_str().map(str::len))
                .is_some_and(|len| len >= 5000)
        },
        "the flood",
    )
    .await;
    // The old watermark is gone from the log: the stream says so.
    let mut stale = client
        .events(Some(&host_id), generation, watermark)
        .await
        .expect("stream");
    let first = stale.next().await.expect("event").expect("event");
    assert_eq!(first["type"], "reset");
    assert_eq!(host.snapshot()["generation"].as_u64(), Some(generation), "same generation");
    // Recover while the turn still runs, then let it finish.
    let followed = follow_turn(&client, &host_id, &session, &turn, true).await;
    let host_view = host_turn(&host, &session, &turn);
    assert_same_items(&followed, &host_view);
    assert_eq!(followed.items[1].text, format!("{}tail", "x".repeat(5000)));
    server.abort();
    host.stop().await;
}

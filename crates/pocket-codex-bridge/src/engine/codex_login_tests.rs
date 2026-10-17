use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use super::*;

/// A session entry whose "child" is this test process under a wrong start
/// time: an identity that must never be signalled.
fn fake(id: &str) -> Private {
    runtime::init(std::env::temp_dir()).expect("test runtime");
    let me = OwnedChild::of(std::process::id()).expect("this process runs");
    Private {
        key: format!("{KEY_PREFIX}{id}"),
        child: Some(OwnedChild::recorded(me.pid(), u64::MAX)),
        addr: "127.0.0.1:9".into(),
        completions: Arc::default(),
        watcher: None,
    }
}

#[test]
fn calls_for_a_session_that_is_not_open_reach_nothing() {
    assert!(key_of("no-such-session").is_err());
    assert!(auth_status("no-such-session", None).is_err());
    assert!(login_cancel("no-such-session", "login").is_err());
    // Closing an unknown id stops nothing.
    close("no-such-session").expect("nothing to stop");
}

#[test]
fn closing_one_session_leaves_the_others_open() {
    sessions().insert("login-test-a".into(), fake("login-test-a"));
    sessions().insert("login-test-b".into(), fake("login-test-b"));
    close("login-test-a").expect("closed");
    assert!(key_of("login-test-a").is_err());
    assert!(key_of("login-test-b").is_ok(), "another attempt is untouched");
    close("login-test-b").expect("closed");
    assert!(key_of("login-test-b").is_err());
}

#[test]
fn only_this_attempts_completion_counts() {
    let completions: Completions = Arc::default();
    let (login, done) =
        completion_of(r#"{"loginId":"mine","success":true,"error":null}"#).expect("completion");
    completions.lock().expect("lock").insert(login, done);
    let failed = completion_of(r#"{"loginId":"other","success":false,"error":"denied"}"#)
        .expect("completion");
    assert_eq!(failed.1, Completion {
        success: false,
        error: Some("denied".into()),
    });
    let held = completions.lock().expect("lock");
    assert_eq!(held.get("mine").map(|c| c.success), Some(true));
    assert!(held.get("other").is_none(), "another attempt's answer is not ours");
    assert!(completion_of(r#"{"success":true}"#).is_none(), "no login id, no attempt");
}

/// A table of its own, so closing it never affects other tests.
fn own_table() -> &'static Table {
    Box::leak(Box::new(Table::new()))
}

#[test]
fn the_limit_counts_attempts_still_opening() {
    let table = own_table();
    let held: Vec<Reservation> = (0..MAX_SESSIONS)
        .map(|n| table.reserve(&format!("opening-{n}")).expect("room"))
        .collect();
    let full = table.reserve("one-more").err().expect("full");
    assert!(full.to_string().contains("too many"), "{full}");
    drop(held);
    assert!(table.reserve("one-more").is_ok(), "a released attempt frees its place");
}

/// Two attempts finish opening after the quit began. Each is handed its
/// session back and takes a while to stop its child; the quit returns only
/// after both stops finished, never as soon as the commits were refused.
#[test]
fn a_quit_waits_until_every_late_attempt_stopped_its_child() {
    let table = own_table();
    let stopped: Vec<Arc<AtomicBool>> = (0..2).map(|_| Arc::default()).collect();
    let (refused_tx, refused) = mpsc::channel();
    let openers: Vec<_> = stopped
        .iter()
        .enumerate()
        .map(|(n, stopped)| {
            let reservation = table.reserve(&format!("late-{n}")).expect("open");
            let (stopped, refused_tx) = (stopped.clone(), refused_tx.clone());
            std::thread::spawn(move || {
                // Spawn and readiness, still running when the quit begins.
                std::thread::sleep(Duration::from_millis(100));
                let handed_back = reservation.commit(fake(&format!("late-{n}"))).is_err();
                refused_tx.send(handed_back).expect("send");
                // Stopping the handed-back child takes a while...
                std::thread::sleep(Duration::from_millis(300));
                stopped.store(true, Ordering::SeqCst);
                // ...and only then is the attempt released.
                drop(reservation);
            })
        })
        .collect();
    let quit = close_table(table, Duration::from_secs(10));
    assert!(quit.is_ok(), "{quit:?}");
    for stopped in &stopped {
        assert!(stopped.load(Ordering::SeqCst), "the quit outlasted every late stop");
    }
    for _ in 0..2 {
        assert!(refused
            .recv_timeout(Duration::from_secs(10))
            .expect("answered"));
    }
    for opener in openers {
        opener.join().expect("opener");
    }
    assert!(table.state().open.is_empty() && table.state().opening.is_empty());
    let after = table.reserve("after-quit").err().expect("closed");
    assert!(after.to_string().contains("quitting"), "{after}");
}

#[test]
fn a_quit_that_still_sees_an_attempt_opening_fails() {
    let table = own_table();
    let reservation = table.reserve("slow").expect("open");
    let quit = close_table(table, Duration::from_millis(50));
    let error = quit.expect_err("not settled");
    assert!(error.to_string().contains("still starting"), "{error}");
    // A later quit waits for it again and succeeds once it settled.
    drop(reservation);
    assert!(close_table(table, Duration::from_secs(1)).is_ok());
}

/// A minimal app-server on a socket this test process holds.
struct FakeServer {
    url: String,
    methods: Arc<Mutex<Vec<String>>>,
    /// Ends the server wherever it waits (accept, handshake or read).
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<std::thread::JoinHandle<()>>,
    /// The socket stays bound through this handle after the server thread
    /// dropped its own (when its one connection closed), until [`finish`].
    ///
    /// [`finish`]: FakeServer::finish
    bound: Option<std::net::TcpListener>,
}

impl FakeServer {
    /// After the test's assertions: wait for the server thread (a panic in
    /// it fails the test), then unbind; returns every method it was sent.
    fn finish(mut self) -> Vec<String> {
        if let Some(server) = self.server.take() {
            server.join().expect("server");
        }
        let methods = self.methods.lock().expect("lock").clone();
        self.bound.take();
        methods
    }
}

impl Drop for FakeServer {
    /// Every other exit (an assertion that failed first): cancel the server
    /// and join it, so no thread is left waiting on the socket.
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(server) = self.server.take() {
            let joined = server.join();
            // While unwinding already, a second panic would abort the run.
            if !std::thread::panicking() {
                joined.expect("server");
            }
        }
    }
}

/// A minimal app-server: answers `initialize` and `config/read` with
/// `config`, and records every method it was sent.
fn fake_server(config: Value) -> FakeServer {
    use futures::{SinkExt as _, StreamExt as _};
    let methods: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = methods.clone();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("ws://{}", listener.local_addr().expect("addr"));
    listener.set_nonblocking(true).expect("nonblocking");
    let bound = listener.try_clone().expect("a second handle on the socket");
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let server = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let serve = async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            let (stream, _) = listener.accept().await.expect("accept");
            // The app-server's own handshake: the client offers compression.
            let mut ws = tokio_tungstenite::accept_async_with_config(
                stream,
                Some(tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()),
            )
            .await
            .expect("ws");
            while let Some(Ok(frame)) = ws.next().await {
                let Ok(text) = frame.into_text() else { continue };
                let Ok(request) = serde_json::from_str::<Value>(&text) else { continue };
                let method = request["method"].as_str().unwrap_or_default().to_string();
                seen.lock().expect("lock").push(method.clone());
                let Some(id) = request.get("id").cloned() else { continue };
                let result = match method.as_str() {
                    "config/read" => serde_json::json!({ "config": config.clone() }),
                    _ => serde_json::json!({}),
                };
                let reply = serde_json::json!({ "id": id, "result": result }).to_string();
                if ws
                    .send(tokio_tungstenite::tungstenite::Message::text(reply))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        };
        rt.block_on(async move {
            tokio::select! {
                _ = cancelled => {}
                () = serve => {}
            }
        });
    });
    FakeServer {
        url,
        methods,
        cancel: Some(cancel),
        server: Some(server),
        bound: Some(bound),
    }
}

/// A test that fails before finishing its server still ends it: dropped
/// before any connection, or with one open and idle, it is cancelled and
/// joined promptly, and the socket is released.
#[test]
fn a_fake_server_left_unfinished_is_cancelled_and_joined() {
    fn dropped_within(fixture: FakeServer, keep: Option<std::net::TcpStream>) {
        let (done_tx, done) = mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(fixture);
            done_tx.send(()).expect("send");
            drop(keep);
        });
        done.recv_timeout(Duration::from_secs(10))
            .expect("the server was cancelled, not waited out");
        dropper.join().expect("dropper");
    }
    let idle = fake_server(serde_json::json!({}));
    dropped_within(idle, None);

    let connected = fake_server(serde_json::json!({}));
    let addr = connected.url.trim_start_matches("ws://").to_string();
    let open = std::net::TcpStream::connect(&addr).expect("connect");
    dropped_within(connected, Some(open));
    // Completion above happens only after Drop joined the owned server and
    // dropped its listener handles. Do not probe the released ephemeral port:
    // another parallel test may already own it, or it may be in TIME_WAIT.
}

/// A fixture "codex" child to be stopped (or kept).
#[cfg(unix)]
fn fixture_child() -> (crate::engine::owned_child::tests::Fixture, OwnedChild) {
    crate::engine::owned_child::tests::sleeper()
}

/// An open attempt owning `child`.
#[cfg(unix)]
fn owning(id: &str, child: OwnedChild) -> Private {
    Private {
        child: Some(child),
        ..fake(id)
    }
}

/// A sign-in closed by its page whose child does not exit stays owned: a
/// quit fails, a second quit still fails (never an early success), and
/// once the child exited a quit succeeds.
#[cfg(unix)]
#[test]
fn a_closed_sign_in_whose_child_does_not_exit_keeps_the_quit_failing() {
    use crate::engine::owned_child::tests::refuse_stops;
    let table = own_table();
    let (mut process, child) = fixture_child();
    refuse_stops(child, true);
    table
        .state()
        .open
        .insert("held".into(), owning("held", child));
    let private = table.take("held").expect("open");
    assert!(!stop(&table.stopping, private), "its stop was refused");
    for _ in 0..2 {
        let quit = close_table(table, Duration::ZERO).expect_err("still running");
        assert!(quit.to_string().contains("did not exit"), "{quit}");
    }
    assert!(child.running(), "a refused stop signalled nothing");
    refuse_stops(child, false);
    close_table(table, Duration::ZERO).expect("it exited");
    assert!(!child.running());
    let _ = process.wait();
}

/// A close taken from the table but not yet stopped is not invisible to a
/// quit: the quit finds its child in the ledger and stops it itself.
#[cfg(unix)]
#[test]
fn a_close_in_progress_is_finished_by_the_quit() {
    let table = own_table();
    let (mut process, child) = fixture_child();
    table
        .state()
        .open
        .insert("closing".into(), owning("closing", child));
    let taken = table.take("closing").expect("open");
    // The quit runs between the close taking the attempt and stopping it.
    close_table(table, Duration::ZERO).expect("the quit stopped it");
    assert!(!child.running(), "the quit did not succeed before the child exited");
    assert!(stop(&table.stopping, taken), "the late stop finds it gone");
    let _ = process.wait();
}

/// An attempt that finishes opening after the quit began is refused and
/// stops its child; one that does not exit keeps every quit failing until
/// it does, after its reservation is long released.
#[cfg(unix)]
#[test]
fn a_late_refused_open_keeps_its_child_owned_until_it_exits() {
    use crate::engine::owned_child::tests::refuse_stops;
    let table = own_table();
    let reservation = table.reserve("late").expect("open");
    let quit = close_table(table, Duration::ZERO).expect_err("still opening");
    assert!(quit.to_string().contains("still starting"), "{quit}");
    let (mut process, child) = fixture_child();
    refuse_stops(child, true);
    let private = reservation
        .commit(owning("late", child))
        .expect_err("quitting");
    assert!(!stop(&table.stopping, private));
    drop(reservation);
    let quit = close_table(table, Duration::ZERO).expect_err("its child still runs");
    assert!(quit.to_string().contains("did not exit"), "{quit}");
    refuse_stops(child, false);
    close_table(table, Duration::ZERO).expect("settled");
    assert!(!child.running());
    let _ = process.wait();
}

/// A failed close keeps its attempt's child, counted against the limit;
/// closing that attempt again tries exactly that child (never another
/// attempt's), fails again while it runs, and frees the place once it exited.
#[cfg(unix)]
#[test]
fn a_failed_close_is_retried_by_closing_again_and_counts_against_the_limit() {
    use crate::engine::owned_child::tests::refuse_stops;
    let table = own_table();
    let (mut process_a, a) = fixture_child();
    let (mut process_b, b) = fixture_child();
    for (id, child) in [("a", a), ("b", b)] {
        refuse_stops(child, true);
        table.state().open.insert(id.into(), owning(id, child));
        let failed = close_in(table, id).expect_err("its child still runs");
        assert!(failed.to_string().contains("close it again"), "{failed}");
        assert!(!table.state().open.contains_key(id), "the attempt is closed");
    }
    // The two kept children take two places.
    let held: Vec<Reservation> = (0..MAX_SESSIONS - 2)
        .map(|n| table.reserve(&format!("room-{n}")).expect("room"))
        .collect();
    let full = table.reserve("over").err().expect("full");
    assert!(full.to_string().contains("still stopping"), "{full}");
    // Repeating a close that still fails never reports success.
    close_in(table, "a").expect_err("still refused");
    // Only `a`'s child is tried again: `b`'s would exit now, and stays.
    refuse_stops(a, false);
    refuse_stops(b, false);
    close_in(table, "a").expect("a exited");
    assert!(!a.running());
    assert!(b.running(), "another attempt's child was not touched");
    assert_eq!(table.stopping.owners(|_| true), [format!("{KEY_PREFIX}b")]);
    let next = table.reserve("next").expect("a's place is free again");
    assert!(table.reserve("over").is_err(), "b still holds one");
    close_in(table, "b").expect("b exited");
    assert!(table.stopping.owners(|_| true).is_empty());
    drop(held);
    drop(next);
    let _ = process_a.wait();
    let _ = process_b.wait();
}

/// A report for a server this test process holds the socket of.
fn report_of(url: &str) -> SpawnReport {
    let me = pocket_codex_codex::ProcessIdentity::of(std::process::id()).expect("runs");
    SpawnReport {
        info: pocket_codex_core::state::CodexProcessInfo {
            pid: me.pid,
            listen: url.to_string(),
            log_file: "unused.log".into(),
            started_at: String::new(),
        },
        reused: false,
        log_offset: 0,
        listener_confirmed: true,
        spawned: Some(me),
        owned: Some(me),
    }
}

fn official_expected() -> ExpectedRoute {
    codex_launch(&CodexProviderMode::Official, "sign-in", "")
        .expect("official")
        .expected
        .expect("expected")
}

/// A managed configuration routing the private sign-in server elsewhere:
/// refused before any login request, and exactly its child stopped.
#[cfg(unix)]
#[test]
fn a_sign_in_server_routed_elsewhere_is_refused_and_stopped() {
    let fixture = fake_server(serde_json::json!({
        "model_provider": "corp",
        "chatgpt_base_url": "https://corp.example/backend-api/",
    }));
    let (mut process, child) = fixture_child();
    let table = own_table();
    let report = report_of(&fixture.url);
    let refused = admit(&report, &official_expected(), Some(child), &table.stopping, "k")
        .expect_err("refused");
    let message = refused.to_string();
    assert!(
        message.contains("model_provider") && message.contains("chatgpt_base_url"),
        "{message}"
    );
    assert!(!message.contains("corp.example"), "no configuration value is quoted");
    assert!(!child.running(), "its own child was stopped");
    assert!(table.stopping.owners(|_| true).is_empty(), "and is no longer kept");
    let _ = process.wait();
    let methods = fixture.finish();
    assert!(methods.iter().any(|m| m == "config/read"), "{methods:?}");
    assert!(
        methods.iter().all(|m| !m.starts_with("account/")),
        "no login request reached it: {methods:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_sign_in_server_on_the_official_route_is_admitted() {
    // This fixture attributes its listener to the test process. Other tests'
    // starting/exiting descendants make that process tree unverifiable.
    const ISOLATED: &str = "PCX_SIGN_IN_OWNER_FIXTURE";
    if std::env::var_os(ISOLATED).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "engine::codex_login::tests::a_sign_in_server_on_the_official_route_is_admitted",
                "--nocapture",
            ])
            .env(ISOLATED, "1")
            .output()
            .expect("isolated ownership fixture");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let fixture = fake_server(serde_json::json!({
        "model_provider": "openai",
        "chatgpt_base_url": "https://chatgpt.com/backend-api/",
    }));
    let (mut process, child) = fixture_child();
    let report = report_of(&fixture.url);
    // The configuration connection is closed by now; the socket is still
    // held by this test process, the report's owner.
    admit(&report, &official_expected(), Some(child), &own_table().stopping, "k")
        .expect("admitted");
    assert!(child.running(), "an admitted child keeps running");
    assert!(still_owned(&report), "still held by its owner after the check");
    fixture.finish();
    assert!(!still_owned(&report), "an unbound endpoint is no longer owned");
    let _ = process.kill();
    let _ = process.wait();
}

/// The official route answered by a server whose socket the attempt's own
/// child does not hold (this test process holds it): refused, and the
/// attempt's child stopped.
#[cfg(unix)]
#[test]
fn a_sign_in_answered_by_a_server_the_attempt_did_not_start_is_refused() {
    let fixture = fake_server(serde_json::json!({
        "model_provider": "openai",
        "chatgpt_base_url": "https://chatgpt.com/backend-api/",
    }));
    let (mut process, child) = fixture_child();
    let ours = pocket_codex_codex::ProcessIdentity::of(process.id()).expect("runs");
    let report = SpawnReport {
        spawned: Some(ours),
        owned: Some(ours),
        ..report_of(&fixture.url)
    };
    let port: u16 = fixture
        .url
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .expect("a port");
    // The socket is bound, by this test process, which the attempt did not
    // start; it stays bound until `finish`, so the refusal below is for a
    // foreign holder, not an endpoint that closed.
    assert_eq!(
        pocket_codex_core::process::endpoint_owner_under(
            port,
            &fixture.url,
            ours.pid,
            ours.started
        ),
        pocket_codex_core::process::EndpointOwner::Foreign
    );
    let table = own_table();
    let refused = admit(&report, &official_expected(), Some(child), &table.stopping, "k")
        .expect_err("refused");
    assert!(refused.to_string().contains("no longer served"), "{refused}");
    assert!(!child.running(), "its own child was stopped");
    let _ = process.wait();
    let methods = fixture.finish();
    assert!(methods.iter().all(|m| !m.starts_with("account/")), "{methods:?}");
}

#[test]
fn private_keys_are_never_relay_keys() {
    assert!(!KEY_PREFIX.starts_with("pcx:") && !KEY_PREFIX.starts_with("pcxu:"));
}

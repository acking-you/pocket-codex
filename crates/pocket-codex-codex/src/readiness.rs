//! Post-spawn readiness verification for a `codex app-server`.
//!
//! [`crate::spawn`] deliberately never fails just because the child did not
//! come up — it records honest state and lets `status` report the truth
//! later. That is right for the supervisor, but wrong for an interactive
//! launch: `pocket-codex serve` / `pocket-codex codex start` used to print a
//! green "✓ codex app-server" the moment the child was forked, even when it
//! died on the very next tick (classically a bind failure because the listen
//! port is already taken — `os error 10048` on Windows, `EADDRINUSE` on
//! unix). The failure then only surfaced minutes later as a stale status,
//! with the real error buried in the log file.
//!
//! [`verify_ready`] closes that gap: after a spawn it polls the app-server's
//! `/readyz` endpoint (the same one the health watchdog probes) until it
//! answers, the launch has provably failed, or the timeout elapses. On
//! failure it returns a [`StartupFailure`] carrying the tail of *this run's*
//! log output (via [`crate::SpawnReport::log_offset`]) and whether that
//! output points at an address-already-in-use bind error, so the CLI can
//! fail fast with the actual cause and a useful hint.
//!
//! A dead child PID alone is deliberately NOT treated as failure: when
//! `codex` is an npm shim, the process we spawned exits right after handing
//! off to the native binary, which may still need seconds to bind (cold
//! start, AV scan). The one *provable* early-failure signal is the child
//! being gone while this run's log already shows the bind error — that pair
//! fails the launch in a couple of seconds; everything else gets the full
//! timeout to come up — full, that is, unless [`crate::spawn`]'s own port
//! wait already ran dry without ever seeing a listener
//! ([`SpawnReport::listener_confirmed`] is `false`), in which case the
//! budget is capped to a short grace instead of re-waiting on top.
//!
//! [`spawn_ready`] fuses [`crate::spawn`] + [`verify_ready`] into the one
//! call every launch path wants, so a new call site cannot forget the
//! verification half of the pair.

use std::{
    fmt,
    fmt::Write as _,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use pocket_codex_core::process::{pid_running, probe_host};

use crate::process::{spawn, ws_host_port, SpawnOptions, SpawnReport};

/// Default overall budget for [`verify_ready`]. [`crate::spawn`] has already
/// waited for the listen port, so a healthy server answers `/readyz` almost
/// immediately; the budget only matters for one that is listening but still
/// warming up.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Cap on [`verify_ready`]'s budget when [`crate::spawn`]'s own port wait
/// ended without ever observing a listener
/// ([`SpawnReport::listener_confirmed`] is `false`) — either it ran its full
/// course (~15s) or it was cut short by provable failure. The child almost
/// certainly died during startup, so re-waiting the full ready budget on top
/// would just block the caller for another stretch before the same failure
/// surfaces.
/// The grace still covers the shim-handoff semantics: a couple of the
/// dead-child-and-bind-error-logged checks, and a final window for a
/// listener that appears right at the boundary.
const UNCONFIRMED_LISTENER_GRACE: Duration = Duration::from_secs(2);

/// Connect timeout for a single `/readyz` probe (loopback — fast).
const CONNECT_TIMEOUT: Duration = Duration::from_millis(600);

/// Read/write timeout for a single `/readyz` exchange. More generous than
/// the connect timeout so a heavily-loaded host that is slow to write the
/// status line is not misread as unready.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// Pause between probes.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Cadence of the dead-child + log checks inside poll loops. Each check
/// snapshots process state (sysinfo) and re-reads the log tail, so it runs
/// once a second rather than on every port/readyz probe.
pub(crate) const DEATH_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// How long the unix-socket path watches for a fatal bind error before
/// reporting OK (it has no HTTP endpoint to confirm readiness against).
const UNIX_WATCH: Duration = Duration::from_secs(2);

/// Cap on how much of the log file is read back for the failure tail.
const TAIL_MAX_BYTES: u64 = 16 * 1024;

/// Cap on how many log lines a [`StartupFailure`] carries.
const TAIL_MAX_LINES: usize = 20;

/// A freshly-launched app-server that never became ready.
///
/// `Display` gives the one-line summary; [`Self::diagnosis`] renders the full
/// multi-line report, with each front-end (CLI, desktop) supplying its own
/// phrasing of the pick-another-port remedy (e.g. which flag or form field
/// picks it).
#[derive(Debug)]
pub struct StartupFailure {
    /// The spawned process was observed dead while nothing served the listen
    /// address — it exited during startup (as opposed to still running but
    /// unresponsive).
    pub process_exited: bool,

    /// The log output points at an address-already-in-use bind failure
    /// (`os error 10048` / `EADDRINUSE` / "address already in use"), i.e.
    /// retrying on another port would likely succeed.
    pub port_in_use: bool,

    /// The listen URL that never became ready.
    pub listen: String,

    /// TCP port parsed out of `listen`, when it is a `ws://` URL — so a
    /// caller building a "retry with another port" hint doesn't have to
    /// re-parse the URL.
    pub port: Option<u16>,

    /// The log file the child's stdout/stderr were redirected to.
    pub log_file: PathBuf,

    /// Last lines of this run's log output (empty when the child wrote
    /// nothing before dying).
    pub log_tail: Vec<String>,
}

impl StartupFailure {
    /// The full multi-line diagnosis shared by every front-end: the one-line
    /// cause, the log location, this run's last output, and — when the listen
    /// port was already taken — `retry_hint`, the caller's phrasing of the
    /// pick-another-port remedy (built from [`Self::port`]).
    pub fn diagnosis(&self, retry_hint: &str) -> String {
        let mut msg = self.to_string();
        let _ = write!(msg, "\n    log: {}", self.log_file.display());
        if self.log_tail.is_empty() {
            msg.push_str("\n    (this run wrote no log output before it stopped)");
        } else {
            msg.push_str("\n    last output:");
            for line in &self.log_tail {
                let _ = write!(msg, "\n      {line}");
            }
        }
        if self.port_in_use {
            let _ = write!(msg, "\n    hint: the listen port is already in use — {retry_hint}");
            // On Windows the usual invisible holder is a WSL mirrored-networking
            // port lease: nothing shows in netstat, yet binds fail with 10048.
            #[cfg(windows)]
            msg.push_str(
                " — or free it (a leaked WSL mirrored-networking lease holds ports invisibly; \
                 `wsl --shutdown` releases them)",
            );
        }
        msg
    }
}

impl fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.process_exited {
            write!(f, "codex app-server exited during startup (listen {})", self.listen)
        } else {
            write!(f, "codex app-server did not become ready on {} in time", self.listen)
        }
    }
}

impl std::error::Error for StartupFailure {}

/// Wait until the just-spawned app-server actually serves, or explain why it
/// will not.
///
/// Polls `/readyz` on the listen address until it answers 2xx (`Ok`), the
/// launch provably failed — child gone AND the bind error already in the log
/// (`Err`, fast) — or `timeout` elapses (`Err`). A reused/adopted server
/// skips the process-death check — there is no fresh child to watch — but is
/// still probed, so adopting a wedged listener fails the launch instead of
/// publishing it. Unix-socket transports have no HTTP endpoint and are only
/// watched briefly for the fatal-bind-error signal.
///
/// When [`crate::spawn`]'s own port wait already ran dry without seeing a
/// listener ([`SpawnReport::listener_confirmed`] is `false`), `timeout` is
/// capped to a short grace — the server had a whole spawn-time wait to bind
/// and never did, so every caller's worst case shrinks uniformly instead of
/// stacking a second full wait on the first.
pub fn verify_ready(report: &SpawnReport, timeout: Duration) -> Result<(), StartupFailure> {
    let Some((host, port)) = ws_host_port(&report.info.listen) else {
        return verify_unix(report);
    };
    let timeout =
        if report.listener_confirmed { timeout } else { timeout.min(UNCONFIRMED_LISTENER_GRACE) };
    let deadline = Instant::now() + timeout;
    let mut next_death_check = Instant::now();
    let mut observed_dead = false;
    loop {
        let down = match probe_readyz(&host, port) {
            Probe::Ready => return Ok(()),
            // Something is listening — a booting server; keep waiting.
            Probe::NotReady => false,
            Probe::Down => {
                if !report.reused && Instant::now() >= next_death_check {
                    next_death_check = Instant::now() + DEATH_CHECK_INTERVAL;
                    if !pid_running(report.info.pid) {
                        observed_dead = true;
                        if bind_failure_logged(&report.info.log_file, report.log_offset) {
                            return Err(failure(report, true));
                        }
                    }
                }
                true
            },
        };
        if Instant::now() >= deadline {
            // "Exited" only when the child was seen dead and, at the end,
            // still nothing served — a dead shim in front of a live server
            // ends in Ready/NotReady instead and never reaches here as such.
            return Err(failure(report, observed_dead && down));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Poll `/readyz` on `host:port` until it answers 2xx (`true`), or `timeout`
/// elapses (`false`).
///
/// The process-agnostic sibling of [`verify_ready`], for an app-server with no
/// child PID or log file to consult — the desktop's EMBEDDED (in-process)
/// codex. Its caller has typically already seen the port accept TCP, but that
/// alone cannot distinguish "our app-server is up" from "an unrelated process
/// held the port first" (the embedded bind then fails and quietly retries in
/// its supervisor while the foreign listener answers the TCP wait) — only a
/// 2xx `/readyz` proves a codex app-server is the one serving.
pub fn wait_for_readyz(host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if matches!(probe_readyz(host, port), Probe::Ready) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Why [`spawn_ready`] did not yield a ready app-server.
///
/// Splits the two phases so callers keep their distinct handling: a spawn
/// error means no child was started at all (bad binary path, foreign process
/// on the port, unwritable state), while `NotReady` means a child was
/// spawned — or a listener adopted — but never verified, and carries the
/// [`SpawnReport`] so callers can still inspect/reap what was started.
#[derive(Debug)]
pub enum SpawnReadyError {
    /// [`crate::spawn`] itself failed; nothing was started.
    Spawn(pocket_codex_core::Error),

    /// The spawn succeeded but [`verify_ready`] did not: the app-server
    /// never answered `/readyz` (or provably died on boot).
    NotReady {
        /// What [`crate::spawn`] started or adopted — e.g. so a caller can
        /// reap a fresh child that is still coming up, or tell an adopted
        /// (`reused`) listener apart from its own spawn.
        report: Box<SpawnReport>,
        /// Why it never became ready, with this run's log tail.
        failure: Box<StartupFailure>,
    },
}

impl fmt::Display for SpawnReadyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(e) => e.fmt(f),
            Self::NotReady {
                failure, ..
            } => failure.fmt(f),
        }
    }
}

impl std::error::Error for SpawnReadyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Display already renders the inner error verbatim (transparent
        // wrapper), so expose the inner error's own source — not the inner
        // error itself — to keep error chains free of duplicate lines.
        match self {
            Self::Spawn(e) => e.source(),
            Self::NotReady {
                failure, ..
            } => failure.source(),
        }
    }
}

/// [`crate::spawn`] and [`verify_ready`] fused into one call: start (or
/// adopt) the app-server, then confirm it actually serves before reporting
/// success.
///
/// Every launch path wants this pairing — a bare `spawn` reports success
/// even for a child that died on boot (that is right for the supervisor's
/// honest-state bookkeeping, wrong for anything that tells a user "running"
/// or publishes the endpoint). Prefer this over hand-pairing the two calls,
/// which a new call site can silently forget.
pub fn spawn_ready(opts: SpawnOptions, timeout: Duration) -> Result<SpawnReport, SpawnReadyError> {
    let report = spawn(opts).map_err(SpawnReadyError::Spawn)?;
    match verify_ready(&report, timeout) {
        Ok(()) => Ok(report),
        Err(failure) => Err(SpawnReadyError::NotReady {
            report: Box::new(report),
            failure: Box::new(failure),
        }),
    }
}

/// Unix-socket verification: there is no HTTP endpoint to probe, and the
/// recorded PID may be a shim that legitimately exits after handoff, so the
/// only trustworthy fast-fail signal is the child being gone with the fatal
/// bind error already in this run's log. Watch for that briefly, then
/// report OK and leave deeper liveness to the supervisor-level checks.
fn verify_unix(report: &SpawnReport) -> Result<(), StartupFailure> {
    if report.reused {
        return Ok(());
    }
    let deadline = Instant::now() + UNIX_WATCH;
    loop {
        if !pid_running(report.info.pid)
            && bind_failure_logged(&report.info.log_file, report.log_offset)
        {
            return Err(failure(report, true));
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        thread::sleep(DEATH_CHECK_INTERVAL);
    }
}

/// Whether this run's log output already shows a fatal address-in-use bind
/// failure. Paired with a dead child PID this makes a launch *provably*
/// failed (a dead PID alone may be a shim that exited after handing off).
/// Also used by [`crate::spawn`]'s port-wait loop to stop waiting out its
/// full timeout on a bind that can never succeed.
pub(crate) fn bind_failure_logged(log_file: &Path, from_offset: u64) -> bool {
    mentions_addr_in_use(&read_log_tail(log_file, from_offset))
}

/// Assemble a [`StartupFailure`] from the spawn report and this run's log
/// output.
fn failure(report: &SpawnReport, process_exited: bool) -> StartupFailure {
    let log_tail = read_log_tail(&report.info.log_file, report.log_offset);
    StartupFailure {
        process_exited,
        port_in_use: mentions_addr_in_use(&log_tail),
        listen: report.info.listen.clone(),
        port: ws_host_port(&report.info.listen).map(|(_, port)| port),
        log_file: report.info.log_file.clone(),
        log_tail,
    }
}

/// One `/readyz` observation.
enum Probe {
    /// Answered 2xx — the server is up.
    Ready,
    /// A listener accepted but did not answer 2xx (still booting, or not a
    /// ready app-server yet).
    NotReady,
    /// Nothing accepted the connection.
    Down,
}

/// GET `/readyz` and classify the outcome. Every resolved address is tried
/// (a dual-stack hostname may serve on only one family): any 2xx wins, and
/// any accepting listener at all downgrades `Down` to `NotReady`.
fn probe_readyz(host: &str, port: u16) -> Probe {
    let host = probe_host(host);
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return Probe::Down;
    };
    let mut saw_listener = false;
    for addr in addrs {
        match probe_addr(addr, host, port) {
            Some(true) => return Probe::Ready,
            Some(false) => saw_listener = true,
            None => {},
        }
    }
    if saw_listener {
        Probe::NotReady
    } else {
        Probe::Down
    }
}

/// One `GET /readyz` exchange against a single address: `None` when nothing
/// accepted the connection, otherwise whether the answer was 2xx. A
/// hand-rolled one-line HTTP exchange keeps this crate free of an
/// HTTP-client dependency; the endpoint is loopback and only the status
/// line matters.
fn probe_addr(addr: SocketAddr, host: &str, port: u16) -> Option<bool> {
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let request =
        format!("GET /readyz HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Some(false);
    }
    let mut line = Vec::new();
    let _ = BufReader::new(stream.take(256)).read_until(b'\n', &mut line);
    Some(status_is_2xx(&String::from_utf8_lossy(&line)))
}

/// Whether an HTTP response starts with a 2xx status line.
fn status_is_2xx(response: &str) -> bool {
    let mut parts = response.split_whitespace();
    parts
        .next()
        .is_some_and(|version| version.starts_with("HTTP/"))
        && parts
            .next()
            .is_some_and(|code| code.len() == 3 && code.starts_with('2'))
}

/// Read the last lines this run wrote to `log_file` (from `from_offset`,
/// where [`crate::spawn`] recorded the pre-spawn length), capped to
/// [`TAIL_MAX_LINES`] / [`TAIL_MAX_BYTES`]. Best-effort: any I/O problem
/// yields an empty tail rather than masking the startup failure itself. A
/// stale offset past EOF (truncated/rotated file) reads as empty too.
fn read_log_tail(log_file: &Path, from_offset: u64) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(log_file) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = from_offset.max(len.saturating_sub(TAIL_MAX_BYTES));
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(TAIL_MAX_BYTES).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(TAIL_MAX_LINES)..]
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// Whether the log output points at an address-already-in-use bind failure.
/// Covers the raw OS codes (Windows WSAEADDRINUSE 10048, Linux 98, macOS 48 —
/// the numeric text survives even on localized Windows), the errno name an
/// npm/node shim prints, and the plain English phrasing.
fn mentions_addr_in_use(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("os error 10048")
            || lower.contains("os error 98")
            || lower.contains("os error 48")
            || lower.contains("eaddrinuse")
            || lower.contains("address already in use")
            || lower.contains("address in use")
    })
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use pocket_codex_core::state::CodexProcessInfo;

    use super::*;

    /// A one-shot fake `/readyz` endpoint answering with `status_line`.
    /// Returns the bound port; the listener thread serves a single request.
    fn fake_readyz(status_line: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake readyz");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || serve_one(&listener, status_line));
        port
    }

    /// Accept one connection and answer with `status_line`.
    fn serve_one(listener: &TcpListener, status_line: &str) {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            let _ =
                stream.write_all(format!("{status_line}\r\nContent-Length: 0\r\n\r\n").as_bytes());
        }
    }

    /// A [`SpawnReport`] with a confirmed listener — the common production
    /// case (spawn's port wait saw the port come up, or an adopted server).
    /// Tests for the unconfirmed path override `listener_confirmed` via
    /// struct update syntax.
    fn report(listen: String, pid: u32, reused: bool, log_file: PathBuf) -> SpawnReport {
        SpawnReport {
            info: CodexProcessInfo {
                pid,
                listen,
                log_file,
                started_at: String::new(),
            },
            reused,
            log_offset: 0,
            listener_confirmed: true,
        }
    }

    /// Unique temp path for a test log file.
    fn temp_log(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("pocket-codex-readiness-{tag}-{}.log", std::process::id()))
    }

    /// A fake endpoint that keeps serving `status_line` for every connection
    /// (a poll loop probes repeatedly, so a one-shot responder would read as
    /// "down" after its first answer). The thread leaks; fine for a test.
    fn fake_readyz_forever(status_line: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake readyz");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || loop {
            serve_one(&listener, status_line);
        });
        port
    }

    #[test]
    fn wait_for_readyz_passes_on_a_2xx() {
        let port = fake_readyz_forever("HTTP/1.1 200 OK");
        assert!(wait_for_readyz("127.0.0.1", port, Duration::from_secs(5)));
    }

    #[test]
    fn wait_for_readyz_rejects_a_foreign_listener() {
        // Something accepts on the port but is not a codex app-server (a plain
        // HTTP server 404s /readyz) — must time out false, never true.
        let port = fake_readyz_forever("HTTP/1.1 404 Not Found");
        assert!(!wait_for_readyz("127.0.0.1", port, Duration::from_millis(600)));
    }

    #[test]
    fn wait_for_readyz_rejects_a_dead_port() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr").port()
        };
        assert!(!wait_for_readyz("127.0.0.1", port, Duration::from_millis(400)));
    }

    #[test]
    fn verify_ready_passes_on_a_2xx_readyz() {
        let port = fake_readyz("HTTP/1.1 200 OK");
        let report = report(
            format!("ws://127.0.0.1:{port}"),
            std::process::id(),
            true,
            PathBuf::from("does-not-exist.log"),
        );
        assert!(verify_ready(&report, Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn verify_ready_fails_fast_on_a_dead_child_with_a_bind_error_logged() {
        // Learn a port nobody listens on.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr").port()
        };
        let log = temp_log("bindfail");
        std::fs::write(&log, "Error: Address already in use (os error 10048)\n")
            .expect("write log");
        // u32::MAX is above every platform's PID ceiling → "process is gone".
        let report = report(format!("ws://127.0.0.1:{port}"), u32::MAX, false, log.clone());
        let started = Instant::now();
        let failure =
            verify_ready(&report, Duration::from_secs(30)).expect_err("bind failure must fail");
        std::fs::remove_file(&log).ok();
        assert!(failure.process_exited);
        assert!(failure.port_in_use);
        assert_eq!(failure.port, Some(port));
        assert!(!failure.log_tail.is_empty());
        // Provably-failed launches must not wait out the (deliberately long)
        // overall timeout.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn verify_ready_survives_a_shim_handoff() {
        // The spawned PID is long dead and the log is silent (no bind error):
        // the launch must NOT be declared failed early — here the "native
        // binary" starts serving 700ms later and the verification succeeds.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake readyz");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(700));
            serve_one(&listener, "HTTP/1.1 200 OK");
        });
        // No log file at all → empty tail → no fatal-bind evidence.
        let report = report(
            format!("ws://127.0.0.1:{port}"),
            u32::MAX,
            false,
            PathBuf::from("does-not-exist.log"),
        );
        assert!(verify_ready(&report, Duration::from_secs(10)).is_ok());
    }

    #[test]
    fn verify_ready_shrinks_its_budget_when_spawn_never_saw_a_listener() {
        // spawn's port wait already ran dry (listener_confirmed = false) and
        // still nothing serves: verify_ready must cap its budget to the short
        // grace instead of re-waiting the full (here deliberately huge)
        // timeout on top of the wait spawn already burned.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr").port()
        };
        let unconfirmed = SpawnReport {
            listener_confirmed: false,
            ..report(
                format!("ws://127.0.0.1:{port}"),
                u32::MAX,
                false,
                PathBuf::from("does-not-exist.log"),
            )
        };
        let started = Instant::now();
        let failure = verify_ready(&unconfirmed, Duration::from_secs(60))
            .expect_err("nothing ever served — must fail");
        assert!(started.elapsed() < Duration::from_secs(10), "budget must be capped");
        assert!(failure.process_exited);
        assert!(!failure.port_in_use);
    }

    #[test]
    fn unconfirmed_grace_still_allows_a_late_shim_handoff() {
        // The capped budget must keep the shim-handoff semantics: a dead PID
        // with a silent log is not failure, and a native binary that binds
        // within the grace still verifies OK.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake readyz");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(700));
            serve_one(&listener, "HTTP/1.1 200 OK");
        });
        let unconfirmed = SpawnReport {
            listener_confirmed: false,
            ..report(
                format!("ws://127.0.0.1:{port}"),
                u32::MAX,
                false,
                PathBuf::from("does-not-exist.log"),
            )
        };
        assert!(verify_ready(&unconfirmed, Duration::from_secs(10)).is_ok());
    }

    #[test]
    fn verify_ready_reports_exited_at_timeout_for_a_silently_dead_child() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr").port()
        };
        let report = report(
            format!("ws://127.0.0.1:{port}"),
            u32::MAX,
            false,
            PathBuf::from("does-not-exist.log"),
        );
        let failure = verify_ready(&report, Duration::from_millis(400))
            .expect_err("nothing ever served — must fail at timeout");
        assert!(failure.process_exited, "dead child + silent port at timeout reads as exited");
        assert!(!failure.port_in_use);
    }

    #[test]
    fn verify_unix_fails_only_with_bind_error_evidence() {
        let log = temp_log("unix");
        std::fs::write(&log, "Error: Address already in use (os error 98)\n").expect("write log");
        let dead = report("unix:///tmp/pcx-test.sock".to_string(), u32::MAX, false, log.clone());
        let failure = verify_ready(&dead, Duration::from_secs(5)).expect_err("must fail");
        assert!(failure.process_exited && failure.port_in_use);
        assert_eq!(failure.port, None);
        std::fs::remove_file(&log).ok();

        // Dead PID with a silent log: could be a shim handoff — passes.
        let silent = report(
            "unix:///tmp/pcx-test.sock".to_string(),
            u32::MAX,
            false,
            PathBuf::from("does-not-exist.log"),
        );
        assert!(verify_ready(&silent, Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn diagnosis_renders_log_location_tail_and_hint() {
        let failure = StartupFailure {
            process_exited: true,
            port_in_use: true,
            listen: "ws://127.0.0.1:18080".to_string(),
            port: Some(18080),
            log_file: PathBuf::from("codex-app-server.log"),
            log_tail: vec!["Error: Address already in use (os error 10048)".to_string()],
        };
        let msg = failure.diagnosis("try the next port");
        assert!(msg.contains("exited during startup"));
        assert!(msg.contains("log: codex-app-server.log"));
        assert!(msg.contains("os error 10048"));
        assert!(msg.contains("hint: the listen port is already in use — try the next port"));
    }

    #[test]
    fn diagnosis_without_a_port_conflict_stays_hint_free() {
        let failure = StartupFailure {
            process_exited: false,
            port_in_use: false,
            listen: "ws://127.0.0.1:18080".to_string(),
            port: Some(18080),
            log_file: PathBuf::from("codex-app-server.log"),
            log_tail: Vec::new(),
        };
        let msg = failure.diagnosis("unused");
        assert!(msg.contains("did not become ready"));
        assert!(msg.contains("wrote no log output"));
        assert!(!msg.contains("hint:"));
    }

    #[test]
    fn status_line_classification() {
        assert!(status_is_2xx("HTTP/1.1 200 OK"));
        assert!(status_is_2xx("HTTP/1.0 204 No Content"));
        assert!(!status_is_2xx("HTTP/1.1 503 Service Unavailable"));
        assert!(!status_is_2xx("HTTP/1.1 404 Not Found"));
        assert!(!status_is_2xx("garbage"));
        assert!(!status_is_2xx(""));
    }

    #[test]
    fn addr_in_use_detection_matches_the_platform_phrasings() {
        let hit = |s: &str| mentions_addr_in_use(&[s.to_string()]);
        // Windows (WSAEADDRINUSE), including localized message text.
        assert!(hit("通常每个套接字地址只允许使用一次。 (os error 10048)"));
        // Linux / macOS errno.
        assert!(hit("Error: Address already in use (os error 98)"));
        // node shim.
        assert!(hit("Error: listen EADDRINUSE: address already in use 127.0.0.1:18080"));
        // Unrelated output stays quiet.
        assert!(!hit("codex app-server listening on ws://127.0.0.1:18080"));
        assert!(!hit("connection reset by peer (os error 10054)"));
    }

    #[test]
    fn log_tail_reads_only_this_runs_lines() {
        let path = temp_log("tail");
        let earlier_run = "old line from a previous run\n";
        let this_run = "boot line\n\nError: Address already in use (os error 10048)\n";
        std::fs::write(&path, format!("{earlier_run}{this_run}")).expect("write log");

        let tail = read_log_tail(&path, earlier_run.len() as u64);
        std::fs::remove_file(&path).ok();

        // Blank lines are dropped, earlier runs' lines are not replayed.
        assert_eq!(tail, vec![
            "boot line".to_string(),
            "Error: Address already in use (os error 10048)".to_string(),
        ]);
        assert!(mentions_addr_in_use(&tail));
    }

    #[test]
    fn log_tail_survives_a_missing_or_truncated_file() {
        assert!(read_log_tail(Path::new("definitely-not-here.log"), 0).is_empty());

        // Offset beyond EOF (file was truncated/rotated) → empty, no panic.
        let path = temp_log("trunc");
        std::fs::write(&path, "short\n").expect("write log");
        let tail = read_log_tail(&path, 10_000);
        std::fs::remove_file(&path).ok();
        assert!(tail.is_empty());
    }
}

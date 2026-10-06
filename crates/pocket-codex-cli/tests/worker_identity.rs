//! Stale runtime records must neither reuse nor signal an unrelated process.
#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration-test failures should fail at the observed boundary"
)]

use std::{
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use pocket_codex_core::{
    process::{pb_worker_identity, send_sigterm},
    state::{PbRole, PbSessionInfo, RuntimeState},
};

struct Fixture {
    root: PathBuf,
    children: Vec<Child>,
    workers: Vec<PbSessionInfo>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for worker in &self.workers {
            if pb_worker_identity(worker).is_some() {
                send_sigterm(worker.pid);
            }
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self {
            root: std::env::temp_dir()
                .join(format!("pcx-worker-identity-{name}-{}", std::process::id())),
            children: Vec::new(),
            workers: Vec::new(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pocket-codex"));
        command
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("MSG_HEADER_KEY", "0".repeat(32));
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        let output = self.command().args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output
    }

    fn entry(&self, pid: u32) -> PbSessionInfo {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        PbSessionInfo {
            role: PbRole::Subscribe,
            key: "identity-test".into(),
            local_addr: "127.0.0.1:0".into(),
            relay_addr: reservation.local_addr().unwrap().to_string(),
            pid,
            log_file: self.root.join("worker.log"),
            codec: false,
            started_at: "2026-10-03T00:00:00Z".into(),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state/pocket-codex/state.toml")
    }

    fn save(&self, entry: &PbSessionInfo) {
        RuntimeState {
            pb: vec![entry.clone()],
            ..Default::default()
        }
        .save_to(&self.state_path())
        .unwrap();
    }

    fn assert_stale(&self, entry: &PbSessionInfo) {
        let diagnostic =
            self.run(&["pb", "diagnostics", "--role", entry.role.as_str(), "--key", &entry.key]);
        let diagnostic: serde_json::Value = serde_json::from_slice(&diagnostic.stdout).unwrap();
        assert_eq!(diagnostic["alive"], false);
        assert!(diagnostic["runtime"].is_null());
        let status = self.run(&["status"]);
        assert!(String::from_utf8_lossy(&status.stdout).contains("stale"));
    }

    fn stop(&self, entry: &PbSessionInfo) {
        self.run(&["stop", "--role", entry.role.as_str(), "--key", &entry.key]);
        assert!(RuntimeState::load_from(&self.state_path())
            .unwrap()
            .pb
            .is_empty());
    }

    fn connect(&mut self, entry: &PbSessionInfo) -> PbSessionInfo {
        self.run(&[
            "connect",
            "--key",
            &entry.key,
            "--local-addr",
            &entry.local_addr,
            "--relay",
            &entry.relay_addr,
        ]);
        let state = RuntimeState::load_from(&self.state_path()).unwrap();
        let worker = state
            .find_pb(PbRole::Subscribe, &entry.key)
            .unwrap()
            .clone();
        if worker.pid != entry.pid {
            self.workers.push(worker.clone());
        }
        worker
    }

    fn spawn_worker(&mut self) -> PbSessionInfo {
        let mut entry = self.entry(0);
        entry.role = PbRole::Register;
        let child = self
            .command()
            .args([
                "__worker",
                "pb-register",
                "--key",
                &entry.key,
                "--local-addr",
                &entry.local_addr,
                "--relay",
                &entry.relay_addr,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        entry.pid = child.id();
        self.children.push(child);
        wait_until(|| pb_worker_identity(&entry).is_some());
        entry
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "process condition timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn unrelated_pid_is_stale_preserved_and_replaced_while_real_worker_is_reused() {
    let mut fixture = Fixture::new("reused-pid");
    let unrelated = Command::new("sleep").arg("60").spawn().unwrap();
    let entry = fixture.entry(unrelated.id());
    fixture.children.push(unrelated);
    fixture.save(&entry);
    fixture.assert_stale(&entry);
    fixture.stop(&entry);
    assert!(fixture.children[0].try_wait().unwrap().is_none(), "unrelated process was killed");

    fixture.save(&entry);
    let replacement = fixture.connect(&entry);
    assert_ne!(replacement.pid, entry.pid, "stale PID must not be reused");
    wait_until(|| pb_worker_identity(&replacement).is_some());
    let reused = fixture.connect(&replacement);
    assert_eq!(reused.pid, replacement.pid, "matching offline worker must be reused");
    assert!(fixture.children[0].try_wait().unwrap().is_none());
    fixture.stop(&replacement);
    wait_until(|| pb_worker_identity(&replacement).is_none());
}

#[test]
fn exited_unreaped_pid_is_replaced() {
    let mut fixture = Fixture::new("zombie");
    let child = Command::new("true").spawn().unwrap();
    let entry = fixture.entry(child.id());
    fixture.children.push(child);
    wait_until(|| {
        std::fs::read_to_string(format!("/proc/{}/stat", entry.pid))
            .unwrap()
            .rsplit_once(") ")
            .unwrap()
            .1
            .starts_with("Z ")
    });
    fixture.save(&entry);
    fixture.assert_stale(&entry);
    let replacement = fixture.connect(&entry);
    assert_ne!(replacement.pid, entry.pid, "zombie must not be reused");
    wait_until(|| pb_worker_identity(&replacement).is_some());
}

#[test]
fn mismatched_worker_and_its_thread_are_stale_and_never_signalled() {
    let mut fixture = Fixture::new("worker-thread");
    let worker = fixture.spawn_worker();
    let mut mismatched = Vec::new();
    let mut entry = worker.clone();
    entry.key.push_str("-other");
    mismatched.push(entry);
    let mut entry = worker.clone();
    entry.role = PbRole::Subscribe;
    mismatched.push(entry);
    let mut entry = worker.clone();
    entry.local_addr = "127.0.0.1:1".into();
    mismatched.push(entry);
    let mut entry = worker.clone();
    entry.relay_addr = "127.0.0.1:1".into();
    mismatched.push(entry);
    let mut thread = worker.clone();
    wait_until(|| {
        thread.pid = std::fs::read_dir(format!("/proc/{}/task", worker.pid))
            .unwrap()
            .find_map(|task| {
                let tid = task
                    .unwrap()
                    .file_name()
                    .to_str()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap();
                (tid != worker.pid).then_some(tid)
            })
            .unwrap_or(worker.pid);
        thread.pid != worker.pid
    });
    mismatched.push(thread);

    for entry in mismatched {
        fixture.save(&entry);
        fixture.assert_stale(&entry);
        assert!(pb_worker_identity(&entry).is_none());
        fixture.stop(&entry);
        assert!(fixture.children[0].try_wait().unwrap().is_none(), "valid worker was killed");
        assert!(pb_worker_identity(&worker).is_some());
    }
}

//! Process-level coverage for offline startup and scoped worker maintenance.
#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration-test failures should fail at the observed boundary"
)]

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use pocket_codex_core::state::{PbRole, PbSessionInfo, RuntimeState};

struct Fixture {
    root: PathBuf,
    children: Vec<Child>,
    pids: Vec<u32>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for pid in &self.pids {
            pocket_codex_core::process::send_sigterm(*pid);
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pocket-codex"));
        command
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("MSG_HEADER_KEY", "0".repeat(32));
        command
    }
}

#[test]
fn offline_worker_survives_readiness_deadline_and_restart_preserves_other_records() {
    let root = std::env::temp_dir().join(format!("pcx-worker-recovery-{}", std::process::id()));
    let mut fixture = Fixture {
        root,
        children: Vec::new(),
        pids: Vec::new(),
    };
    std::fs::create_dir_all(fixture.root.join("state/pocket-codex")).unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay = reservation.local_addr().unwrap().to_string();
    drop(reservation);
    let worker = fixture
        .command()
        .args([
            "__worker",
            "pb-register",
            "--key",
            "recovery-test",
            "--local-addr",
            "127.0.0.1:9",
            "--relay",
            &relay,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let old_pid = worker.id();
    fixture.children.push(worker);
    let entry = PbSessionInfo {
        role: PbRole::Register,
        key: "recovery-test".into(),
        local_addr: "127.0.0.1:9".into(),
        relay_addr: relay,
        pid: old_pid,
        log_file: fixture
            .root
            .join("state/pocket-codex/logs/pb-register-recovery-test.log"),
        codec: false,
        started_at: "2026-10-03T00:00:00Z".into(),
    };
    let mut sentinel = entry.clone();
    sentinel.key = "protected-host".into();
    sentinel.pid = std::process::id();
    let state = RuntimeState {
        pb: vec![entry.clone(), sentinel.clone()],
        ..Default::default()
    };
    let state_path = fixture.root.join("state/pocket-codex/state.toml");
    state.save_to(&state_path).unwrap();
    // Reproduces the old worker's unconditional exit after the 30-second ready
    // wait.
    let deadline = Instant::now() + Duration::from_secs(32);
    while Instant::now() < deadline {
        assert!(fixture.children[0].try_wait().unwrap().is_none(), "offline worker exited");
        std::thread::sleep(Duration::from_millis(100));
    }
    let diagnostic = fixture
        .command()
        .args(["pb", "diagnostics", "--role", "register", "--key", "recovery-test"])
        .output()
        .unwrap();
    assert!(diagnostic.status.success(), "{}", String::from_utf8_lossy(&diagnostic.stderr));
    let diagnostic: serde_json::Value = serde_json::from_slice(&diagnostic.stdout).unwrap();
    assert_eq!(diagnostic["runtime"]["sdk_version"], pocket_codex_pb::SDK_VERSION);
    assert_eq!(diagnostic["runtime"]["status"], "retrying");
    let refused = fixture
        .command()
        .args(["pb", "restart", "--role", "register", "--key", "protected-host"])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "must not signal a reused/nonworker PID");
    let restarted = fixture
        .command()
        .args(["pb", "restart", "--role", "register", "--key", "recovery-test"])
        .output()
        .unwrap();
    let state = RuntimeState::load_from(&state_path).unwrap();
    let replacement = state.find_pb(PbRole::Register, "recovery-test").unwrap();
    if replacement.pid != old_pid {
        fixture.pids.push(replacement.pid);
    }
    assert!(restarted.status.success(), "{}", String::from_utf8_lossy(&restarted.stderr));
    assert_ne!(replacement.pid, old_pid);
    assert_eq!(
        state
            .find_pb(PbRole::Register, "protected-host")
            .unwrap()
            .pid,
        sentinel.pid
    );
    assert_eq!(replacement.relay_addr, entry.relay_addr);
    assert!(pocket_codex_core::process::pid_running(replacement.pid));
}

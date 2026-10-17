//! Managed CLI ownership, cold-start choices and graceful process cleanup.
#![cfg(target_os = "linux")]
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration-test assertions")]

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

static NEXT_FIXTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    relay: String,
    children: Vec<Child>,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!("pcx-managed-host-{}-{sequence}", std::process::id()));
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay = reservation.local_addr().unwrap().to_string();
        let state = root.join("state/pocket-codex/managed-host/hosting");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(
            state.join("lifecycle.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "revision": 1,
                "entries": {"default": {"revision": 1, "runtime": null}},
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            root,
            relay,
            children: vec![],
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

    fn start(&mut self) -> u32 {
        self.start_with_device(Some("managed-test-device"))
    }

    fn start_with_device(&mut self, device: Option<&str>) -> u32 {
        let mut command = self.command();
        command.args(["serve", "--allow-remote-management"]);
        if let Some(device) = device {
            command.args(["--device", device]);
        }
        let child = command
            .args(["--relay", &self.relay, "--codex-binary", "/missing-codex-must-not-start"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(bytes) = std::fs::read(self.status()) {
                if let Ok(status) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if status["pid"] == pid {
                        assert!(status["services"].as_array().unwrap().is_empty());
                        assert_eq!(status["management"]["enabled"], true);
                        return pid;
                    }
                }
            }
            assert!(self
                .children
                .last_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_none());
            assert!(Instant::now() < deadline, "no managed-host status");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn status(&self) -> PathBuf {
        self.root
            .join("state/pocket-codex/managed-host/status.json")
    }

    fn stop(&mut self, pid: u32) {
        pocket_codex_core::process::send_sigterm(pid);
        let child = self
            .children
            .iter_mut()
            .find(|child| child.id() == pid)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(exit) = child.try_wait().unwrap() {
                assert!(exit.success());
                break;
            }
            assert!(Instant::now() < deadline, "SIGTERM did not stop the supervisor");
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(!self.status().exists());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn stopped_services_survive_restart_and_duplicate_owners_are_refused() {
    let mut fixture = Fixture::new();
    let pid = fixture.start();
    let status = fixture.command().arg("status").output().unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    assert!(String::from_utf8_lossy(&status.stdout).contains("computer management"));
    let duplicate = fixture
        .command()
        .args(["serve", "--allow-remote-management", "--relay", &fixture.relay])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("another managed-host supervisor"));
    fixture.stop(pid);
    let restarted = fixture.start();
    fixture.stop(restarted);
    let ledger: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            fixture
                .root
                .join("state/pocket-codex/managed-host/hosting/lifecycle.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(ledger["entries"]["default"]["revision"], 1);
    assert!(ledger["entries"]["default"]["runtime"].is_null());
}

#[test]
fn independent_installations_persist_distinct_defaults_across_restart() {
    fn identity(fixture: &Fixture) -> serde_json::Value {
        let file = std::fs::read_dir(fixture.root.join("state/pocket-codex/identity"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
    }
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    let first_pid = first.start_with_device(None);
    let second_pid = second.start_with_device(None);
    let before = identity(&first);
    let other = identity(&second);
    assert_ne!(before["id"], other["id"]);
    assert_ne!(before["device"], other["device"]);
    let id = before["id"].as_str().unwrap().replace('-', "");
    assert!(before["device"]
        .as_str()
        .unwrap()
        .ends_with(&format!(".{id}")));
    first.stop(first_pid);
    let restarted = first.start_with_device(None);
    assert_eq!(identity(&first), before);
    assert_eq!(identity(&second), other);
    first.stop(restarted);
    second.stop(second_pid);
}

#[test]
fn invalid_options_and_corrupt_restore_never_publish_management() {
    let fixture = Fixture::new();
    for arguments in [vec!["--host", "0.0.0.0"], vec!["--name", "  "], vec!["--key", "exact-key"]] {
        let result = fixture
            .command()
            .args(["serve", "--allow-remote-management", "--relay", &fixture.relay])
            .args(arguments)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!fixture.status().exists());
        assert!(!fixture
            .root
            .join("state/pocket-codex/managed-host/owner.lock")
            .exists());
    }
    std::fs::write(
        fixture
            .root
            .join("state/pocket-codex/managed-host/hosting/lifecycle.json"),
        b"invalid lifecycle contents",
    )
    .unwrap();
    let result = fixture
        .command()
        .args(["serve", "--allow-remote-management", "--relay", &fixture.relay])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!fixture.status().exists());
    assert!(!fixture
        .root
        .join("state/pocket-codex/managed-host/hosting/management.json")
        .exists());
}

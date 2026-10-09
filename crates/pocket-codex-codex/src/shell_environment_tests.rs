use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use super::*;

fn script(root: &Path, name: &str, body: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("chmod");
    path
}

fn environment(root: &Path) -> Vec<(OsString, OsString)> {
    vec![
        ("HOME".into(), root.into()),
        ("ZDOTDIR".into(), root.into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ]
}

#[test]
fn npm_codex_uses_shell_node_without_changing_controller_path() {
    let root = tempfile::tempdir().expect("tempdir");
    let node_dir = root.path().join("node with spaces");
    fs::create_dir(&node_dir).expect("node directory");
    script(&node_dir, "node", "printf node-ok");
    let codex = root.path().join("codex");
    fs::write(&codex, "#!/usr/bin/env node\n").expect("npm launcher");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).expect("chmod");
    let shell = script(
        root.path(),
        "shell",
        r#"
        [ "$1" = '-ilc' ] || exit 9
        printf 'startup noise\n'
        printf 'startup warning\n' >&2
        export PATH="$HOME:$HOME/node with spaces:/usr/bin:/bin"
        eval "$2"
    "#,
    );
    let inherited = root.path().join("empty-bin").into_os_string();
    let mut env = environment(root.path());
    env.push(("PATH".into(), inherited.clone()));
    let parent_path = std::env::var_os("PATH");
    let broken = Command::new(&codex)
        .env("PATH", &inherited)
        .output()
        .expect("launcher");
    assert_eq!(broken.status.code(), Some(127));
    let resolved =
        resolve_path(&shell, &env, Some(&inherited), Duration::from_secs(3)).expect("PATH");
    let located = crate::process::locate_on_path(Some(&resolved)).expect("find shell Codex");
    assert_eq!(located, codex);
    let repaired =
        crate::process::build_command(&located, "ws://127.0.0.1:1", &[], None, Some(&resolved))
            .output()
            .expect("launcher");
    assert!(repaired.status.success());
    assert_eq!(repaired.stdout, b"node-ok");
    assert_eq!(std::env::var_os("PATH"), parent_path);
    assert!(!resolved.to_string_lossy().contains("startup"));
    assert!(std::env::split_paths(&resolved).any(|entry| entry == node_dir));
}

#[test]
fn preserves_inherited_tool_order_and_deduplicates_shell_entries() {
    let path =
        merge_path(Some(OsStr::new("/chosen/node:/bin")), OsStr::new("/other/node:/bin::/usr/bin"))
            .expect("PATH");
    assert_eq!(path, "/chosen/node:/bin:/other/node:/usr/bin");
    assert!(merge_path(None, OsStr::new("::")).is_err());
}

#[test]
fn rejects_failed_missing_empty_and_oversized_shell_output() {
    let root = tempfile::tempdir().expect("tempdir");
    let env = environment(root.path());
    for body in [
        "exit 2",
        "exit 0",
        "printf '\\n' > \"$POCKET_CODEX_PATH_FILE\"",
        "/usr/bin/head -c 131072 /dev/zero > \"$POCKET_CODEX_PATH_FILE\"",
    ] {
        let shell = script(root.path(), "shell", body);
        assert!(resolve_path(&shell, &env, None, Duration::from_secs(3)).is_err(), "{body}");
    }
    assert!(resolve_path(Path::new("relative-shell"), &env, None, Duration::from_secs(1)).is_err());
    assert!(resolve_path(&root.path().join("absent"), &env, None, Duration::from_secs(1)).is_err());
}

fn assert_stopped(pid_file: &Path) {
    let pid = fs::read_to_string(pid_file).expect("child started before probe returned");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let output = Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .expect("process status");
        let status = String::from_utf8_lossy(&output.stdout);
        // Orphan reaping belongs to init; a zombie cannot execute or retain files.
        if !output.status.success() || status.trim().starts_with('Z') {
            return;
        }
        assert!(Instant::now() < deadline, "probe child {pid} still running: {status}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn timeout_kills_foreground_and_background_startup_children_only() {
    let root = tempfile::tempdir().expect("tempdir");
    script(root.path(), "helper", "echo $$ > \"$HOME/child.pid\"\nexec /bin/sleep 60");
    // A separate group must survive every cleanup.
    let sentinel = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .expect("sentinel");
    let mut sentinel = Probe(sentinel);
    for body in [
        "trap '' TERM\n\"$HOME/helper\"",
        "trap '' TERM\n/bin/sleep 60 &\necho $! > \"$HOME/child.pid\"\nwait",
        "echo $$ > \"$HOME/child.pid\"\ntrap '' TERM\nwhile :; do :; done",
    ] {
        let shell = script(root.path(), "shell", body);
        let start = Instant::now();
        let result =
            resolve_path(&shell, &environment(root.path()), None, Duration::from_millis(500));
        assert!(result
            .expect_err("timeout")
            .to_string()
            .contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_stopped(&root.path().join("child.pid"));
        assert!(sentinel.0.try_wait().expect("sentinel status").is_none());
        fs::remove_file(root.path().join("child.pid")).expect("remove pid");
    }
}

#[test]
fn successful_and_failed_shells_do_not_leave_background_children() {
    let root = tempfile::tempdir().expect("tempdir");
    for status in [0, 2] {
        let shell = script(
            root.path(),
            "shell",
            &format!(
                r#"
            /bin/sleep 60 &
            echo $! > "$HOME/child.pid"
            /usr/bin/printenv PATH > "$POCKET_CODEX_PATH_FILE"
            exit {status}
        "#
            ),
        );
        let result = resolve_path(&shell, &environment(root.path()), None, Duration::from_secs(3));
        assert_eq!(result.is_ok(), status == 0);
        assert_stopped(&root.path().join("child.pid"));
    }
}

#[test]
fn real_zsh_startup_is_isolated_and_its_blocked_child_is_terminated() {
    let zsh = Path::new("/bin/zsh");
    if !zsh.exists() && !cfg!(target_os = "macos") {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(
        root.path().join(".zshrc"),
        "export PATH=\"$HOME/custom node:/usr/bin:/bin\"\nprint banner\n",
    )
    .expect("zshrc");
    let path = resolve_path(zsh, &environment(root.path()), None, Duration::from_secs(3))
        .expect("zsh PATH");
    assert_eq!(std::env::split_paths(&path).next(), Some(root.path().join("custom node")));
    fs::write(root.path().join(".zshrc"), "/bin/sleep 60 &\necho $! > \"$HOME/child.pid\"\nwait\n")
        .expect("blocking zshrc");
    assert!(resolve_path(zsh, &environment(root.path()), None, Duration::from_millis(500)).is_err());
    assert_stopped(&root.path().join("child.pid"));
}

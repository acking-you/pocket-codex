//! Desktop [`TerminalLauncher`]: run an agent's own login command in a visible
//! terminal (TRD §4.4.1, D8). A wrapper script records the exit code in the
//! status file the hub polls.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use pocket_codex_host_svc::acp::{AcpError, TerminalLaunch, TerminalLauncher};

/// Opens Terminal (macOS), `cmd.exe` (Windows) or the first available Linux
/// terminal emulator.
#[derive(Clone, Copy, Debug, Default)]
pub struct DesktopTerminal;

/// POSIX single quotes (`'` → `'\''`).
pub fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `cmd.exe` double quotes, escaping `"`, `%` and `^`.
pub fn cmd_quote(value: &str) -> String {
    let escaped = value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// The wrapper script for `launch` writing its exit code to `status`.
pub fn script(launch: &TerminalLaunch, status: &Path, windows: bool) -> String {
    if windows {
        let mut out = String::from("@echo off\r\n");
        for (k, v) in &launch.env {
            let escaped = v.replace('^', "^^").replace('%', "%%").replace('"', "\"\"");
            out.push_str(&format!("set \"{k}={escaped}\"\r\n"));
        }
        let mut line = cmd_quote(&launch.program.to_string_lossy());
        for arg in &launch.args {
            line.push(' ');
            line.push_str(&cmd_quote(arg));
        }
        out.push_str(&line);
        out.push_str("\r\n");
        out.push_str(&format!("echo %ERRORLEVEL% > {}\r\n", cmd_quote(&status.to_string_lossy())));
        out
    } else {
        let mut out = String::from("#!/bin/sh\n");
        for (k, v) in &launch.env {
            out.push_str(&format!("export {k}={}\n", sh_quote(v)));
        }
        let mut line = sh_quote(&launch.program.to_string_lossy());
        for arg in &launch.args {
            line.push(' ');
            line.push_str(&sh_quote(arg));
        }
        out.push_str(&line);
        out.push('\n');
        out.push_str(&format!(
            "code=$?\nprintf '%s' \"$code\" > {}\n",
            sh_quote(&status.to_string_lossy())
        ));
        out.push_str(
            "echo\necho \"Login finished (exit code $code). You can close this window.\"\n",
        );
        out
    }
}

fn write_script(path: &Path, body: &str) -> Result<(), AcpError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    let mut file = options.open(path)?;
    file.write_all(body.as_bytes())?;
    Ok(())
}

fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

impl TerminalLauncher for DesktopTerminal {
    fn open(&self, launch: &TerminalLaunch, status_file: &Path) -> Result<(), AcpError> {
        let dir = status_file
            .parent()
            .ok_or_else(|| AcpError::Io("no run directory".into()))?;
        std::fs::create_dir_all(dir)?;
        let stem = status_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let windows = cfg!(windows);
        let extension = if windows {
            "cmd"
        } else if cfg!(target_os = "macos") {
            "command"
        } else {
            "sh"
        };
        let path = dir.join(format!("{stem}.{extension}"));
        write_script(&path, &script(launch, status_file, windows))?;
        let spawn = |program: &str, args: &[&str]| -> Result<(), AcpError> {
            Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|e| AcpError::Io(format!("starting {program}: {e}")))
        };
        let script_path = path.to_string_lossy().into_owned();
        if cfg!(target_os = "macos") {
            return spawn("open", &["-a", "Terminal", &script_path]);
        }
        if windows {
            return spawn("cmd.exe", &[
                "/c",
                "start",
                "Pocket-Codex login",
                "cmd.exe",
                "/k",
                &script_path,
            ]);
        }
        let candidates: [(&str, &[&str]); 5] = [
            ("x-terminal-emulator", &["-e"]),
            ("gnome-terminal", &["--"]),
            ("konsole", &["-e"]),
            ("xfce4-terminal", &["-x"]),
            ("xterm", &["-e"]),
        ];
        for (terminal, prefix) in candidates {
            if on_path(terminal).is_some() {
                let mut args: Vec<&str> = prefix.to_vec();
                args.push(&script_path);
                return spawn(terminal, &args);
            }
        }
        Err(AcpError::NoTerminal {
            command: launch.command_line(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn scripts_quote_every_argument() {
        let launch = TerminalLaunch {
            program: PathBuf::from("/opt/my agent/bin/node"),
            args: vec!["it's".into(), "--x=$HOME".into()],
            env: BTreeMap::from([("TOKEN".to_string(), "a'b".to_string())]),
            title: "t".into(),
        };
        let posix = script(&launch, Path::new("/tmp/s.status"), false);
        assert!(posix.contains(r"export TOKEN='a'\''b'"));
        assert!(posix.contains(r"'/opt/my agent/bin/node' 'it'\''s' '--x=$HOME'"));
        assert!(posix.contains("> '/tmp/s.status'"));
        let win = script(&launch, Path::new(r"C:\run\s.status"), true);
        assert!(win.contains("set \"TOKEN=a'b\""));
        assert_eq!(cmd_quote("100% \"x\" ^"), "\"100%% \"\"x\"\" ^^\"");
    }
}

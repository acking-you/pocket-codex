//! What to launch: an agent profile (identity + display name) plus the
//! executable and its argument vector.
//!
//! The program and arguments are passed to the operating system exactly as
//! given — no shell, no splitting, no interpolation — so a path containing
//! spaces is valid and an argument containing `$HOME` or `;` stays literal.
//! Arguments may contain secrets, so [`AgentSpec`]'s `Debug` output and every
//! log line show only their count.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Most arguments accepted.
pub const MAX_ARGS: usize = 64;
/// Longest program path or single argument, in bytes.
pub const MAX_ARG_BYTES: usize = 4096;

/// One configured agent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    /// Stable profile id (a preset id such as `opencode`, or a custom id).
    /// Persisted session associations are only reused for the same id.
    pub profile_id: String,
    /// Name shown in the UI.
    pub display_name: String,
    /// Executable: an explicit path, or a bare name searched on `PATH`.
    pub program: String,
    /// Arguments, passed verbatim.
    pub args: Vec<String>,
}

impl fmt::Debug for AgentSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSpec")
            .field("profile_id", &self.profile_id)
            .field("display_name", &self.display_name)
            .field("args", &format_args!("[{} redacted]", self.args.len()))
            .finish_non_exhaustive()
    }
}

/// Why a spec or its program is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// The profile id is empty, too long, or not `[A-Za-z0-9._-]`.
    #[error("the agent profile id is invalid")]
    ProfileId,
    /// The display name is empty, too long or contains control characters.
    #[error("the agent name is invalid")]
    DisplayName,
    /// The program is empty, too long, or contains NUL.
    #[error("the agent executable is invalid")]
    Program,
    /// Too many arguments, or one is too long or contains NUL.
    #[error("the agent arguments are invalid")]
    Args,
    /// The program could not be found.
    #[error("the agent executable was not found")]
    NotFound,
}

/// A built-in starting point for a well-known agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Profile id.
    pub id: &'static str,
    /// Display name.
    pub display_name: &'static str,
    /// Bare executable name.
    pub program: &'static str,
    /// Default arguments.
    pub args: &'static [&'static str],
    /// Install locations, relative to the home directory, searched after
    /// `PATH`.
    pub fallback_dirs: &'static [&'static str],
}

/// Built-in presets. OpenCode documents `opencode acp` as its stdio ACP
/// agent; nothing else about its ACP feature set is assumed.
pub const PRESETS: &[Preset] = &[Preset {
    id: "opencode",
    display_name: "OpenCode",
    program: "opencode",
    args: &["acp"],
    fallback_dirs: &[".opencode/bin"],
}];

/// The preset with `id`.
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

impl Preset {
    /// This preset as a spec.
    pub fn spec(&self) -> AgentSpec {
        AgentSpec {
            profile_id: self.id.to_string(),
            display_name: self.display_name.to_string(),
            program: self.program.to_string(),
            args: self.args.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}

fn valid_text(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.contains('\0')
}

impl AgentSpec {
    /// Check sizes and characters. Does not touch the filesystem.
    pub fn validate(&self) -> Result<(), SpecError> {
        let id = &self.profile_id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(SpecError::ProfileId);
        }
        let name = self.display_name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(SpecError::DisplayName);
        }
        if !valid_text(&self.program, MAX_ARG_BYTES) || self.program.trim().is_empty() {
            return Err(SpecError::Program);
        }
        if self.args.len() > MAX_ARGS
            || self
                .args
                .iter()
                .any(|arg| arg.len() > MAX_ARG_BYTES || arg.contains('\0'))
        {
            return Err(SpecError::Args);
        }
        Ok(())
    }
}

fn is_explicit_path(program: &str) -> bool {
    program.contains('/') || (cfg!(windows) && program.contains('\\'))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Resolve the executable to launch.
///
/// An explicit path (anything containing a path separator) is used as is and
/// never replaced by a search result: a wrong explicit choice is an error.
/// A bare name is searched in `search_path` (the inherited `PATH`, followed
/// by any extra directories the caller adds), then in the preset's install
/// locations under `home`.
pub fn resolve_program(
    program: &str,
    search_path: Option<&OsString>,
    home: Option<&Path>,
    fallback_dirs: &[&str],
) -> Result<PathBuf, SpecError> {
    if is_explicit_path(program) {
        let path = PathBuf::from(program);
        return if path.is_absolute() && is_executable(&path) {
            Ok(path)
        } else {
            Err(SpecError::NotFound)
        };
    }
    let inherited = std::env::var_os("PATH");
    let search = search_path.or(inherited.as_ref());
    let from_path = search.and_then(|paths| {
        std::env::split_paths(paths)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(program))
            .find(|candidate| is_executable(candidate))
    });
    from_path
        .or_else(|| {
            let home = home?;
            fallback_dirs
                .iter()
                .map(|dir| home.join(dir).join(program))
                .find(|candidate| is_executable(candidate))
        })
        .ok_or(SpecError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(program: &str, args: &[&str]) -> AgentSpec {
        AgentSpec {
            profile_id: "custom-1".into(),
            display_name: "My agent".into(),
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn debug_output_never_shows_arguments() {
        let shown = format!("{:?}", spec("/bin/agent", &["--token", "s3cr3t"]));
        assert!(!shown.contains("s3cr3t"));
        assert!(shown.contains("2 redacted"));
    }

    #[test]
    fn validation_bounds_without_rejecting_spaces() {
        assert!(spec("/Applications/My Agent/agent", &["--flag=a b", "$HOME", ";"])
            .validate()
            .is_ok());
        assert_eq!(spec("", &[]).validate(), Err(SpecError::Program));
        assert_eq!(spec("a\0b", &[]).validate(), Err(SpecError::Program));
        assert_eq!(spec("a", &["x\0"]).validate(), Err(SpecError::Args));
        let many: Vec<&str> = vec!["a"; MAX_ARGS + 1];
        assert_eq!(spec("a", &many).validate(), Err(SpecError::Args));
        let mut bad = spec("a", &[]);
        bad.profile_id = "../x".into();
        assert_eq!(bad.validate(), Err(SpecError::ProfileId));
    }

    #[test]
    fn the_opencode_preset_is_opencode_acp() {
        let preset = preset("opencode").expect("preset");
        let spec = preset.spec();
        assert_eq!(spec.program, "opencode");
        assert_eq!(spec.args, vec!["acp".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn resolution_prefers_path_and_never_replaces_an_explicit_choice() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let spaced = dir.path().join("with space");
        let home = dir.path().join("home");
        std::fs::create_dir_all(spaced.join("bin")).expect("dirs");
        std::fs::create_dir_all(home.join(".tool/bin")).expect("dirs");
        for exe in [spaced.join("bin/agent"), home.join(".tool/bin/agent")] {
            std::fs::write(&exe, b"#!/bin/sh\n").expect("write");
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let path = std::env::join_paths([spaced.join("bin")]).expect("path");
        assert_eq!(
            resolve_program("agent", Some(&path), Some(&home), &[".tool/bin"]),
            Ok(spaced.join("bin/agent"))
        );
        let empty = OsString::from("/definitely/not/here");
        assert_eq!(
            resolve_program("agent", Some(&empty), Some(&home), &[".tool/bin"]),
            Ok(home.join(".tool/bin/agent"))
        );
        let explicit = spaced.join("bin/agent");
        assert_eq!(
            resolve_program(explicit.to_str().expect("utf8"), None, None, &[]),
            Ok(explicit.clone())
        );
        assert_eq!(
            resolve_program("/definitely/not/here/agent", Some(&path), Some(&home), &[".tool/bin"]),
            Err(SpecError::NotFound)
        );
        assert_eq!(resolve_program("./agent", Some(&path), None, &[]), Err(SpecError::NotFound));
    }
}

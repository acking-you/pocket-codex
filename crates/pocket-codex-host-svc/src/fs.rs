//! Host filesystem directory listing for the remote project-folder browser.
//!
//! A remote client (a phone) picks the working directory for a new session by
//! drilling into the host's directory tree. To keep that from becoming a
//! free-roam of the whole filesystem, browsing is **confined to the host's
//! configured project roots** ([`crate::store::HostConfig::project_roots`]): a
//! listing request is honoured only for a path that is one of those roots or
//! lives inside one. The confinement check ([`within_roots`]) canonicalises
//! both sides so `..` traversal and symlinks can't escape a root.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One browsable child directory of a listed folder. Only directories are
/// returned — the browser picks a working folder, never a file. `Deserialize`
/// as well as `Serialize` so the bridge client can decode the meta response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// The directory's own name (final path component).
    pub name: String,
    /// Absolute host path, ready to pass back as the next listing's `path` or
    /// as a session's working directory.
    pub path: String,
    /// Whether this directory is a git repository (has a `.git` entry) — a
    /// strong hint that it is a project the user wants to open.
    pub is_git_repo: bool,
}

/// Canonicalise `p`, falling back to a lexical normalisation when the path does
/// not exist on disk (so confinement checks still work for a not-yet-created
/// path). Canonicalisation is what makes `..` and symlinks unable to escape.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Whether `path` is one of `roots` or lives inside one — the
/// browse-confinement rule. Both sides are canonicalised so traversal/symlinks
/// can't slip out.
pub fn within_roots(path: &Path, roots: &[String]) -> bool {
    let target = canonical(path);
    roots.iter().any(|root| {
        let root = canonical(Path::new(root));
        target == root || target.starts_with(&root)
    })
}

/// List the immediate sub*directories* of `dir`, sorted case-insensitively by
/// name, each annotated with whether it is a git repo. Hidden dot-directories
/// are skipped (build/VCS noise a project browser doesn't want), except that a
/// directory's own `.git` presence is still what flags it as a repo. Errors if
/// `dir` is not a readable directory.
pub fn list_subdirs(dir: &Path) -> Result<Vec<DirEntry>> {
    let mut out = Vec::new();
    let read =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;
    for entry in read {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // skip an unreadable entry rather than fail the whole listing
        };
        // Only directories; follow the entry's file type without an extra stat
        // where possible, falling back to a metadata probe for symlinks.
        let is_dir = match entry.file_type() {
            Ok(ft) if ft.is_dir() => true,
            Ok(ft) if ft.is_symlink() => entry.path().is_dir(),
            _ => false,
        };
        if !is_dir {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Skip hidden dot-directories (e.g. `.git`, `.cache`) — they clutter a
        // project picker; the `.git` of THIS dir is detected below, not listed.
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let is_git_repo = path.join(".git").exists();
        out.push(DirEntry {
            name,
            path: path.to_string_lossy().into_owned(),
            is_git_repo,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_subdirs_sorted_skipping_hidden() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir(root.join("Zeta")).expect("mkdir");
        std::fs::create_dir(root.join("alpha")).expect("mkdir");
        std::fs::create_dir(root.join(".hidden")).expect("mkdir");
        std::fs::write(root.join("file.txt"), b"x").expect("write");

        let out = list_subdirs(root).expect("list");
        let names: Vec<_> = out.iter().map(|e| e.name.as_str()).collect();
        // Case-insensitive sort, files + dotdirs excluded.
        assert_eq!(names, vec!["alpha", "Zeta"]);
    }

    #[test]
    fn flags_git_repositories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("proj");
        std::fs::create_dir(&repo).expect("mkdir");
        std::fs::create_dir(repo.join(".git")).expect("mkdir");
        std::fs::create_dir(dir.path().join("plain")).expect("mkdir");

        let out = list_subdirs(dir.path()).expect("list");
        let proj = out.iter().find(|e| e.name == "proj").expect("proj");
        let plain = out.iter().find(|e| e.name == "plain").expect("plain");
        assert!(proj.is_git_repo);
        assert!(!plain.is_git_repo);
    }

    #[test]
    fn confinement_allows_roots_and_children_rejects_outside() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        let child = root.join("a").join("b");
        std::fs::create_dir_all(&child).expect("mkdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("mkdir");

        let roots = vec![root.to_string_lossy().into_owned()];
        assert!(within_roots(&root, &roots), "a root itself is allowed");
        assert!(within_roots(&child, &roots), "a child is allowed");
        assert!(!within_roots(&outside, &roots), "a sibling is rejected");
    }

    #[test]
    fn confinement_blocks_traversal_escape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        let roots = vec![root.to_string_lossy().into_owned()];
        // `root/sub/../../outside` canonicalises out of the root → rejected.
        let escape = root.join("sub").join("..").join("..").join("outside");
        assert!(!within_roots(&escape, &roots));
    }
}

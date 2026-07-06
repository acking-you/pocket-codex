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

/// One file in a listed directory (not a sub-directory), with size + mtime for
/// the file-transfer panel. `Deserialize` too so the bridge client decodes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// The file's own name (final path component).
    pub name: String,
    /// Absolute host path.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified time in unix seconds (0 when unavailable).
    pub mtime: i64,
}

/// Canonicalise `p`, or `None` when it does not exist / cannot be resolved.
fn canonical(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

/// Whether `path` is one of `roots` or lives inside one — the
/// browse-confinement rule.
///
/// This endpoint only ever lists **existing** directories, so a path that
/// cannot be canonicalised is rejected outright. That is also what closes the
/// path-traversal hole: a non-existent `root/sub/../../outside` fails to
/// canonicalise (instead of falling back to a raw path whose `..` a plain
/// `starts_with(root)` would miss — a real escape, reproduced on Linux where
/// the fallback kept the literal components), while an existing traversal path
/// is fully resolved first, so the `starts_with` check runs against the REAL
/// target with symlinks and `..` already collapsed.
pub fn within_roots(path: &Path, roots: &[String]) -> bool {
    let Some(target) = canonical(path) else {
        return false;
    };
    roots.iter().any(|root| {
        canonical(Path::new(root)).is_some_and(|root| target == root || target.starts_with(&root))
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

/// List the immediate *files* of `dir` (not sub-directories), sorted
/// case-insensitively by name, each with size + mtime. Hidden dotfiles are
/// skipped. Errors if `dir` is not a readable directory.
pub fn list_files(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    let read =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;
    for entry in read {
        let Ok(entry) = entry else {
            continue; // skip an unreadable entry rather than fail the listing
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(FileEntry {
            name,
            path: entry.path().to_string_lossy().into_owned(),
            size: meta.len(),
            mtime,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_files_sorted_skipping_hidden() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("Beta.txt"), b"bb").expect("write");
        std::fs::write(root.join("alpha.md"), b"a").expect("write");
        std::fs::write(root.join(".hidden"), b"x").expect("write");
        std::fs::create_dir(root.join("subdir")).expect("mkdir");

        let out = list_files(root).expect("list");
        let names: Vec<_> = out.iter().map(|e| e.name.as_str()).collect();
        // Case-insensitive sort; dirs + dotfiles excluded.
        assert_eq!(names, vec!["alpha.md", "Beta.txt"]);
        let a = out.iter().find(|e| e.name == "alpha.md").expect("alpha");
        assert_eq!(a.size, 1);
    }

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
        // `root/sub/../../outside` (non-existent target) can't canonicalise →
        // rejected, so it can't slip past the leading-`root` prefix.
        let escape = root.join("sub").join("..").join("..").join("outside");
        assert!(!within_roots(&escape, &roots));
    }

    #[test]
    fn confinement_rejects_nonexistent_and_allows_resolved_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("a").join("b")).expect("mkdir");
        let roots = vec![root.to_string_lossy().into_owned()];

        // A path that doesn't exist is rejected (only existing dirs are listed).
        assert!(!within_roots(&root.join("missing"), &roots));

        // An EXISTING traversal path that resolves back inside the root is
        // allowed — canonicalisation collapses the `..` before the check.
        let resolved = root.join("a").join("b").join("..");
        assert!(within_roots(&resolved, &roots));
    }
}

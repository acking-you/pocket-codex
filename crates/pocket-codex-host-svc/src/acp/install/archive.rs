//! Safe extraction of `.zip`, `.tar.gz` and `.tgz` archives (TRD §4.3.5).
//!
//! Rejected: absolute paths, `..`, drive prefixes, links resolving outside
//! `dest`, writing through a link, devices and FIFOs, more than 3 GiB or
//! 200 000 entries. Unix permission bits are kept without setuid/setgid.

use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use super::super::error::AcpError;

/// Total extracted size limit.
pub const MAX_TOTAL_BYTES: u64 = 3 * 1024 * 1024 * 1024;
/// Entry count limit.
pub const MAX_ENTRIES: usize = 200_000;

fn reject(message: impl Into<String>) -> AcpError {
    AcpError::ArchiveRejected(message.into())
}

/// Extract `archive` (by suffix) into `dest`.
pub fn extract(archive: &Path, dest: &Path) -> Result<(), AcpError> {
    extract_with_limits(archive, dest, MAX_TOTAL_BYTES, MAX_ENTRIES)
}

/// [`extract`] with explicit limits (tests use small ones).
pub fn extract_with_limits(
    archive: &Path,
    dest: &Path,
    max_bytes: u64,
    max_entries: usize,
) -> Result<(), AcpError> {
    std::fs::create_dir_all(dest)?;
    let name = archive.to_string_lossy().to_lowercase();
    let mut state = State::new(dest, max_bytes, max_entries)?;
    if name.ends_with(".zip") {
        extract_zip(archive, &mut state)?;
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar(archive, &mut state)?;
    } else {
        return Err(reject(format!("unsupported archive type: {}", archive.display())));
    }
    state.verify_links()
}

struct State {
    dest: PathBuf,
    real_dest: PathBuf,
    entries: usize,
    bytes: u64,
    links: Vec<PathBuf>,
    max_bytes: u64,
    max_entries: usize,
}

impl State {
    fn new(dest: &Path, max_bytes: u64, max_entries: usize) -> Result<Self, AcpError> {
        let real_dest = std::fs::canonicalize(dest)?;
        Ok(Self {
            dest: dest.to_path_buf(),
            real_dest,
            entries: 0,
            bytes: 0,
            links: Vec::new(),
            max_bytes,
            max_entries,
        })
    }

    fn count_entry(&mut self) -> Result<(), AcpError> {
        self.entries += 1;
        if self.entries > self.max_entries {
            return Err(reject("too many archive entries"));
        }
        Ok(())
    }

    /// Relative, normalized path of an entry name.
    fn relative(name: &str) -> Result<PathBuf, AcpError> {
        let name = name.replace('\\', "/");
        if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
            return Err(reject(format!("absolute path in archive: {name}")));
        }
        let mut out = PathBuf::new();
        for component in Path::new(&name).components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {},
                _ => return Err(reject(format!("unsafe path in archive: {name}"))),
            }
        }
        Ok(out)
    }

    /// Destination of `rel`; refuses to pass through an existing link.
    fn target(&self, rel: &Path) -> Result<PathBuf, AcpError> {
        let mut at = self.dest.clone();
        for part in rel.parent().into_iter().flat_map(Path::components) {
            at.push(part);
            if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(reject(format!("archive writes through a link: {}", rel.display())));
            }
        }
        Ok(self.dest.join(rel))
    }

    fn write_file(
        &mut self,
        rel: &Path,
        reader: &mut dyn Read,
        mode: Option<u32>,
    ) -> Result<(), AcpError> {
        let out = self.target(rel)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(&out).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(reject(format!("archive overwrites a link: {}", rel.display())));
        }
        let mut file = File::create(&out)?;
        let remaining = self.max_bytes.saturating_sub(self.bytes);
        let copied = std::io::copy(&mut reader.take(remaining + 1), &mut file)?;
        self.bytes += copied;
        if self.bytes > self.max_bytes {
            return Err(reject("archive expands beyond the size limit"));
        }
        file.flush()?;
        set_mode(&out, mode)?;
        Ok(())
    }

    fn make_dir(&self, rel: &Path, mode: Option<u32>) -> Result<(), AcpError> {
        let out = self.target(&rel.join("x"))?;
        let dir = out
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.dest.clone());
        std::fs::create_dir_all(&dir)?;
        set_mode(&dir, mode.map(|m| m | 0o700))?;
        Ok(())
    }

    /// Link target must stay inside `dest` lexically.
    fn check_link(&self, rel: &Path, target: &Path) -> Result<(), AcpError> {
        if target.is_absolute() || target.to_string_lossy().as_bytes().get(1) == Some(&b':') {
            return Err(reject(format!("absolute link target in archive: {}", rel.display())));
        }
        let mut depth: i64 = rel.parent().map_or(0, |p| p.components().count() as i64);
        for component in target.components() {
            match component {
                Component::ParentDir => depth -= 1,
                Component::Normal(_) => depth += 1,
                Component::CurDir => {},
                _ => return Err(reject(format!("unsafe link target: {}", rel.display()))),
            }
            if depth < 0 {
                return Err(reject(format!("link escapes the archive: {}", rel.display())));
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    fn symlink(&mut self, rel: &Path, target: &Path) -> Result<(), AcpError> {
        self.check_link(rel, target)?;
        let out = self.target(rel)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, &out)?;
        self.links.push(out);
        Ok(())
    }

    #[cfg(not(unix))]
    fn symlink(&mut self, rel: &Path, target: &Path) -> Result<(), AcpError> {
        self.check_link(rel, target)?;
        Err(reject(format!("symbolic links are not supported here: {}", rel.display())))
    }

    fn hard_link(&self, rel: &Path, target: &Path) -> Result<(), AcpError> {
        let source_rel = Self::relative(&target.to_string_lossy())?;
        let source = self.target(&source_rel)?;
        let out = self.target(rel)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let real =
            std::fs::canonicalize(&source).map_err(|_| reject("hard link to a missing file"))?;
        if !real.starts_with(&self.real_dest) {
            return Err(reject(format!("hard link escapes the archive: {}", rel.display())));
        }
        std::fs::hard_link(&real, &out)?;
        Ok(())
    }

    /// Every created link must resolve inside `dest` (or dangle inside it).
    fn verify_links(&self) -> Result<(), AcpError> {
        for link in &self.links {
            if let Ok(real) = std::fs::canonicalize(link) {
                if !real.starts_with(&self.real_dest) {
                    return Err(reject(format!(
                        "link resolves outside the archive: {}",
                        link.display()
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), AcpError> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> Result<(), AcpError> {
    Ok(())
}

fn extract_tar(archive: &Path, state: &mut State) -> Result<(), AcpError> {
    let file = File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let entries = tar
        .entries()
        .map_err(|e| reject(format!("reading archive: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| reject(format!("reading archive: {e}")))?;
        state.count_entry()?;
        let raw = entry
            .path()
            .map_err(|e| reject(format!("bad entry path: {e}")))?;
        let rel = State::relative(&raw.to_string_lossy())?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let mode = entry.header().mode().ok();
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            state.make_dir(&rel, mode)?;
        } else if kind.is_file() || kind == tar::EntryType::Continuous {
            state.write_file(&rel, &mut entry, mode)?;
        } else if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(|e| reject(format!("bad link: {e}")))?
                .ok_or_else(|| reject("link without a target"))?
                .into_owned();
            state.symlink(&rel, &target)?;
        } else if kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|e| reject(format!("bad link: {e}")))?
                .ok_or_else(|| reject("link without a target"))?
                .into_owned();
            state.hard_link(&rel, &target)?;
        } else if kind.is_pax_global_extensions()
            || kind.is_pax_local_extensions()
            || kind.is_gnu_longname()
            || kind.is_gnu_longlink()
        {
            continue;
        } else {
            return Err(reject(format!("unsupported archive entry: {}", rel.display())));
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, state: &mut State) -> Result<(), AcpError> {
    let file = File::open(archive)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| reject(format!("reading archive: {e}")))?;
    if zip.len() > state.max_entries {
        return Err(reject("too many archive entries"));
    }
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| reject(format!("reading archive: {e}")))?;
        state.count_entry()?;
        let rel = State::relative(entry.name())?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let mode = entry.unix_mode();
        let is_link = mode.is_some_and(|m| m & 0o170000 == 0o120000);
        if entry.is_dir() {
            state.make_dir(&rel, mode)?;
        } else if is_link {
            let mut target = String::new();
            entry.by_ref().take(4096).read_to_string(&mut target)?;
            state.symlink(&rel, Path::new(&target))?;
        } else {
            state.write_file(&rel, &mut entry, mode)?;
        }
    }
    Ok(())
}

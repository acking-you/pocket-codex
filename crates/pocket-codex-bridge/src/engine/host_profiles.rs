//! The saved configuration of each local host instance: its model service
//! and, optionally, its desired runtime.
//!
//! Per canonical instance name (see [`super::host_names`]): where the host
//! gets its model service (the agent's existing configuration, the official
//! account, or an instance-owned custom provider) with, for a custom
//! provider, its protocol, base URL, optional default model and key; and the
//! runtime it should run with (a Codex binary, port and proxy, or an ACP
//! program and arguments). Saving never touches a running host; see
//! [`super::host_apply`] for applying a saved revision.
//!
//! # Storage
//!
//! `hosting/profiles.json` holds the records; each key is its own file,
//! `hosting/keys/<hex(name)>.<generation>.key`, named by the record that
//! uses it. A save writes a new key file first, then replaces the record
//! file atomically (temporary file + rename), and only then removes key
//! files neither the record nor an unfinished staged save (its pin in the
//! record file) names. A pin is released only by its own stage, never by
//! age, so a crashed stage leaves its key files behind rather than risking
//! one a rollback still needs. Cleanup runs only under the lock, after a
//! successful read of the record file and after that record file was
//! durably written; a record whose directory could not be synced counts as
//! committed (readers see it) but removes no key. A new key file that could
//! not be made durable fails its save before any record names it. Keys are
//! one printable ASCII token (no whitespace, quotes or backslashes). Files
//! are `0600` and directories `0700` on Unix; elsewhere they rely on the
//! per-user state directory. Writers in several processes are serialised by
//! a lock file (`hosting/profiles.lock`) held only around file access, never
//! across a probe or a start. A read takes it only once the record file
//! exists; a read that found no record file answers that absent snapshot,
//! never a record that appeared after it looked.
//!
//! A record that cannot be read faithfully (unknown source or protocol,
//! invalid URL, missing key file) is reported as such and never resolved to
//! another provider; saving over it replaces it.
//!
//! # Authority
//!
//! Every non-inherited record carries an opaque `authority` generation that
//! changes whenever the source, protocol, URL, model or key does. ACP
//! session records are keyed by it, so history made through one account or
//! provider is never adopted through another; a runtime-only save keeps it.
//! An instance without a record inherits its agent's configuration, exactly
//! as every host did before, so old restore records keep working unchanged.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use anyhow::{anyhow, bail, Context, Result};
use pocket_codex_codex::provider_launch::{codex_launch, CodexLaunch, CodexProviderMode};
use pocket_codex_core::provider::{validate_base_url, ApiProtocol, ProviderEndpoint, Secret};
use pocket_codex_host_svc::acp::{provider as acp_provider, AgentSpec};
use serde::{Deserialize, Serialize};

use super::host_names;

#[path = "host_profiles_environment.rs"]
pub mod environment;
#[path = "host_profiles_files.rs"]
pub(super) mod files;
#[path = "host_profiles_opencode.rs"]
pub(super) mod opencode;
#[path = "host_profiles_secrets.rs"]
mod secrets;

use self::files::{key_file_name, key_path, valid_key_ref, write_private, Committed, FileLock};
pub use self::secrets::{redact, redactor, Redactor};

/// The current file version; version 1 (no key references) is still read.
const FILE_VERSION: u32 = 2;

#[path = "host_profiles_types.rs"]
mod types;
pub use types::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claude_env: Option<String>,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// The key file this record uses (a file name under `keys/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime: Option<Runtime>,
    revision: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Profiles {
    version: u32,
    #[serde(default)]
    hosts: BTreeMap<String, Record>,
    /// Key files of unfinished staged saves, by stage id: cleanup keeps
    /// them, so a rollback can always put its previous record back.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pins: BTreeMap<String, Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pin {
    name: String,
    keys: Vec<String>,
    /// Seconds since the Unix epoch, for diagnosis only: a pin never
    /// expires (see the module docs).
    at: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Serialises file access within this process (the lock file serialises
/// processes). Held only around file access.
static STORE: Mutex<()> = Mutex::new(());

/// Both locks, for one read-modify-write.
struct Locked {
    file: Option<FileLock>,
    _process: MutexGuard<'static, ()>,
}

impl Locked {
    /// The records as this lock may see them: without the file lock (a
    /// read that found no record file) the absent snapshot it observed,
    /// never a record file that appeared after it looked, whose key another
    /// process may replace and sweep meanwhile.
    fn load(&self, root: &Path) -> Result<Profiles> {
        match self.file {
            Some(_) => load(root),
            None => Ok(Profiles {
                version: FILE_VERSION,
                ..Profiles::default()
            }),
        }
    }
}

fn lock(root: &Path, create: bool) -> Result<Locked> {
    let process = STORE.lock().unwrap_or_else(|p| p.into_inner());
    // A read with no record file has nothing to be consistent with, so it
    // creates nothing (the file only ever appears by an atomic rename).
    let file =
        if create || profiles_path(root).exists() { Some(FileLock::acquire(root)?) } else { None };
    #[cfg(test)]
    if file.is_none() {
        files::after_absent_check(root);
    }
    Ok(Locked {
        file,
        _process: process,
    })
}

/// The process's store directory.
pub fn root() -> Result<PathBuf> {
    if let Some(root) = super::runtime::host_profiles_dir() {
        return Ok(root);
    }
    Ok(pocket_codex_core::paths::state_dir()
        .context("resolving the state directory")?
        .join("hosting"))
}

fn profiles_path(root: &Path) -> PathBuf {
    root.join("profiles.json")
}

fn load(root: &Path) -> Result<Profiles> {
    let raw = match std::fs::read(profiles_path(root)) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Profiles {
                version: FILE_VERSION,
                hosts: BTreeMap::new(),
                pins: BTreeMap::new(),
            })
        },
        Err(e) => return Err(e).context("reading the saved host configuration"),
    };
    // The content is never quoted in the error: it is the user's file.
    let mut profiles: Profiles = serde_json::from_slice(&raw)
        .map_err(|_| anyhow!("the saved host configuration is unreadable; it was left as is"))?;
    match profiles.version {
        FILE_VERSION => {},
        1 => {
            // Version 1 kept one unnamed key file per instance.
            for (name, record) in &mut profiles.hosts {
                let legacy = key_file_name(name, None);
                if key_path(root, &legacy).is_file() {
                    record.key = Some(legacy);
                }
            }
        },
        _ => bail!("the saved host configuration is from a newer version; it was left as is"),
    }
    Ok(profiles)
}

/// Write the records. Once renamed into place they are committed, durable
/// or not; only a [`Committed::Durable`] write may be followed by cleanup.
fn store(root: &Path, profiles: &mut Profiles) -> Result<Committed> {
    profiles.version = FILE_VERSION;
    let bytes = serde_json::to_vec_pretty(&*profiles).context("encoding the host configuration")?;
    #[cfg(test)]
    if files::fail_next_commit() {
        bail!("injected commit failure");
    }
    write_private(&profiles_path(root), &bytes)
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// A record as it is, or why it cannot be used.
fn profile_of(root: &Path, name: &str, record: Option<&Record>) -> HostProfile {
    let Some(record) = record else {
        return HostProfile::inherited();
    };
    let parsed = parse(root, name, record);
    let problem = parsed.as_ref().err().map(|e| format!("{e:#}"));
    let parsed = parsed.ok();
    HostProfile {
        source: parsed.as_ref().map_or(Source::Inherited, |p| p.source),
        protocol: parsed.as_ref().and_then(|p| p.protocol),
        base_url: record.base_url.clone(),
        model: record.model.clone(),
        key_set: record.key.is_some(),
        claude_env: environment::read(root, name, record.claude_env.as_deref())
            .map(|v| environment::visible(&v))
            .unwrap_or_default(),
        runtime: record.runtime.clone(),
        revision: record.revision,
        problem,
    }
}

struct Parsed {
    source: Source,
    protocol: Option<ApiProtocol>,
    base_url: Option<String>,
}

/// Validate a stored record as strictly as an input.
fn parse(root: &Path, name: &str, record: &Record) -> Result<Parsed> {
    let invalid = |what: &str| anyhow!("the saved {what} is invalid; edit and save the host again");
    if record.revision == 0 {
        return Err(invalid("revision"));
    }
    let source = Source::parse(&record.source).ok_or_else(|| invalid("model service"))?;
    let protocol = match record.protocol.as_deref() {
        Some(raw) => Some(ApiProtocol::parse(raw).ok_or_else(|| invalid("provider API"))?),
        None => None,
    };
    let base_url = match record.base_url.as_deref() {
        Some(url) => Some(validate_base_url(url).map_err(|_| invalid("base URL"))?),
        None => None,
    };
    if record
        .model
        .as_deref()
        .is_some_and(|m| m.trim().is_empty() || m.chars().any(char::is_control))
    {
        return Err(invalid("default model"));
    }
    if source == Source::Custom && (protocol.is_none() || base_url.is_none()) {
        return Err(invalid("provider"));
    }
    if let Some(key) = record.key.as_deref() {
        if !valid_key_ref(name, key) {
            return Err(invalid("key reference"));
        }
        if !key_path(root, key).is_file() {
            bail!("the stored key is missing; enter it again and save");
        }
    }
    environment::read(root, name, record.claude_env.as_deref())?;
    if let Some(runtime) = &record.runtime {
        runtime.normalized().map_err(|_| invalid("runtime"))?;
    }
    Ok(Parsed {
        source,
        protocol,
        base_url,
    })
}

/// The saved configuration of `name` (inherited, revision 0, when none).
pub fn get(name: &str) -> Result<HostProfile> {
    get_in(&root()?, name)
}

/// [`get`] in the store at `root`.
pub fn get_in(root: &Path, name: &str) -> Result<HostProfile> {
    let name = host_names::canonical(name)?;
    let lock = lock(root, false)?;
    let profiles = lock.load(root)?;
    Ok(profile_of(root, &name, profiles.hosts.get(&name)))
}

/// Save `change` for `name` in the store at `root`, validated for `engine`.
/// With `expected` set, refuses unless the saved revision is still that
/// one. Never touches a running host.
pub fn save_in(
    root: &Path,
    name: &str,
    change: &Change,
    expected: Option<u64>,
    engine: Engine<'_>,
) -> Result<HostProfile> {
    let (profile, staged) = stage_in(root, name, change, expected, engine)?;
    staged.finish();
    Ok(profile)
}

/// A committed save whose previous record (and key file) can still be put
/// back: see [`stage_in`].
///
/// The stage pins its previous and new key files in the record file until
/// it finishes or rolls back, so another save's cleanup never removes a key
/// this stage may still restore or launch with.
#[must_use = "finish or roll back a staged save"]
pub struct Staged {
    root: PathBuf,
    name: String,
    previous: Option<Record>,
    revision: u64,
    pin: String,
    /// What the committed record resolves to, read under the same lock as
    /// the commit (an error is safe to show).
    resolved: std::result::Result<Resolved, String>,
}

impl Staged {
    /// The configuration this save committed, as a launch uses it: never a
    /// later save made meanwhile.
    pub fn resolved(&self) -> Result<&Resolved> {
        self.resolved.as_ref().map_err(|e| anyhow!("{e}"))
    }

    /// Keep the save: release its pin and remove key files nothing names
    /// any more. Best effort: on any failure (or a record write that may not
    /// survive a crash) every key file is kept.
    pub fn finish(self) {
        if let Err(error) = self.settle(false) {
            tracing::warn!(error = %format!("{error:#}"), "host key cleanup skipped");
        }
    }

    /// Put the previous record back (under a newer revision), unless
    /// another save came in meanwhile.
    pub fn rollback(self) -> Result<()> {
        self.settle(true)
    }

    fn settle(&self, restore: bool) -> Result<()> {
        // Cleanup only under the lock and after an authoritative read.
        let lock = lock(&self.root, true)?;
        let mut profiles = lock.load(&self.root)?;
        let current = profiles.hosts.get(&self.name).map(|r| r.revision);
        if restore && current == Some(self.revision) {
            let next = self
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow!("the saved revision cannot move further"))?;
            match self.previous.clone() {
                Some(mut previous) => {
                    previous.revision = next;
                    profiles.hosts.insert(self.name.clone(), previous);
                },
                None => {
                    profiles.hosts.remove(&self.name);
                },
            }
        }
        profiles.pins.remove(&self.pin);
        match store(&self.root, &mut profiles)? {
            Committed::Durable => sweep(&self.root, &self.name, &profiles),
            // A crash could bring back an earlier record file naming keys
            // this one does not: keep them all until a durable write.
            Committed::Uncertain => {
                tracing::warn!("host key cleanup deferred: the record write may not be durable")
            },
        }
        Ok(())
    }
}

/// Commit `change` but keep the previous key file until [`Staged::finish`],
/// so a failed start can [`Staged::rollback`] to exactly what was saved.
pub fn stage_in(
    root: &Path,
    name: &str,
    change: &Change,
    expected: Option<u64>,
    engine: Engine<'_>,
) -> Result<(HostProfile, Staged)> {
    stage_checked_in(root, name, change, expected, engine, &|| Ok(()))
}

/// Stage a remote save only if its admission is still valid under the file
/// lock.
pub(super) fn stage_checked_in(
    root: &Path,
    name: &str,
    change: &Change,
    expected: Option<u64>,
    engine: Engine<'_>,
    check: &dyn Fn() -> Result<()>,
) -> Result<(HostProfile, Staged)> {
    let name = host_names::canonical(name)?;
    let input = &change.input;
    engine.allows(input.source, input.protocol)?;
    let custom = input.source == Source::Custom;
    let base_url = match trimmed(input.base_url.as_deref()) {
        Some(url) if custom => Some(validate_base_url(&url).map_err(|e| anyhow!("{e}"))?),
        None if custom => bail!("the provider base URL is required"),
        _ => None,
    };
    let model = trimmed(input.model.as_deref()).filter(|_| custom);
    if model
        .as_deref()
        .is_some_and(|m| m.chars().any(char::is_control))
    {
        bail!("the default model name contains a control character");
    }
    let protocol = input.protocol.filter(|_| custom);
    if custom && matches!(engine, Engine::OpenCode) {
        opencode::validate_fields(base_url.as_deref(), model.as_deref())?;
    }
    let key = match &change.key {
        KeyChange::Replace(secret) if secret.expose().trim().is_empty() => {
            bail!("the new key is empty; leave it blank to keep the stored one")
        },
        KeyChange::Replace(_) if !custom => bail!("only a custom provider has a key"),
        KeyChange::Replace(secret) => {
            secrets::check_key(secret.expose().trim())?;
            change.key.clone()
        },
        // Leaving the custom provider always forgets its key.
        _ if !custom => KeyChange::Remove,
        key => key.clone(),
    };
    let runtime_set = match &change.runtime {
        RuntimeChange::Set(runtime) => {
            engine.admits(runtime)?;
            Some(runtime.normalized()?)
        },
        _ => None,
    };

    let lock = lock(root, true)?;
    check()?;
    let mut profiles = lock.load(root)?;
    let previous = profiles.hosts.get(&name).cloned();
    let current = previous.as_ref().map_or(0, |r| r.revision);
    if expected.is_some_and(|expected| expected != current) {
        return Err(RevisionConflict {
            current,
        }
        .into());
    }
    if custom && matches!(engine, Engine::OpenCode) {
        let present = match &key {
            KeyChange::Replace(secret) => {
                opencode::validate_key(secret.expose())?;
                true
            },
            KeyChange::Keep => previous.as_ref().is_some_and(|r| r.key.is_some()),
            KeyChange::Remove => false,
        };
        if !present {
            bail!("OpenCode needs the custom provider's API key");
        }
    }
    let revision = current
        .checked_add(1)
        .ok_or_else(|| anyhow!("the saved revision cannot move further"))?;
    let old_env = previous.as_ref().and_then(|r| r.claude_env.clone());
    let (old_values, readable) = match environment::read(root, &name, old_env.as_deref()) {
        Ok(values) => (values, true),
        Err(_)
            if input
                .claude_env
                .as_ref()
                .is_some_and(|v| v.values().all(Option::is_some)) =>
        {
            (Default::default(), false)
        },
        Err(error) => return Err(error),
    };
    let values = match &input.claude_env {
        Some(input) => environment::normalized(input, &old_values, engine)?,
        None => old_values.clone(),
    };
    // Refuse overrides when changing the host to another runtime.
    environment::normalized(
        &values
            .iter()
            .map(|(n, v)| (n.clone(), Some(v.expose().to_string())))
            .collect(),
        &Default::default(),
        engine,
    )?;
    let new_env = if readable && values == old_values {
        old_env.clone()
    } else {
        environment::write(root, &name, &values)?
    };
    let old_key = previous.as_ref().and_then(|r| r.key.clone());
    let new_key_result = (|| -> Result<Option<String>> {
        Ok(match &key {
            KeyChange::Keep => old_key.clone(),
            KeyChange::Remove => None,
            KeyChange::Replace(secret) => {
                let file = key_file_name(&name, Some(&uuid::Uuid::new_v4().to_string()));
                let path = key_path(root, &file);
                // A record must never durably name a key file a crash could
                // lose: refuse the save before any record names it.
                if write_private(&path, secret.expose().trim().as_bytes())? == Committed::Uncertain
                {
                    let _ = std::fs::remove_file(&path);
                    bail!("the key could not be stored durably; nothing was saved, try again");
                }
                Some(file)
            },
        })
    })();
    let new_key = match new_key_result {
        Ok(key) => key,
        Err(error) => {
            if new_env != old_env {
                if let Some(file) = &new_env {
                    let _ = std::fs::remove_file(key_path(root, file));
                }
            }
            return Err(error);
        },
    };
    let source = input.source.as_str().to_string();
    let same_service = previous.as_ref().is_some_and(|p| {
        p.source == source
            && p.protocol.as_deref() == protocol.map(ApiProtocol::as_str)
            && p.base_url == base_url
            && p.model == model
            && p.key == new_key
            && p.claude_env == new_env
    });
    let authority = match input.source {
        Source::Inherited if values.is_empty() => None,
        _ if same_service => previous
            .as_ref()
            .and_then(|p| p.authority.clone())
            .or_else(|| Some(uuid::Uuid::new_v4().to_string())),
        _ => Some(uuid::Uuid::new_v4().to_string()),
    };
    let runtime = match &change.runtime {
        RuntimeChange::Keep => previous.as_ref().and_then(|r| r.runtime.clone()),
        RuntimeChange::Set(_) => runtime_set,
        #[cfg(test)]
        RuntimeChange::Clear => None,
    };
    let record = Record {
        claude_env: new_env.clone(),
        source,
        protocol: protocol.map(|p| p.as_str().to_string()),
        base_url,
        model,
        key: new_key.clone(),
        authority,
        runtime,
        revision,
    };
    let resolved = resolve_record(root, &name, &record).map_err(|e| format!("{e:#}"));
    profiles.hosts.insert(name.clone(), record);
    // Other stages' pins stay however old they are: only their own stage
    // releases them (see the module docs).
    let now = now_secs();
    let pin = uuid::Uuid::new_v4().to_string();
    let mut pinned: Vec<String> = old_key
        .iter()
        .chain(new_key.iter())
        .chain(old_env.iter())
        .chain(new_env.iter())
        .cloned()
        .collect();
    pinned.dedup();
    if !pinned.is_empty() {
        profiles.pins.insert(pin.clone(), Pin {
            name: name.clone(),
            keys: pinned,
            at: now,
        });
    }
    if let Err(error) = store(root, &mut profiles) {
        // Not renamed into place (`store` errs only then): the new key file
        // is no record's, so remove it.
        if new_key != old_key {
            if let Some(file) = &new_key {
                let _ = std::fs::remove_file(key_path(root, file));
            }
        }
        if new_env != old_env {
            if let Some(file) = &new_env {
                let _ = std::fs::remove_file(key_path(root, file));
            }
        }
        return Err(error);
    }
    let profile = profile_of(root, &name, profiles.hosts.get(&name));
    Ok((profile, Staged {
        root: root.to_path_buf(),
        name,
        previous,
        revision,
        pin,
        resolved,
    }))
}

/// Remove `name`'s key files that neither its record in `profiles` nor a
/// pin names. `profiles` must have been read under the held store lock.
fn sweep(root: &Path, name: &str, profiles: &Profiles) {
    let keep: Vec<&str> = profiles
        .hosts
        .get(name)
        .into_iter()
        .flat_map(|r| r.key.as_deref().into_iter().chain(r.claude_env.as_deref()))
        .chain(
            profiles
                .pins
                .values()
                .filter(|pin| pin.name == name)
                .flat_map(|pin| pin.keys.iter().map(String::as_str)),
        )
        .collect();
    files::sweep_keys(root, name, &keep);
}

/// The saved configuration of `name` in the store at `root` for a launch,
/// with its key.
pub fn resolve_in(root: &Path, name: &str) -> Result<Resolved> {
    let name = host_names::canonical(name)?;
    let lock = lock(root, false)?;
    let profiles = lock.load(root)?;
    match profiles.hosts.get(&name) {
        Some(record) => resolve_record(root, &name, record),
        None => Ok(Resolved::inherited()),
    }
}

/// What `record` of `name` resolves to; the caller holds the store lock.
fn resolve_record(root: &Path, name: &str, record: &Record) -> Result<Resolved> {
    let parsed = parse(root, name, record)?;
    let endpoint = if parsed.source == Source::Custom {
        let key = match record.key.as_deref() {
            Some(file) => {
                let key = std::fs::read_to_string(key_path(root, file))
                    .context("reading the stored key")?;
                let key = key.trim();
                if key.is_empty() {
                    bail!("the stored key is empty; enter it again and save");
                }
                if secrets::check_key(key).is_err() {
                    bail!("the stored key is not usable; enter it again and save");
                }
                Some(Secret::new(key))
            },
            None => None,
        };
        Some(ProviderEndpoint {
            protocol: parsed
                .protocol
                .ok_or_else(|| anyhow!("the saved provider has no API; edit the host"))?,
            base_url: parsed
                .base_url
                .ok_or_else(|| anyhow!("the saved provider has no base URL; edit the host"))?,
            key,
            model: record.model.clone(),
        })
    } else {
        None
    };
    let claude_env = environment::read(root, name, record.claude_env.as_deref())?;
    let authority = match parsed.source {
        Source::Inherited if claude_env.is_empty() => None,
        // Version 1 had no authority: one per saved revision is safe.
        _ => Some(
            record
                .authority
                .clone()
                .unwrap_or_else(|| format!("legacy-{}", record.revision)),
        ),
    };
    Ok(Resolved {
        claude_env,
        source: parsed.source,
        endpoint,
        authority,
        runtime: record.runtime.clone(),
        revision: record.revision,
    })
}

/// The Codex launch of `resolved` for the instance `name`.
pub fn codex(resolved: &Resolved, name: &str) -> Result<CodexLaunch> {
    let mode = match (resolved.source, &resolved.endpoint) {
        (Source::Inherited, _) => CodexProviderMode::Inherited,
        (Source::Official, _) => CodexProviderMode::Official,
        (Source::Custom, Some(endpoint)) => CodexProviderMode::Custom(endpoint.clone()),
        (Source::Custom, None) => bail!("the saved provider is incomplete; edit the host"),
    };
    let authority = resolved.authority.as_deref().unwrap_or_default();
    codex_launch(&mode, &host_names::canonical(name)?, authority).map_err(|e| anyhow!("{e}"))
}

/// The ACP provider launch of `resolved` for `spec` under the instance
/// `name`, and the authority its session source is keyed by; `None` when it
/// inherits.
pub fn acp(
    resolved: &Resolved,
    spec: &AgentSpec,
    name: &str,
) -> Result<Option<(acp_provider::AcpProviderLaunch, String)>> {
    match (resolved.source, &resolved.endpoint) {
        (Source::Inherited, _) => Ok(None),
        (Source::Official, _) => bail!("only native Codex offers an official-account sign-in here"),
        (Source::Custom, None) => bail!("the saved provider is incomplete; edit the host"),
        (Source::Custom, Some(endpoint)) => {
            host_names::canonical(name)?;
            let authority = resolved
                .authority
                .clone()
                .ok_or_else(|| anyhow!("the saved provider has no authority; save it again"))?;
            let launch = acp_provider::launch(spec, endpoint).map_err(|e| anyhow!("{e}"))?;
            Ok(Some((launch, authority)))
        },
    }
}

#[cfg(test)]
#[path = "host_profiles_tests.rs"]
mod tests;

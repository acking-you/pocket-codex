//! Versioned OpenCode preferences, separate from legacy Codex configuration.

use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{
    service::{sanitize_component, ServiceId, ServiceKind},
    Error, Result,
};

/// Non-secret details needed to reconnect to an existing OpenCode server.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeProfile {
    /// Stable controller-local profile identifier.
    pub id: String,
    /// User-visible connection name.
    pub label: String,
    /// HTTP(S) server origin without credentials, query, or fragment.
    pub base_url: String,
    /// Explicit project directory on the OpenCode host.
    pub directory: String,
}

impl std::fmt::Debug for OpenCodeProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenCodeProfile")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("base_url", &"[redacted]")
            .field("directory", &self.directory)
            .finish()
    }
}

/// Versioned OpenCode profiles and defaults; never contains authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeProfiles {
    /// File format version, currently 1.
    pub version: u32,
    /// Saved direct connections.
    pub profiles: Vec<OpenCodeProfile>,
    /// Default direct connection, if selected.
    pub default_profile_id: Option<String>,
    /// Default relay target, independent of the Codex defaults.
    pub default_service: Option<ServiceId>,
}

impl Default for OpenCodeProfiles {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: Vec::new(),
            default_profile_id: None,
            default_service: None,
        }
    }
}

impl OpenCodeProfiles {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Config("unsupported OpenCode profile version".into()));
        }
        let mut ids = HashSet::new();
        for profile in &self.profiles {
            if profile.id.is_empty()
                || sanitize_component(&profile.id) != profile.id
                || !ids.insert(profile.id.as_str())
                || profile.directory.trim().is_empty()
                || profile.directory.chars().any(char::is_control)
            {
                return Err(Error::Config(
                    "OpenCode profiles require unique canonical IDs and explicit directories"
                        .into(),
                ));
            }
            let invalid_url = || {
                Error::Config(
                    "OpenCode URL must be an HTTP(S) origin without credentials, query, or \
                     fragment"
                        .into(),
                )
            };
            let url = url::Url::parse(&profile.base_url).map_err(|_| invalid_url())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(invalid_url());
            }
        }
        if self
            .default_profile_id
            .as_ref()
            .is_some_and(|id| !ids.contains(id.as_str()))
        {
            return Err(Error::Config("OpenCode default profile does not exist".into()));
        }
        if let Some(service) = &self.default_service {
            if service.kind != ServiceKind::OpenCode
                || service.device.is_empty()
                || service.name.is_empty()
                || sanitize_component(&service.device) != service.device
                || sanitize_component(&service.name) != service.name
            {
                return Err(Error::Config(
                    "OpenCode default service must be a canonical OpenCode target".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Synchronous profile storage at a caller-selected config/support path.
pub struct OpenCodeProfileStore {
    path: PathBuf,
}

impl OpenCodeProfileStore {
    /// Select the independent `opencode-v1.json` path without doing I/O.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
        }
    }

    /// Read profiles, treating a missing file as an empty version 1 store.
    pub fn read(&self) -> Result<OpenCodeProfiles> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let profiles: OpenCodeProfiles = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Config("invalid OpenCode profile file".into()))?;
                profiles.validate()?;
                Ok(profiles)
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(OpenCodeProfiles::default())
            },
            Err(error) => Err(error.into()),
        }
    }

    /// Update the newest stored preferences and persist them atomically.
    ///
    /// Holds an OS file lock across read, change, and write. The callback must
    /// not recursively update this store. Async callers should use a blocking
    /// worker because acquiring the lock can wait for another process.
    pub fn update(
        &self,
        change: impl FnOnce(&mut OpenCodeProfiles) -> Result<()>,
    ) -> Result<OpenCodeProfiles> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.path.with_extension("json.lock"))?;
        lock.lock()?;
        let mut profiles = self.read()?;
        change(&mut profiles)?;
        profiles.validate()?;
        self.write(&profiles)?;
        drop(lock);
        Ok(profiles)
    }

    fn write(&self, profiles: &OpenCodeProfiles) -> Result<()> {
        static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
        let temporary = self.path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let result = (|| -> Result<()> {
            file.write_all(&serde_json::to_vec_pretty(profiles)?)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{config::Config, service::ServiceKind, state::RuntimeState};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pocket-opencode-core-{}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().expect("timestamp"),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("create isolated test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(id: &str) -> OpenCodeProfile {
        OpenCodeProfile {
            id: id.into(),
            label: "Work".into(),
            base_url: "http://127.0.0.1:4096".into(),
            directory: "/project".into(),
        }
    }

    #[test]
    fn saving_profiles_preserves_legacy_codex_configuration_and_defaults() -> Result<()> {
        let dir = TestDir::new();
        let config_path = dir.path().join("config.toml");
        let state_path = dir.path().join("state.toml");
        let legacy_config = "[services.app.default]\ndevice = 'studio'\nname = 'codex'\n";
        let legacy_state = "[[selected_services]]\nkind = 'app'\ndevice = 'studio'\nname = \
                            'codex'\nselected_at = '2026-09-27T00:00:00Z'\n";
        std::fs::write(&config_path, legacy_config)?;
        std::fs::write(&state_path, legacy_state)?;
        let path = dir.path().join("opencode-v1.json");
        let store = OpenCodeProfileStore::new(&path);
        assert_eq!(store.read()?, OpenCodeProfiles::default());
        let saved = store.update(|settings| {
            settings.profiles.push(profile("work"));
            settings.default_profile_id = Some("work".into());
            settings.default_service =
                Some(ServiceId::new("studio", ServiceKind::OpenCode, "work"));
            Ok(())
        })?;
        assert_eq!(OpenCodeProfileStore::new(path).read()?, saved);
        assert_eq!(std::fs::read_to_string(&config_path)?, legacy_config);
        assert_eq!(std::fs::read_to_string(&state_path)?, legacy_state);
        let config: Config = toml::from_str(legacy_config)?;
        assert_eq!(
            config
                .default_service(ServiceKind::App)
                .expect("Codex default")
                .name,
            "codex"
        );
        assert_eq!(
            RuntimeState::load_from(&state_path)?
                .selected_service(ServiceKind::App)
                .expect("Codex selection")
                .name,
            "codex"
        );
        Ok(())
    }

    #[test]
    fn credential_bearing_urls_are_rejected_without_exposing_them() -> Result<()> {
        let dir = TestDir::new();
        let store = OpenCodeProfileStore::new(dir.path().join("opencode-v1.json"));
        for url in [
            "https://opencode:CANARY_SECRET@example.com",
            "https://example.com?auth_token=CANARY_SECRET",
            "https://example.com#CANARY_SECRET",
            "ftp://example.com",
        ] {
            let error = store
                .update(|settings| {
                    let mut entry = profile("work");
                    entry.base_url = url.into();
                    settings.profiles.push(entry);
                    Ok(())
                })
                .expect_err("credential channel or unsupported scheme must be rejected");
            assert!(!format!("{error:?} {error}").contains("CANARY_SECRET"));
            assert_eq!(store.read()?, OpenCodeProfiles::default());
        }
        Ok(())
    }

    #[test]
    fn untrusted_profile_files_cannot_return_or_echo_authentication() -> Result<()> {
        let dir = TestDir::new();
        let path = dir.path().join("opencode-v1.json");
        let store = OpenCodeProfileStore::new(&path);
        for raw in [
            r#"{"version":1,"profiles":[{"id":"work","label":"Work","base_url":"https://user:CANARY_SECRET@example.com","directory":"/project"}],"default_profile_id":null,"default_service":null}"#,
            r#"{"version":1,"profiles":[],"CANARY_SECRET":"unknown field","default_profile_id":null,"default_service":null}"#,
        ] {
            std::fs::write(&path, raw)?;
            let error = store.read().expect_err("unsafe profile file");
            assert!(!format!("{error:?} {error}").contains("CANARY_SECRET"));
        }
        Ok(())
    }

    #[test]
    fn concurrent_store_updates_preserve_both_writers() -> Result<()> {
        use std::{sync::mpsc, thread, time::Duration};

        let dir = TestDir::new();
        let path = dir.path().join("opencode-v1.json");
        let first = OpenCodeProfileStore::new(&path);
        let second = OpenCodeProfileStore::new(&path);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_writer = thread::spawn(move || {
            first.update(|settings| {
                entered_tx.send(()).expect("announce first writer");
                release_rx.recv().expect("release first writer");
                settings.profiles.push(profile("first"));
                Ok(())
            })
        });
        entered_rx.recv().expect("first writer entered");
        let (second_tx, second_rx) = mpsc::channel();
        let second_writer = thread::spawn(move || {
            second.update(|settings| {
                second_tx.send(()).expect("announce second writer");
                settings.profiles.push(profile("second"));
                Ok(())
            })
        });
        let entered_early = second_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        release_tx.send(()).expect("release first writer");
        first_writer.join().expect("first writer panicked")?;
        second_writer.join().expect("second writer panicked")?;
        assert!(!entered_early, "updates must serialize before reading the current file");
        let saved = OpenCodeProfileStore::new(path).read()?;
        assert_eq!(
            saved
                .profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        Ok(())
    }

    #[test]
    fn future_profile_versions_are_not_read_or_overwritten() -> Result<()> {
        let dir = TestDir::new();
        let path = dir.path().join("opencode-v1.json");
        let original =
            r#"{"version":2,"profiles":[],"default_profile_id":null,"default_service":null}"#;
        std::fs::write(&path, original)?;
        let store = OpenCodeProfileStore::new(&path);
        assert!(store.read().is_err(), "future format must not be interpreted as version 1");
        assert!(store
            .update(|settings| {
                settings.profiles.push(profile("work"));
                Ok(())
            })
            .is_err());
        assert_eq!(std::fs::read_to_string(path)?, original);
        Ok(())
    }

    #[test]
    fn invalid_profile_identity_or_default_cannot_replace_saved_preferences() -> Result<()> {
        let dir = TestDir::new();
        let store = OpenCodeProfileStore::new(dir.path().join("opencode-v1.json"));
        let saved = store.update(|settings| {
            settings.profiles.push(profile("work"));
            settings.default_profile_id = Some("work".into());
            Ok(())
        })?;
        for case in 0..5 {
            assert!(
                store
                    .update(|settings| {
                        match case {
                            0 => settings.profiles.push(profile("work")),
                            1 => settings.default_profile_id = Some("missing".into()),
                            2 => {
                                settings.default_service =
                                    Some(ServiceId::new("studio", ServiceKind::App, "work"))
                            },
                            3 => settings.profiles[0].directory.clear(),
                            _ => settings.profiles[0].id.clear(),
                        }
                        Ok(())
                    })
                    .is_err(),
                "invalid case {case} must fail"
            );
            assert_eq!(store.read()?, saved);
        }
        Ok(())
    }

    #[test]
    fn profile_debug_does_not_expose_credentials_before_validation() {
        let mut entry = profile("work");
        entry.base_url = "https://user:CANARY_SECRET@example.com".into();
        assert!(!format!("{entry:?}").contains("CANARY_SECRET"));
    }
}

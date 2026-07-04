//! Bootstrap a usable `CODEX_HOME` from the Pocket-Codex onboarding UI.
//!
//! codex reads its provider + model credentials from `$CODEX_HOME/config.toml`
//! and `$CODEX_HOME/auth.json`. A fresh machine has neither, so Pocket-Codex's
//! own (自带) app-server can't make a single model call and hosting fails at
//! the auth gate. This module lets the setup wizard bootstrap that config
//! without the user hand-editing TOML:
//!
//! * [`write_provider_config`] writes a minimal custom OpenAI-compatible
//!   provider (base URL + API key) straight into `config.toml`. The key rides
//!   as the provider's `experimental_bearer_token`, and `requires_openai_auth`
//!   stays `false`, so codex needs **no** `auth.json` and shows **no** login
//!   screen — a turn authorizes with `Authorization: Bearer <key>`.
//! * [`set_prompt_variant`] swaps codex's built-in gpt-5.5 system prompt for a
//!   bundled "non-degraded" variant (the default minus the commentary /
//!   Intermediary-updates mandates that can starve reasoning, see
//!   openai/codex#30364) by pointing `model_instructions_file` at a file this
//!   module drops into `CODEX_HOME`.
//!
//! The official ChatGPT-login path is driven separately over the app-server's
//! `account/login/*` RPC (codex writes `auth.json` itself); this module only
//! owns what Pocket-Codex writes to disk. All edits are format-preserving
//! (`toml_edit`) so a user's hand-written `config.toml` keeps its comments and
//! ordering, and idempotent so re-running the wizard is safe.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::rollout::codex_home;

/// The `model_providers` id Pocket-Codex writes for the custom provider. Chosen
/// to avoid codex's reserved ids (`openai` / `ollama` / `lmstudio` /
/// `amazon-bedrock`), which `config.toml` refuses to override.
pub const POCKET_PROVIDER_ID: &str = "pocket";

/// Display name for the generated provider entry.
const POCKET_PROVIDER_NAME: &str = "Pocket-Codex Provider";

/// Default model id the custom-provider wizard selects when the user leaves it
/// blank. Providers expose their own model ids; this is a sensible default that
/// the UI lets the user override.
pub const DEFAULT_MODEL: &str = "gpt-5.5";

/// Filename of the non-degraded system prompt Pocket-Codex drops into
/// `CODEX_HOME` and points `model_instructions_file` at.
const NONDEGRADED_PROMPT_FILE: &str = "pocket-codex-nondegraded-prompt.md";

/// The bundled non-degraded gpt-5.5 prompt: the model's default instructions
/// with the `commentary`-channel mandate neutered and the whole `Intermediary
/// updates` section removed, so the model spends its budget reasoning instead
/// of narrating. Written to `CODEX_HOME` on demand by [`set_prompt_variant`].
pub const NONDEGRADED_PROMPT: &str = include_str!("../assets/prompts/gpt-5.5-non-degraded.md");

/// Which system-prompt variant is active for the 自带 codex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptVariant {
    /// codex's built-in model prompt (no `model_instructions_file`).
    Default,
    /// The bundled non-degraded prompt (`model_instructions_file` points at our
    /// dropped file).
    NonDegraded,
    /// `model_instructions_file` is set but to some other file — a prompt the
    /// user configured themselves. Left untouched by the toggle.
    Custom,
}

impl PromptVariant {
    /// The wire string the bridge/UI exchanges (`default` / `non_degraded` /
    /// `custom`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::NonDegraded => "non_degraded",
            Self::Custom => "custom",
        }
    }
}

/// A snapshot of what codex has on disk in `CODEX_HOME`, for the onboarding UI
/// to decide whether to run the setup wizard.
#[derive(Debug, Clone)]
pub struct CodexSetupStatus {
    /// Resolved `CODEX_HOME` (display path).
    pub codex_home: String,
    /// `config.toml` exists.
    pub has_config: bool,
    /// A credential exists: `auth.json` on disk OR `CODEX_ACCESS_TOKEN` in the
    /// environment.
    pub has_auth: bool,
    /// `config.toml` selects a non-OpenAI custom provider (so codex authorizes
    /// turns without `auth.json`).
    pub has_custom_provider: bool,
    /// `auth.json`'s `auth_mode` (e.g. `apikey` / `chatgpt`), when present.
    pub auth_mode: Option<String>,
    /// Nothing lets codex authenticate a model call yet: no credential AND no
    /// custom provider. This is the signal to show the setup wizard.
    pub needs_setup: bool,
    /// Which system-prompt variant is active.
    pub prompt_variant: String,
}

/// Path to `$CODEX_HOME/config.toml`.
fn config_path(home: &Path) -> PathBuf {
    home.join("config.toml")
}

/// Path to `$CODEX_HOME/auth.json`.
fn auth_path(home: &Path) -> PathBuf {
    home.join("auth.json")
}

/// Read `config.toml` into a `toml_edit` document, or an empty document when
/// the file is absent. Surfaces a parse error (a corrupt hand-edited file)
/// rather than silently discarding the user's content.
fn load_document(home: &Path) -> Result<DocumentMut> {
    match std::fs::read_to_string(config_path(home)) {
        Ok(raw) => raw
            .parse::<DocumentMut>()
            .context("parsing existing codex config.toml"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e).context("reading codex config.toml"),
    }
}

/// Write `doc` to `$CODEX_HOME/config.toml`, creating `CODEX_HOME` first. On
/// unix the file is tightened to `0o600` before the bytes are written, because
/// it may carry the provider's inline bearer token.
fn write_document(home: &Path, doc: &DocumentMut) -> Result<()> {
    std::fs::create_dir_all(home).context("creating CODEX_HOME")?;
    let path = config_path(home);
    let raw = doc.to_string();
    write_secret_file(&path, raw.as_bytes()).context("writing codex config.toml")
}

/// Write `bytes` to `path`, `0o600` on unix (set before writing so the secret
/// is never briefly world-readable), a plain overwrite elsewhere.
fn write_secret_file(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::{
            io::Write as _,
            os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
        };
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

/// Inspect `CODEX_HOME` for the onboarding UI. Never fails on missing files —
/// absence is the normal first-run state — only on an unresolvable home.
pub fn setup_status() -> Result<CodexSetupStatus> {
    let home = codex_home()?;
    let has_config = config_path(&home).exists();
    let auth_file = auth_path(&home);
    let auth_on_disk = auth_file.exists();
    let has_env_token = std::env::var_os("CODEX_ACCESS_TOKEN")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let auth_mode = if auth_on_disk { read_auth_mode(&auth_file) } else { None };
    let has_custom_provider = document_selects_custom_provider(&home);
    let has_auth = auth_on_disk || has_env_token;
    Ok(CodexSetupStatus {
        codex_home: home.display().to_string(),
        has_config,
        has_auth,
        has_custom_provider,
        auth_mode,
        needs_setup: !has_auth && !has_custom_provider,
        prompt_variant: prompt_variant(&home).as_str().to_string(),
    })
}

/// Best-effort read of `auth.json`'s `auth_mode` field (`apikey` / `chatgpt` /
/// …). Returns `None` on any read/parse problem — the caller only uses it for
/// display.
fn read_auth_mode(auth_file: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(auth_file).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    json.get("auth_mode")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Whether `config.toml` currently selects a custom (non-OpenAI) provider. A
/// custom provider carries its own auth (env key / bearer token), so codex can
/// run turns with no `auth.json` — hosting must not gate on a codex login in
/// that case.
pub fn has_custom_provider() -> bool {
    codex_home()
        .map(|home| document_selects_custom_provider(&home))
        .unwrap_or(false)
}

/// True when `config.toml`'s top-level `model_provider` names a provider that
/// isn't the built-in OpenAI/ChatGPT one — i.e. a provider defined in
/// `[model_providers.*]`, which brings its own credentials.
fn document_selects_custom_provider(home: &Path) -> bool {
    let Ok(doc) = load_document(home) else {
        return false;
    };
    let Some(selected) = doc
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    // The built-in openai provider still needs auth.json / a token; only a
    // provider defined under [model_providers] carries its own credentials.
    selected != "openai"
        && doc
            .get("model_providers")
            .and_then(Item::as_table_like)
            .map(|t| t.contains_key(selected))
            .unwrap_or(false)
}

/// Write a minimal custom OpenAI-compatible provider into `config.toml` and
/// select it. `base_url` is the provider's API base (e.g.
/// `https://example.com/v1`); `api_key` is stored as the provider's inline
/// `experimental_bearer_token`. `model` selects the default model (falls back
/// to [`DEFAULT_MODEL`] when blank).
///
/// Format-preserving and idempotent: existing keys, other providers, comments
/// and ordering are kept; only `model`, `model_provider`, and the `pocket`
/// provider entry are (re)written.
pub fn write_provider_config(base_url: &str, api_key: &str, model: Option<&str>) -> Result<()> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        bail!("provider base URL is required");
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        bail!("provider base URL must start with http:// or https:// (got `{base_url}`)");
    }
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("provider API key is required");
    }
    let model = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL);

    let home = codex_home()?;
    let mut doc = load_document(&home)?;

    doc["model"] = value(model);
    doc["model_provider"] = value(POCKET_PROVIDER_ID);

    // Ensure a `[model_providers]` table exists, preserving any sibling
    // providers the user already defined.
    let providers = doc
        .entry("model_providers")
        .or_insert(Item::Table(Table::new()));
    let providers = providers
        .as_table_mut()
        .context("`model_providers` in config.toml is not a table")?;
    // `model_providers` is only meaningful via its child tables; don't emit a
    // bare `[model_providers]` header.
    providers.set_implicit(true);

    // Replace only our own `pocket` entry (keep other providers intact).
    let mut entry = Table::new();
    entry["name"] = value(POCKET_PROVIDER_NAME);
    entry["base_url"] = value(base_url);
    // The key rides inline as the bearer token. `requires_openai_auth` stays at
    // its default (false), so codex needs no auth.json and shows no login
    // screen; a turn authorizes with `Authorization: Bearer <key>`. `wire_api`
    // is left unset (defaults to `responses`, the only supported transport).
    entry["experimental_bearer_token"] = value(api_key);
    providers.insert(POCKET_PROVIDER_ID, Item::Table(entry));

    write_document(&home, &doc)
}

/// Which system-prompt variant `config.toml` currently selects, from its
/// `model_instructions_file` key.
pub fn prompt_variant(home: &Path) -> PromptVariant {
    let Ok(doc) = load_document(home) else {
        return PromptVariant::Default;
    };
    match doc
        .get("model_instructions_file")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => PromptVariant::Default,
        Some(path) => {
            if same_file(Path::new(path), &home.join(NONDEGRADED_PROMPT_FILE)) {
                PromptVariant::NonDegraded
            } else {
                PromptVariant::Custom
            }
        },
    }
}

/// Whether two paths point at the same file: a direct comparison first, then a
/// best-effort canonicalized one so a stored absolute path still matches
/// `home.join(...)` across `\\?\` verbatim prefixes / symlinks. Canonicalize
/// only counts when *both* sides resolve (two `None`s must never compare
/// equal).
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(ca), Ok(cb)) if ca == cb
    )
}

/// The absolute on-disk path to point `model_instructions_file` at. Kept
/// prefix-clean when `home` is already absolute (the common case — `~/.codex`
/// or an absolute `$CODEX_HOME`), and canonicalized only to absolutize a
/// relative home, so the value written to `config.toml` reads naturally.
fn absolute_prompt_path(home: &Path, prompt_file: &Path) -> PathBuf {
    if home.is_absolute() {
        prompt_file.to_path_buf()
    } else {
        std::fs::canonicalize(prompt_file).unwrap_or_else(|_| prompt_file.to_path_buf())
    }
}

/// Set the active system-prompt variant. `non_degraded` drops the bundled
/// prompt into `CODEX_HOME` and points `model_instructions_file` at it;
/// `default` removes our `model_instructions_file` key (leaving the dropped
/// file, which is harmless). A user's own `model_instructions_file` (a `custom`
/// variant) is never overwritten by `non_degraded`→ err, and `default` only
/// clears the key when it points at our file.
///
/// Takes effect for threads started after the change (the app-server reads
/// `model_instructions_file` when it builds a session).
pub fn set_prompt_variant(variant: &str) -> Result<()> {
    let home = codex_home()?;
    match variant {
        "non_degraded" => {
            std::fs::create_dir_all(&home).context("creating CODEX_HOME")?;
            let prompt_file = home.join(NONDEGRADED_PROMPT_FILE);
            std::fs::write(&prompt_file, NONDEGRADED_PROMPT)
                .context("writing the non-degraded prompt file")?;
            let mut doc = load_document(&home)?;
            // model_instructions_file wants an absolute path; keep it prefix-clean
            // when CODEX_HOME is already absolute (the common case).
            let abs = absolute_prompt_path(&home, &prompt_file);
            doc["model_instructions_file"] = value(abs.display().to_string());
            write_document(&home, &doc)
        },
        "default" => {
            let mut doc = load_document(&home)?;
            // Only clear the key when it's ours — never disturb a prompt file the
            // user configured themselves.
            if prompt_variant(&home) == PromptVariant::NonDegraded {
                doc.remove("model_instructions_file");
                write_document(&home, &doc)?;
            }
            Ok(())
        },
        other => bail!("unknown prompt variant `{other}` (want `default` or `non_degraded`)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch `CODEX_HOME` under the OS temp dir, unique per test, cleaned
    /// on drop. Sets `$CODEX_HOME` so [`codex_home`] resolves here.
    struct TempHome {
        dir: PathBuf,
    }

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "pcx-setup-{tag}-{}-{:p}",
                std::process::id(),
                &tag as *const _
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("mk temp home");
            // `set_var` is a safe fn in edition 2021 (this crate forbids unsafe);
            // tests are serialized by `ENV_LOCK`, so the process-global var is
            // scoped to one test at a time and restored on drop.
            std::env::set_var("CODEX_HOME", &dir);
            Self {
                dir,
            }
        }

        fn config(&self) -> String {
            std::fs::read_to_string(self.dir.join("config.toml")).unwrap_or_default()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            std::env::remove_var("CODEX_HOME");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    // The CODEX_HOME env var is process-global, so these tests must not run
    // concurrently. A shared mutex serializes them (a poisoned lock is fine —
    // the guarded state is just the env var).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn provider_config_writes_minimal_working_toml() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = TempHome::new("prov");
        write_provider_config("https://api.example.com/v1/", "sk-secret", Some("gpt-5.5"))
            .expect("write provider");
        let raw = home.config();
        // Selected provider + model.
        assert!(raw.contains("model = \"gpt-5.5\""), "config:\n{raw}");
        assert!(raw.contains("model_provider = \"pocket\""), "config:\n{raw}");
        // Provider entry with the bearer token and a trailing-slash-trimmed URL.
        assert!(raw.contains("[model_providers.pocket]"), "config:\n{raw}");
        assert!(raw.contains("base_url = \"https://api.example.com/v1\""), "config:\n{raw}");
        assert!(raw.contains("experimental_bearer_token = \"sk-secret\""), "config:\n{raw}");
        // No login needed → the wizard reports the machine configured.
        let status = setup_status().expect("status");
        assert!(status.has_custom_provider);
        assert!(!status.needs_setup);
    }

    #[test]
    fn provider_config_preserves_existing_keys_and_providers() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = TempHome::new("merge");
        std::fs::write(
            home.dir.join("config.toml"),
            "# my config\napproval_policy = \"never\"\n\n[model_providers.other]\nname = \"Other\"\nbase_url = \"https://other/v1\"\n",
        )
        .expect("seed config");
        write_provider_config("https://api.example.com/v1", "sk-2", None).expect("write provider");
        let raw = home.config();
        // Untouched user content survives.
        assert!(raw.contains("# my config"), "config:\n{raw}");
        assert!(raw.contains("approval_policy = \"never\""), "config:\n{raw}");
        assert!(raw.contains("[model_providers.other]"), "config:\n{raw}");
        // Ours added alongside, default model applied.
        assert!(raw.contains("[model_providers.pocket]"), "config:\n{raw}");
        assert!(raw.contains(&format!("model = \"{DEFAULT_MODEL}\"")), "config:\n{raw}");
    }

    #[test]
    fn provider_config_rejects_bad_input() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _home = TempHome::new("bad");
        assert!(write_provider_config("", "k", None).is_err());
        assert!(write_provider_config("ftp://x", "k", None).is_err());
        assert!(write_provider_config("https://x", "", None).is_err());
    }

    #[test]
    fn prompt_variant_toggles_and_is_idempotent() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = TempHome::new("prompt");
        // Default when nothing set.
        assert_eq!(prompt_variant(&home.dir), PromptVariant::Default);

        set_prompt_variant("non_degraded").expect("set non-degraded");
        assert_eq!(prompt_variant(&home.dir), PromptVariant::NonDegraded);
        // The prompt file was dropped and the config points at it.
        assert!(home.dir.join(NONDEGRADED_PROMPT_FILE).exists());
        assert!(home.config().contains("model_instructions_file"));
        // Idempotent re-apply.
        set_prompt_variant("non_degraded").expect("re-apply");
        assert_eq!(prompt_variant(&home.dir), PromptVariant::NonDegraded);

        // Back to default clears our key.
        set_prompt_variant("default").expect("set default");
        assert_eq!(prompt_variant(&home.dir), PromptVariant::Default);
        assert!(!home.config().contains("model_instructions_file"));
    }

    #[test]
    fn default_does_not_disturb_a_user_prompt_file() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = TempHome::new("userprompt");
        std::fs::write(
            home.dir.join("config.toml"),
            "model_instructions_file = \"/home/me/my-prompt.md\"\n",
        )
        .expect("seed config");
        assert_eq!(prompt_variant(&home.dir), PromptVariant::Custom);
        // Setting default must NOT clear a prompt file the user configured.
        set_prompt_variant("default").expect("no-op default");
        assert!(home.config().contains("/home/me/my-prompt.md"));
    }

    #[test]
    fn status_flags_first_run_as_needing_setup() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _home = TempHome::new("firstrun");
        // Empty CODEX_HOME, no CODEX_ACCESS_TOKEN → needs setup.
        let prev = std::env::var_os("CODEX_ACCESS_TOKEN");
        std::env::remove_var("CODEX_ACCESS_TOKEN");
        let status = setup_status().expect("status");
        assert!(!status.has_config);
        assert!(!status.has_auth);
        assert!(!status.has_custom_provider);
        assert!(status.needs_setup);
        if let Some(v) = prev {
            std::env::set_var("CODEX_ACCESS_TOKEN", v);
        }
    }
}

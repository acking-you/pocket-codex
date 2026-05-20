//! Crate-wide error type.

use thiserror::Error;

/// Convenience alias for `Result<T, Error>` used by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All errors surfaced by `pocket-codex-core`.
///
/// New variants should be added here rather than re-using
/// [`anyhow::Error`] so callers can pattern-match on failure modes.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O failure while reading configuration or talking to the
    /// app-server.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialisation failure when handling app-server messages.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing/serialisation failure.
    #[error("toml deserialise error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    /// TOML serialisation failure.
    #[error("toml serialise error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// User-facing configuration error.
    #[error("config error: {0}")]
    Config(String),

    /// We could not determine a project path (e.g. no `$HOME`).
    #[error("path error: {0}")]
    Path(String),
}

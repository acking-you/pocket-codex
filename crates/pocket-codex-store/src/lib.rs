//! SQLite persistence for the Pocket-Codex hosted backend.
//!
//! A thin, focused store over `sqlx` (SQLite, WAL, a connection pool): just the
//! identity and credential tables — `users`, `refresh_tokens`, `device_flows`.
//! Live service liveness comes from the relay, not here. Queries are runtime
//! (no compile-time `DATABASE_URL` needed); timestamps are unix seconds
//! supplied by the caller so the store is clock-injectable and fully testable.

#![forbid(unsafe_code)]

mod error;
mod flows;
mod models;
mod tokens;
mod users;

use std::{str::FromStr, time::Duration};

pub use error::{Result, StoreError};
pub use models::{DeviceFlow, RefreshToken, User};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

/// Embedded migrations under `migrations/`, applied on [`Store::connect`].
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Handle to the backend's SQLite database.
#[derive(Debug, Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (creating if missing) the database at `database_url`
    /// (e.g. `sqlite:///var/lib/pocket-codex/backend.db` or `sqlite::memory:`),
    /// enable WAL, and run the embedded migrations.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self {
            pool,
        })
    }

    /// The underlying connection pool (for advanced/ad-hoc use).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh in-memory store pinned to a single connection so all queries
    /// share the one in-memory database for the duration of the test.
    async fn mem_store() -> Store {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("opts")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .connect_with(opts)
            .await
            .expect("connect");
        MIGRATOR.run(&pool).await.expect("migrate");
        Store {
            pool,
        }
    }

    #[tokio::test]
    async fn user_upsert_is_stable_across_logins() {
        let store = mem_store().await;
        let a = store
            .upsert_user(42, "octocat", 1000)
            .await
            .expect("insert");
        assert_eq!(a.github_login, "octocat");
        // Second login: same internal id, updated login + last_login_at.
        let b = store
            .upsert_user(42, "octocat-renamed", 2000)
            .await
            .expect("update");
        assert_eq!(b.id, a.id);
        assert_eq!(b.github_login, "octocat-renamed");
        assert_eq!(b.created_at, 1000);
        assert_eq!(b.last_login_at, 2000);
    }

    #[tokio::test]
    async fn refresh_token_lifecycle() {
        let store = mem_store().await;
        let user = store.upsert_user(7, "u", 0).await.expect("user");
        let id = store
            .insert_refresh_token(&user.id, b"hash1", Some("laptop"), 0, 10_000)
            .await
            .expect("insert");

        // Active lookup hits.
        let found = store
            .active_refresh_token(b"hash1", 5_000)
            .await
            .expect("lookup");
        assert_eq!(found.expect("some").id, id);

        // Expired lookup misses.
        assert!(store
            .active_refresh_token(b"hash1", 20_000)
            .await
            .expect("lookup")
            .is_none());

        // Revoke → no longer active.
        store
            .revoke_refresh_token(&id, 6_000, Some("next-id"))
            .await
            .expect("revoke");
        assert!(store
            .active_refresh_token(b"hash1", 5_000)
            .await
            .expect("lookup")
            .is_none());
    }

    #[tokio::test]
    async fn device_flow_consume_once() {
        let store = mem_store().await;
        store
            .insert_device_flow("h1", "dc1", 5, 0, 900)
            .await
            .expect("insert");
        let f = store.device_flow("h1").await.expect("get").expect("some");
        assert_eq!(f.github_device_code, "dc1");
        assert!(f.consumed_at.is_none());

        assert!(store.consume_device_flow("h1", 100).await.expect("consume"));
        // Second consume is a no-op (already consumed).
        assert!(!store.consume_device_flow("h1", 200).await.expect("consume"));
    }
}

//! Refresh-token queries. Only SHA-256 hashes are stored.

use uuid::Uuid;

use crate::{models::RefreshToken, Result, Store};

impl Store {
    /// Store a new refresh token (by its hash) for a user; returns the row id.
    pub async fn insert_refresh_token(
        &self,
        user_id: &str,
        token_hash: &[u8],
        device_label: Option<&str>,
        created_at: i64,
        expires_at: i64,
    ) -> Result<String> {
        let id = Uuid::new_v4().simple().to_string();
        sqlx::query(
            "INSERT INTO refresh_tokens (id, user_id, token_hash, device_label, created_at, \
             expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(id.as_str())
        .bind(user_id)
        .bind(token_hash)
        .bind(device_label)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Look up an active (not revoked, not expired at `now`) refresh token by
    /// hash.
    pub async fn active_refresh_token(
        &self,
        token_hash: &[u8],
        now: i64,
    ) -> Result<Option<RefreshToken>> {
        let token = sqlx::query_as::<_, RefreshToken>(
            "SELECT id, user_id, token_hash, device_label, created_at, expires_at, revoked_at, \
             rotated_to FROM refresh_tokens WHERE token_hash = ?1 AND revoked_at IS NULL AND \
             expires_at > ?2",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(token)
    }

    /// Revoke a refresh token (no-op if already revoked), optionally recording
    /// the id it was rotated into.
    pub async fn revoke_refresh_token(
        &self,
        id: &str,
        now: i64,
        rotated_to: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ?2, rotated_to = ?3 WHERE id = ?1 AND \
             revoked_at IS NULL",
        )
        .bind(id)
        .bind(now)
        .bind(rotated_to)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Revoke all of a user's active refresh tokens (full logout); returns the
    /// number revoked.
    pub async fn revoke_user_refresh_tokens(&self, user_id: &str, now: i64) -> Result<u64> {
        let res = sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }
}

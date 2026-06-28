//! Pending GitHub device-flow queries.

use crate::{models::DeviceFlow, Result, Store};

impl Store {
    /// Record a pending device flow keyed by its opaque handle, with an
    /// optional client device label to carry onto the eventual refresh
    /// token.
    pub async fn insert_device_flow(
        &self,
        handle: &str,
        github_device_code: &str,
        device_label: Option<&str>,
        interval_secs: i64,
        created_at: i64,
        expires_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO device_flows (handle, github_device_code, device_label, interval_secs, \
             created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(handle)
        .bind(github_device_code)
        .bind(device_label)
        .bind(interval_secs)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch a device flow by handle.
    pub async fn device_flow(&self, handle: &str) -> Result<Option<DeviceFlow>> {
        let flow = sqlx::query_as::<_, DeviceFlow>(
            "SELECT handle, github_device_code, interval_secs, created_at, expires_at, \
             consumed_at, device_label FROM device_flows WHERE handle = ?1",
        )
        .bind(handle)
        .fetch_optional(&self.pool)
        .await?;
        Ok(flow)
    }

    /// Mark a device flow consumed. Returns `true` if it was newly consumed,
    /// `false` if already consumed or missing (so authorization happens once).
    pub async fn consume_device_flow(&self, handle: &str, now: i64) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE device_flows SET consumed_at = ?2 WHERE handle = ?1 AND consumed_at IS NULL",
        )
        .bind(handle)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Delete expired device flows, web login flows, web exchange codes and
    /// refresh tokens (periodic cleanup).
    pub async fn purge_expired(&self, now: i64) -> Result<()> {
        for table in ["device_flows", "web_auth_flows", "web_exchange_codes", "refresh_tokens"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE expires_at < ?1"))
                .bind(now)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}

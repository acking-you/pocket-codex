//! Web (authorization-code) login-flow queries: the in-flight browser round-trip
//! (`web_auth_flows`) and the single-use exchange codes (`web_exchange_codes`).
//! Both enforce once-only consumption via a conditional UPDATE so a replayed
//! callback / redeem is a no-op.

use crate::{
    models::{WebAuthFlow, WebExchangeCode},
    Result, Store,
};

impl Store {
    /// Record a started web login flow keyed by its opaque `flow_id`, with the
    /// GitHub `gh_state` (matched on the callback) and the PKCE challenge.
    #[allow(clippy::too_many_arguments, reason = "one row's columns, mirrors the schema")]
    pub async fn insert_web_flow(
        &self,
        flow_id: &str,
        gh_state: &str,
        redirect_uri: &str,
        app_state: &str,
        code_challenge: &str,
        device_label: Option<&str>,
        created_at: i64,
        expires_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO web_auth_flows (flow_id, gh_state, redirect_uri, app_state, \
             code_challenge, device_label, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, \
             ?6, ?7, ?8)",
        )
        .bind(flow_id)
        .bind(gh_state)
        .bind(redirect_uri)
        .bind(app_state)
        .bind(code_challenge)
        .bind(device_label)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch a web login flow by the GitHub `gh_state` returned on the callback.
    pub async fn web_flow_by_state(&self, gh_state: &str) -> Result<Option<WebAuthFlow>> {
        let flow = sqlx::query_as::<_, WebAuthFlow>(
            "SELECT flow_id, gh_state, redirect_uri, app_state, code_challenge, device_label, \
             created_at, expires_at, consumed_at FROM web_auth_flows WHERE gh_state = ?1",
        )
        .bind(gh_state)
        .fetch_optional(&self.pool)
        .await?;
        Ok(flow)
    }

    /// Mark a web login flow consumed. Returns `true` if it was newly consumed,
    /// `false` if already consumed or missing (so the callback runs once).
    pub async fn consume_web_flow(&self, flow_id: &str, now: i64) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE web_auth_flows SET consumed_at = ?2 WHERE flow_id = ?1 AND consumed_at IS NULL",
        )
        .bind(flow_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Store a one-time exchange code for a user, carrying the PKCE challenge so
    /// the redeem can verify the client's code verifier.
    pub async fn insert_web_exchange(
        &self,
        code: &str,
        user_id: &str,
        device_label: Option<&str>,
        code_challenge: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO web_exchange_codes (code, user_id, device_label, code_challenge, \
             created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(code)
        .bind(user_id)
        .bind(device_label)
        .bind(code_challenge)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Look up an active (not consumed, not expired at `now`) exchange code.
    pub async fn active_web_exchange(
        &self,
        code: &str,
        now: i64,
    ) -> Result<Option<WebExchangeCode>> {
        let row = sqlx::query_as::<_, WebExchangeCode>(
            "SELECT code, user_id, device_label, code_challenge, created_at, expires_at, \
             consumed_at FROM web_exchange_codes WHERE code = ?1 AND consumed_at IS NULL AND \
             expires_at > ?2",
        )
        .bind(code)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Mark an exchange code redeemed. Returns `true` if this call won the
    /// redeem (single-use), `false` if it was already consumed or missing.
    pub async fn consume_web_exchange(&self, code: &str, now: i64) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE web_exchange_codes SET consumed_at = ?2 WHERE code = ?1 AND consumed_at IS \
             NULL",
        )
        .bind(code)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

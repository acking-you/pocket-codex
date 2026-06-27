//! User identity queries.

use uuid::Uuid;

use crate::{models::User, Result, Store};

impl Store {
    /// Insert or refresh a user by their GitHub id. On first login a stable
    /// internal id is generated (used as the relay-key namespace); on later
    /// logins the id is kept and the login/last-login are updated.
    pub async fn upsert_user(&self, github_id: i64, github_login: &str, now: i64) -> Result<User> {
        let id = Uuid::new_v4().simple().to_string();
        let user = sqlx::query_as::<_, User>(
            "INSERT INTO users (id, github_id, github_login, created_at, last_login_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(github_id) DO UPDATE SET
                 github_login = excluded.github_login,
                 last_login_at = excluded.last_login_at
             RETURNING id, github_id, github_login, created_at, last_login_at",
        )
        .bind(id)
        .bind(github_id)
        .bind(github_login)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    /// Fetch a user by internal id.
    pub async fn user(&self, id: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT id, github_id, github_login, created_at, last_login_at FROM users WHERE id = \
             ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }
}

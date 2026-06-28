//! The two GitHub Device Flow HTTP calls plus the `/user` profile fetch.
//!
//! GitHub's device flow is a pair of well-defined form POSTs; we make them
//! directly with `reqwest` rather than an OAuth helper because the backend
//! polls exactly once per client request (no blocking poll loop).

use serde::Deserialize;

use crate::error::{AuthError, Result};

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Device-code response from `POST /login/device/code`.
#[derive(Debug, Deserialize)]
pub(crate) struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

/// Outcome of one access-token poll.
pub(crate) enum PollResult {
    /// Not authorized yet.
    Pending,
    /// Polling too fast.
    SlowDown,
    /// Authorized; carries the GitHub access token.
    Authorized(String),
    /// The device code expired.
    Expired,
    /// The user denied the request.
    Denied,
}

/// GitHub user profile (`GET /user`).
#[derive(Debug, Deserialize)]
pub(crate) struct GhUser {
    pub id: i64,
    pub login: String,
}

/// Thin GitHub client bound to one OAuth app's `client_id`.
pub(crate) struct GitHub {
    http: reqwest::Client,
    client_id: String,
}

impl GitHub {
    pub(crate) fn new(http: reqwest::Client, client_id: String) -> Self {
        Self {
            http,
            client_id,
        }
    }

    /// Request a device + user code for the given scope.
    pub(crate) async fn request_device_code(&self, scope: &str) -> Result<DeviceCode> {
        let resp = self
            .http
            .post(DEVICE_CODE_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[("client_id", self.client_id.as_str()), ("scope", scope)])
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    /// Poll once for the access token (device flow needs no client secret).
    pub(crate) async fn poll_token(&self, device_code: &str) -> Result<PollResult> {
        let resp = self
            .http
            .post(ACCESS_TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("device_code", device_code),
                ("grant_type", DEVICE_GRANT),
            ])
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
            return Ok(PollResult::Authorized(token.to_string()));
        }
        match body.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => Ok(PollResult::Pending),
            Some("slow_down") => Ok(PollResult::SlowDown),
            Some("expired_token") => Ok(PollResult::Expired),
            Some("access_denied") => Ok(PollResult::Denied),
            Some(other) => Err(AuthError::Github(other.to_string())),
            None => Err(AuthError::Github("unexpected token response".to_string())),
        }
    }

    /// Build the browser authorization URL for the web (authorization-code)
    /// flow. `redirect_uri` is the backend's own public callback (which must
    /// exactly match the OAuth app's registered Authorization callback URL);
    /// `state` is the CSRF token GitHub echoes back.
    pub(crate) fn authorize_url(&self, scope: &str, redirect_uri: &str, state: &str) -> String {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", scope)
            .append_pair("state", state)
            .append_pair("allow_signup", "true")
            .finish();
        format!("{AUTHORIZE_URL}?{query}")
    }

    /// Exchange an authorization code for an access token (web flow). Unlike the
    /// device flow this requires the OAuth app's `client_secret`, which is held
    /// only on the backend.
    pub(crate) async fn exchange_code(
        &self,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<String> {
        let resp = self
            .http
            .post(ACCESS_TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", client_secret),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
            return Ok(token.to_string());
        }
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unexpected token response");
        Err(AuthError::Github(err.to_string()))
    }

    /// Fetch the authenticated user's id + login.
    pub(crate) async fn fetch_user(&self, access_token: &str) -> Result<GhUser> {
        let resp = self
            .http
            .get(USER_URL)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }
}

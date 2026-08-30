//! Tests for the auth service.
//!
//! Split out of `lib.rs` purely for size: the module had grown to 420 lines
//! beside 620 of implementation. Nothing here is public API.

use std::{
    io::{Error, ErrorKind},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};

use super::*;

#[test]
fn lost_response_retry_only_within_grace_of_a_rotation() {
    let rotated_at = 1_000;
    // A token rotated within the grace window → benign lost-response retry,
    // so refresh rejects this one request without nuking the family.
    assert!(is_lost_response_retry(Some("next"), Some(rotated_at), rotated_at));
    assert!(is_lost_response_retry(Some("next"), Some(rotated_at), rotated_at + REUSE_GRACE_SECS));
    // Rotated long ago → treat the replay as theft (revoke the family).
    assert!(!is_lost_response_retry(
        Some("next"),
        Some(rotated_at),
        rotated_at + REUSE_GRACE_SECS + 1
    ));
    // Revoked via logout (no successor) → theft, even if recent.
    assert!(!is_lost_response_retry(None, Some(rotated_at), rotated_at));
    // Never revoked → not an inactive-token replay at all.
    assert!(!is_lost_response_retry(Some("next"), None, rotated_at));
}

#[test]
fn redirect_allowlist_accepts_scheme_and_loopback_only() {
    // Mobile custom scheme.
    assert!(is_allowed_redirect("pocketcodex://auth"));
    assert!(is_allowed_redirect("pocketcodex://auth/callback"));
    // Desktop / CLI loopback.
    assert!(is_allowed_redirect("http://127.0.0.1:54321/callback"));
    assert!(is_allowed_redirect("http://localhost:8080"));
    // Rejected: arbitrary origins, https non-loopback, other schemes.
    assert!(!is_allowed_redirect("https://evil.example/callback"));
    assert!(!is_allowed_redirect("http://evil.example/callback"));
    assert!(!is_allowed_redirect("https://127.0.0.1/callback"));
    assert!(!is_allowed_redirect("javascript:alert(1)"));
    assert!(!is_allowed_redirect("not a url"));
}

#[test]
fn pkce_challenge_matches_rfc7636_vector() {
    // RFC 7636 Appendix B test vector.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(pkce_challenge(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn build_redirect_appends_query_for_scheme_and_url() {
    assert_eq!(
        build_redirect("pocketcodex://auth", &[("exchange_code", "abc"), ("state", "s1")]),
        "pocketcodex://auth?exchange_code=abc&state=s1"
    );
    // A target that already has a query gets `&`.
    assert_eq!(
        build_redirect("http://localhost:8080/cb?x=1", &[("error", "denied")]),
        "http://localhost:8080/cb?x=1&error=denied"
    );
    // Values are percent-encoded.
    assert_eq!(
        build_redirect("pocketcodex://auth", &[("error", "exchange failed")]),
        "pocketcodex://auth?error=exchange+failed"
    );
}

#[test]
fn github_poll_interval_bounds_provider_values() {
    assert_eq!(github_poll_interval(0), 5);
    assert_eq!(github_poll_interval(11), 11);
    assert_eq!(github_poll_interval(u64::MAX), 300);
}

fn cfg(web: bool) -> Config {
    Config {
        github_client_id: "Iv1.test".to_string(),
        github_client_secret: web.then(|| "client-secret".to_string()),
        github_scope: "read:user".to_string(),
        jwt_secret: "x".repeat(32),
        jwt_ttl_secs: 3600,
        refresh_ttl_secs: 1000,
        web_callback_url: web.then(|| "https://lb7666.top:8443/auth/web/callback".to_string()),
    }
}

#[tokio::test]
async fn web_start_gates_on_config_and_redirect_allowlist() {
    // Disabled flow → WebDisabled before touching the store, even for an
    // otherwise-valid redirect.
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let disabled = Auth::new(store, cfg(false)).expect("auth");
    assert!(!disabled.web_enabled());
    assert!(matches!(
        disabled
            .web_start("pocketcodex://auth", "s", "c", None, 0)
            .await,
        Err(AuthError::WebDisabled)
    ));

    // Enabled flow but a disallowed redirect → BadRedirect (returned before
    // any store write, so the in-memory pool is never queried).
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let enabled = Auth::new(store, cfg(true)).expect("auth");
    assert!(enabled.web_enabled());
    assert!(matches!(
        enabled
            .web_start("https://evil.example/cb", "s", "c", None, 0)
            .await,
        Err(AuthError::BadRedirect)
    ));
}

#[tokio::test]
async fn device_flow_authorizes_after_github_approval() {
    let github = FakeGithub::spawn().await;
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let auth = Auth::new_with_github_base(store, cfg(false), github.base_url()).expect("auth");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after unix epoch")
        .as_secs() as i64;

    let start = auth
        .device_start(Some("laptop"), now)
        .await
        .expect("device start");
    assert_eq!(start.user_code, "ABCD-EFGH");
    assert_eq!(start.verification_uri, format!("{}/login/device", github.base_url()));
    assert_eq!(start.interval_secs, 5);

    let pending = auth
        .device_poll(&start.poll_handle, now + 1)
        .await
        .expect("pending poll");
    assert_eq!(pending.status, DevicePollStatus::Pending);
    assert!(pending.credential.is_none());

    let slow_down = auth
        .device_poll(&start.poll_handle, now + 2)
        .await
        .expect("slow_down poll");
    assert_eq!(slow_down.status, DevicePollStatus::SlowDown);
    assert_eq!(slow_down.interval_secs, Some(11));
    assert!(slow_down.credential.is_none());

    let authorized = auth
        .device_poll(&start.poll_handle, now + 3)
        .await
        .expect("authorized poll");
    assert_eq!(authorized.status, DevicePollStatus::Authorized);
    assert_eq!(authorized.interval_secs, None);
    let credential = authorized.credential.expect("credential");
    assert_eq!(credential.login, "octocat");
    assert_eq!(credential.account_id.as_deref(), Some("42"));
    let claims = auth.verify(&credential.token).expect("session jwt");
    assert_eq!(claims.login, "octocat");
    assert_eq!(claims.gh_id, 42);
    assert_eq!(github.token_polls(), 3);

    let consumed = auth
        .device_poll(&start.poll_handle, now + 4)
        .await
        .expect("consumed poll");
    assert_eq!(consumed.status, DevicePollStatus::Expired);
    assert!(consumed.credential.is_none());
}

/// Sign in through the device flow and return the issued session.
///
/// Polls until the fake GitHub authorizes (it reports pending, then
/// slow_down, then approves every call after), so this works for a
/// SECOND sign-in in the same test — which is the whole point of the
/// recovery cases below.
async fn signed_in(auth: &Auth, now: i64) -> SessionCredential {
    let start = auth
        .device_start(Some("laptop"), now)
        .await
        .expect("device start");
    for at in 1..=5 {
        let poll = auth
            .device_poll(&start.poll_handle, now + at)
            .await
            .expect("poll");
        if let Some(credential) = poll.credential {
            return credential;
        }
    }
    panic!("the fake GitHub should have authorized within five polls");
}

#[tokio::test]
async fn a_retried_stale_refresh_cannot_lock_the_account_out() {
    // The lb7666.top lockout (2026-08-30), as a test. One lost refresh race
    // revoked the family; the client then retried the same dead token every few
    // seconds, and because each arrival was treated as fresh abuse it re-revoked
    // the family ~10x/minute — taking down any session the user created by
    // signing in again, so the account could not be recovered at all.
    let github = FakeGithub::spawn().await;
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let auth = Auth::new_with_github_base(store, cfg(false), github.base_url()).expect("auth");
    let now = 1_000_000;

    let first = signed_in(&auth, now).await;
    // Rotate once, well past the grace window, so the old token is genuinely
    // inactive and its replay is not a lost-response retry.
    let rotated = auth
        .refresh(&first.refresh_token, now + 10)
        .await
        .expect("first refresh rotates");
    let stale = first.refresh_token.clone();
    let long_after = now + 10 + REUSE_GRACE_SECS + 5;

    // First replay: theft response. The rotated session is revoked with it.
    assert!(matches!(auth.refresh(&stale, long_after).await, Err(AuthError::BadRefresh)));
    assert!(
        auth.refresh(&rotated.refresh_token, long_after + 1)
            .await
            .is_err(),
        "the compromised family must be revoked"
    );

    // The user signs in again. This is the session that must survive.
    let recovered = signed_in(&auth, long_after + 2).await;

    // The old client keeps retrying — it has no way to know it was revoked.
    for attempt in 0..5 {
        assert!(
            auth.refresh(&stale, long_after + 10 + attempt)
                .await
                .is_err(),
            "a dead token must keep being rejected"
        );
    }

    // ...and the recovered session still works. Before the fix, each of those
    // retries revoked it again and the user was thrown back to the sign-in
    // screen indefinitely.
    auth.refresh(&recovered.refresh_token, long_after + 30)
        .await
        .expect("a session created AFTER the abuse must survive the old client's retries");
}

#[tokio::test]
async fn revoking_a_family_is_idempotent_across_replays() {
    // The mechanism behind the test above: only the FIRST sighting of an
    // inactive token may revoke, because only it carries new information. A
    // client that retries is expected, not evidence of further abuse.
    let github = FakeGithub::spawn().await;
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let auth = Auth::new_with_github_base(store, cfg(false), github.base_url()).expect("auth");
    let now = 2_000_000;

    let session = signed_in(&auth, now).await;
    let live = auth
        .refresh(&session.refresh_token, now + 10)
        .await
        .expect("rotate");
    let stale = session.refresh_token;
    let after_grace = now + 10 + REUSE_GRACE_SECS + 1;

    assert!(auth.refresh(&stale, after_grace).await.is_err());
    // `live` is gone as a consequence of the first sighting.
    assert!(auth
        .refresh(&live.refresh_token, after_grace + 1)
        .await
        .is_err());

    // A session issued after that point is not collateral of later replays.
    let fresh = signed_in(&auth, after_grace + 2).await;
    assert!(auth.refresh(&stale, after_grace + 20).await.is_err());
    auth.refresh(&fresh.refresh_token, after_grace + 30)
        .await
        .expect("replays must not revoke sessions issued after the abuse");
}

struct FakeGithub {
    base_url: String,
    token_polls: Arc<AtomicUsize>,
}

impl FakeGithub {
    async fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake github");
        let addr = listener.local_addr().expect("fake github addr");
        let base_url = format!("http://{addr}");
        let token_polls = Arc::new(AtomicUsize::new(0));
        let server_base = base_url.clone();
        let server_polls = token_polls.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let base = server_base.clone();
                let polls = server_polls.clone();
                tokio::spawn(async move {
                    if let Ok((method, target, body)) = read_http_request(&mut stream).await {
                        let (status, json) =
                            fake_github_response(&base, &polls, &method, &target, &body);
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: \
                             application/json\r\nContent-Length: {}\r\nConnection: \
                             close\r\n\r\n{json}",
                            json.len()
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                });
            }
        });
        Self {
            base_url,
            token_polls,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn token_polls(&self) -> usize {
        self.token_polls.load(Ordering::SeqCst)
    }
}

async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<(String, String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "request closed before headers"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 16 * 1024 {
            return Err(Error::new(ErrorKind::InvalidData, "request headers too large"));
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = headers.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing method"))?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing target"))?
        .to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "request closed before body"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..header_end + content_length]).to_string();
    Ok((method, target, body))
}

fn fake_github_response(
    base_url: &str,
    token_polls: &AtomicUsize,
    method: &str,
    target: &str,
    body: &str,
) -> (&'static str, String) {
    match (method, target) {
        ("POST", "/login/device/code") if body.contains("client_id=Iv1.test") => (
            "200 OK",
            format!(
                r#"{{"device_code":"device-123","user_code":"ABCD-EFGH","verification_uri":"{base_url}/login/device","expires_in":900,"interval":1}}"#
            ),
        ),
        ("POST", "/login/oauth/access_token") if body.contains("device_code=device-123") => {
            match token_polls.fetch_add(1, Ordering::SeqCst) {
                0 => ("200 OK", r#"{"error":"authorization_pending"}"#.to_string()),
                1 => ("200 OK", r#"{"error":"slow_down","interval":11}"#.to_string()),
                _ => ("200 OK", r#"{"access_token":"gh-access"}"#.to_string()),
            }
        },
        ("GET", "/user") => ("200 OK", r#"{"id":42,"login":"octocat"}"#.to_string()),
        _ => {
            ("404 Not Found", format!(r#"{{"error":"unexpected {method} {target} body={body}"}}"#))
        },
    }
}

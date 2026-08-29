//! Backend integration: the real HTTP router over plain TCP.
//!
//! Auth is exercised with a directly-minted JWT (the GitHub flows need a live
//! OAuth app), proving everything downstream of login: `/healthz`, `/v1/me`,
//! and that every `/v1/*` route refuses an unauthenticated caller.
//!
//! # Why there is no tunnel test here any more
//!
//! The backend used to carry client traffic, so an end-to-end test had to stand
//! up a relay, register an echo service through the backend's broker, and
//! round-trip a payload. It no longer touches that path: it authenticates a
//! caller and hands out a relay credential, and the client takes it from there.
//! So what remains to test in-process is the HTTP contract.
//!
//! The relay-facing half (`/v1/relay` minting a real credential, `/v1/services`
//! listing) needs a live `pb-mapper server` holding a known administrator key.
//! The relay server crate is `publish = false`, so it cannot be spun up from
//! the registry — those cases run only when `PCX_TEST_RELAY` /
//! `PCX_TEST_RELAY_KEY` point at one, and are skipped (loudly) otherwise. Build
//! a relay from an `acking-you/pb-mapper` checkout
//! (`cargo build --release --bin pb-mapper`) to run them locally.

use std::sync::Arc;

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use pocket_codex_account_proto::http::{MeResponse, RelayCredentialResponse};
use pocket_codex_auth::{Auth, Claims};
use pocket_codex_backend::{router, AppState};
use pocket_codex_store::Store;
use tokio::net::TcpListener;

const JWT_SECRET: &str = "test-jwt-secret";

/// A relay to test against, from the environment. `None` skips the cases that
/// need one rather than failing them: a missing relay is a missing fixture, not
/// a defect in the code under test.
fn test_relay() -> Option<pocket_codex_pb::RelaySession> {
    let addr = std::env::var("PCX_TEST_RELAY").ok()?;
    let key = std::env::var("PCX_TEST_RELAY_KEY").ok()?;
    Some(pocket_codex_pb::RelaySession::new(addr, key))
}

fn mint(user: &str, login: &str, gh_id: i64) -> String {
    let claims = Claims {
        sub: user.to_string(),
        ns: format!("pcxu:{user}"),
        login: login.to_string(),
        gh_id,
        iat: 0,
        exp: 9_999_999_999,
        jti: "test".to_string(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .expect("mint jwt")
}

/// Start the real router on a loopback port and return its base URL.
async fn start_backend(relay: pocket_codex_pb::RelaySession) -> String {
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let auth = Arc::new(
        Auth::new(store, pocket_codex_auth::Config {
            github_client_id: "test-client".to_string(),
            github_client_secret: None,
            github_scope: "read:user".to_string(),
            jwt_secret: JWT_SECRET.to_string(),
            jwt_ttl_secs: 3600,
            refresh_ttl_secs: 1000,
            web_callback_url: None,
        })
        .expect("auth"),
    );
    let app = router(AppState {
        auth,
        credentials: pocket_codex_backend::Credentials::new(relay.clone()),
        relay,
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("http bind");
    let addr = listener.local_addr().expect("http addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_identity_and_auth_enforcement() {
    // No relay needed: none of these routes talk to one. That is the point —
    // authentication is the backend's own job and must work standalone.
    let base = start_backend(pocket_codex_pb::RelaySession::for_test("127.0.0.1:7666")).await;
    let client = reqwest::Client::new();
    let token = mint("userA", "octocat", 42);

    let r = client
        .get(format!("{base}/healthz"))
        .send()
        .await
        .expect("healthz");
    assert_eq!(r.status(), 200);
    assert_eq!(r.text().await.expect("healthz body"), "ok");

    let r = client
        .get(format!("{base}/v1/me"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("me");
    assert_eq!(r.status(), 200);
    let me: MeResponse = r.json().await.expect("me json");
    assert_eq!(me.login, "octocat");
    assert_eq!(me.account_id.as_deref(), Some("42"));

    // Every per-account route must refuse an unauthenticated caller. `/v1/relay`
    // matters most: it hands out a relay credential, so an unauthenticated 200
    // there would let anyone into some account's namespace.
    for path in ["/v1/me", "/v1/services", "/v1/relay"] {
        let r = client
            .get(format!("{base}{path}"))
            .send()
            .await
            .expect("unauthenticated request");
        assert_eq!(r.status(), 401, "{path} must require a bearer token");
    }
    // And a forged token must not pass, whatever it claims.
    let forged = encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: "userB".to_string(),
            ns: "pcxu:userb".to_string(),
            login: "attacker".to_string(),
            gh_id: 1,
            iat: 0,
            exp: 9_999_999_999,
            jti: "forged".to_string(),
        },
        &EncodingKey::from_secret(b"not-the-backend-secret"),
    )
    .expect("mint a token with the wrong secret");
    let r = client
        .get(format!("{base}/v1/relay"))
        .bearer_auth(&forged)
        .send()
        .await
        .expect("forged request");
    assert_eq!(r.status(), 401, "a token signed with another secret must not authenticate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_credential_is_minted_and_scoped_to_the_account() {
    let Some(relay) = test_relay() else {
        eprintln!(
            "skipping: set PCX_TEST_RELAY=host:port and PCX_TEST_RELAY_KEY=<32-byte admin key> to \
             run the relay-facing cases"
        );
        return;
    };
    let base = start_backend(relay).await;
    let client = reqwest::Client::new();

    let issued = |token: String| {
        let client = client.clone();
        let base = base.clone();
        async move {
            let r = client
                .get(format!("{base}/v1/relay"))
                .bearer_auth(&token)
                .send()
                .await
                .expect("relay credential");
            assert_eq!(r.status(), 200, "minting a credential failed");
            r.json::<RelayCredentialResponse>()
                .await
                .expect("relay credential json")
        }
    };

    let a = issued(mint("userA", "octocat", 42)).await;
    assert!(
        a.credential.starts_with("pbmt1_"),
        "clients must get a TEMPORARY credential, never the admin key: {}",
        a.credential
    );
    assert_eq!(a.namespace, "usera", "the namespace comes from the verified token");
    assert!(a.expires_at > 0, "a client needs an expiry to schedule its refresh against");

    // Asking twice returns the same credential: the backend caches and renews
    // rather than re-minting, because a temporary credential's namespace IS its
    // key id — a fresh one would move the account and orphan its registrations.
    let again = issued(mint("userA", "octocat", 42)).await;
    assert_eq!(again.credential, a.credential, "an account must keep one credential");
    assert_eq!(again.namespace, a.namespace);

    // A different account gets a different credential in a different namespace.
    let b = issued(mint("userB", "hubot", 7)).await;
    assert_ne!(b.credential, a.credential, "accounts must not share a credential");
    assert_eq!(b.namespace, "userb");
}

use std::{path::PathBuf, sync::Arc, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

use super::*;

struct TestAccount(PathBuf);

impl TestAccount {
    fn new(backend: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pcx-relay-{}-{id}", std::process::id()));
        let token = format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{}}}"#, unix_now() + 3600))
        );
        let mut config = Config::default();
        config.set_account_backend(backend);
        config.set_account_session(&token, "test-refresh", "alice", Some("alice".into()));
        save_config(&dir, &config).expect("save test account");
        Self(dir)
    }

    async fn cache(&self, expires_at: u64) -> Arc<RelayCache> {
        Arc::new(RelayCache {
            current: tokio::sync::Mutex::new(Some((
                CacheOwner::of(&load_config(&self.0).expect("read test account")),
                credential(expires_at),
            ))),
            fetching: tokio::sync::Mutex::new(()),
        })
    }

    fn switch_account(&self) {
        let mut config = load_config(&self.0).expect("read test account");
        let token = config.account_token().expect("test token").to_owned();
        config.set_account_session(&token, "bob-refresh", "bob", Some("bob".into()));
        save_config(&self.0, &config).expect("switch test account");
    }
}

impl Drop for TestAccount {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn credential(expires_at: u64) -> RelayCredentialResponse {
    RelayCredentialResponse {
        relay_addr: "127.0.0.1:7666".into(),
        credential: "test-relay-credential".into(),
        namespace: "alice".into(),
        expires_at,
    }
}

async fn backend(
    response: Option<RelayCredentialResponse>,
) -> (String, Arc<Notify>, Arc<Notify>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test backend");
    let url = format!("http://{}", listener.local_addr().expect("backend address"));
    let received = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let task = tokio::spawn({
        let received = received.clone();
        let release = release.clone();
        async move {
            let (mut socket, _) = listener.accept().await.expect("accept credential request");
            let mut request = Vec::new();
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut chunk = [0; 1024];
                let count = socket
                    .read(&mut chunk)
                    .await
                    .expect("read credential request");
                assert!(count > 0, "request closed before headers");
                request.extend_from_slice(&chunk[..count]);
            }
            assert!(request.starts_with(b"GET /v1/relay HTTP/1.1\r\n"));
            received.notify_one();
            release.notified().await;
            let (status, body) = match response {
                Some(value) => {
                    ("200 OK", serde_json::to_string(&value).expect("encode credential"))
                },
                None => ("503 Service Unavailable", String::new()),
            };
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: \
                         {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .expect("reply to credential request");
        }
    });
    (url, received, release, task)
}

#[tokio::test]
async fn failed_renewal_keeps_valid_credentials_available_even_while_backend_stalls() {
    let (url, received, release, server) = backend(None).await;
    let account = TestAccount::new(&url);
    // Inside the old five-minute cutoff, but still accepted by the relay.
    let old = credential(unix_now() as u64 + 60);
    let cache = account.cache(old.expires_at).await;
    let refresh = tokio::spawn({
        let cache = cache.clone();
        let support = account.0.clone();
        async move { cache.credential(&support, true).await }
    });
    tokio::time::timeout(Duration::from_secs(5), received.notified())
        .await
        .expect("renewal reached backend");
    let during =
        tokio::time::timeout(Duration::from_millis(500), cache.credential(&account.0, false))
            .await
            .expect("cached reads must not wait for backend")
            .expect("old credential remains valid");
    assert_eq!(during, old);
    release.notify_one();
    assert!(refresh.await.expect("renewal task").is_err());
    server.await.expect("backend task");
    assert_eq!(
        cache
            .credential(&account.0, false)
            .await
            .expect("cached credential"),
        old
    );
}

#[tokio::test]
async fn expired_credentials_require_the_issuer() {
    let account = TestAccount::new("http://127.0.0.1:0");
    let cache = account.cache(unix_now() as u64).await;
    assert!(cache.credential(&account.0, false).await.is_err());
}

#[tokio::test]
async fn another_account_cannot_use_the_cached_credential_during_an_outage() {
    let account = TestAccount::new("http://127.0.0.1:0");
    let cache = account.cache(unix_now() as u64 + 3600).await;
    account.switch_account();
    assert!(cache.credential(&account.0, false).await.is_err());
}

#[tokio::test]
async fn successful_renewal_replaces_the_cached_expiry() {
    let renewed = credential(unix_now() as u64 + 7200);
    let (url, _, release, server) = backend(Some(renewed.clone())).await;
    let account = TestAccount::new(&url);
    let cache = account.cache(unix_now() as u64 + 60).await;
    release.notify_one();
    assert_eq!(
        cache
            .credential(&account.0, true)
            .await
            .expect("renew credential"),
        renewed
    );
    server.await.expect("backend task");
    assert_eq!(
        cache
            .credential(&account.0, false)
            .await
            .expect("cached credential"),
        renewed
    );
}

#[tokio::test]
async fn an_account_switch_during_renewal_discards_the_response() {
    let (url, received, release, server) =
        backend(Some(credential(unix_now() as u64 + 7200))).await;
    let account = TestAccount::new(&url);
    let cache = account.cache(unix_now() as u64 + 60).await;
    let refresh = tokio::spawn({
        let cache = cache.clone();
        let support = account.0.clone();
        async move { cache.credential(&support, true).await }
    });
    tokio::time::timeout(Duration::from_secs(5), received.notified())
        .await
        .expect("renewal reached backend");
    account.switch_account();
    release.notify_one();
    let error = refresh
        .await
        .expect("renewal task")
        .expect_err("identity changed");
    assert!(error.to_string().contains("account changed"));
    server.await.expect("backend task");
    assert!(cache.credential(&account.0, false).await.is_err());
}

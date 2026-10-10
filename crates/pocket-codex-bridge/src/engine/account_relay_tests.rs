use std::{path::PathBuf, sync::Arc, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
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

async fn login_backend(web: bool, login: &str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("login listener");
    let url = format!("http://{}", listener.local_addr().expect("login address"));
    let cred = serde_json::json!({
        "token": "new-token", "refresh_token": "new-refresh", "expires_in_secs": 3600,
        "login": login, "account_id": login,
    });
    let body = if web {
        serde_json::json!({"credential": cred})
    } else {
        serde_json::json!({"status": "authorized", "credential": cred})
    }
    .to_string();
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("login request");
        let mut reader = tokio::io::BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("request line");
        let path = if web { "/auth/web/exchange" } else { "/auth/device/poll" };
        assert!(line.starts_with(&format!("POST {path} ")));
        let mut length = 0;
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).await.expect("header") > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().expect("content length");
                }
            }
        }
        reader
            .read_exact(&mut vec![0; length])
            .await
            .expect("request body");
        reader
            .get_mut()
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .expect("login response");
    });
    (url, task)
}

#[tokio::test]
async fn successful_login_retires_obsolete_refresher_without_resolving_a_tunnel() {
    struct ResetRefresher;
    impl Drop for ResetRefresher {
        fn drop(&mut self) {
            stop_credential_refresh();
        }
    }
    let _reset = ResetRefresher;
    for web in [false, true] {
        for (login, change_backend) in [("bob", false), ("alice", true), ("alice", false)] {
            let (url, server) = login_backend(web, login).await;
            let account =
                TestAccount::new(if change_backend { "https://old.example" } else { &url });
            start_credential_refresh(&account.0, unix_now() as u64 + 86400)
                .expect("start old refresher");
            let old = credential_refresh()
                .lock()
                .expect("refresher lock")
                .current
                .as_ref()
                .expect("old task")
                .3
                .abort_handle();
            let outcome = if web {
                web_login_exchange(&account.0, &url, "exchange".into(), "verifier".into()).await
            } else {
                device_poll(&account.0, &url, "poll".into()).await
            }
            .expect("successful login");
            assert!(matches!(outcome, PollOutcome::Authorized { .. }));
            server.await.expect("login backend task");
            tokio::task::yield_now().await;
            assert_eq!(
                old.is_finished(),
                login != "alice" || change_backend,
                "login must retire only obsolete tasks without waiting for another tunnel"
            );
            let config = load_config(&account.0).expect("new account");
            assert_eq!(config.account_login(), Some(login));
            assert_eq!(config.account_backend(), Some(url.as_str()));
            stop_credential_refresh();
        }
    }
}

#[tokio::test]
async fn waiting_refresh_never_sends_replacement_credentials_to_previous_backend() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let account = TestAccount::new(&url);
    update_config(&account.0, |cfg| {
        cfg.set_account_session("expired", "alice-refresh", "alice", Some("alice".into()));
        Ok(())
    })
    .expect("expire");
    let gate = refresh_lock().lock().await;
    let mut original = load_config(&account.0).expect("read");
    let mut request = Box::pin(valid_token(&account.0, &mut original, &url));
    // Poll into the actual refresh lock before changing backend/account.
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut request)
        .await
        .is_err());
    account.switch_account();
    update_config(&account.0, |cfg| {
        cfg.set_account_backend("http://127.0.0.1:1");
        Ok(())
    })
    .expect("switch backend");
    drop(gate);
    assert!(request
        .await
        .expect_err("reject stale owner")
        .to_string()
        .contains("account changed"));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err(),
        "no credential may reach previous backend"
    );
}

#[tokio::test]
async fn refresh_response_cannot_restore_a_replaced_session_or_erase_settings() {
    for replace in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let account = TestAccount::new(&url);
        update_config(&account.0, |cfg| {
            cfg.set_account_session("expired", "alice-refresh", "alice", Some("alice".into()));
            Ok(())
        })
        .expect("expire");
        let (arrived, waiting) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut socket = tokio::io::BufReader::new(socket);
            let mut header = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                socket.read_line(&mut line).await.expect("headers");
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().expect("length");
                }
                header.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            assert!(header.starts_with("POST /auth/refresh "));
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.expect("body");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).expect("json")["refresh_token"],
                "alice-refresh"
            );
            arrived.send(()).expect("signal");
            released.await.expect("release");
            let body = serde_json::json!({"credential":{"token":"refreshed-alice", "refresh_token":"rotated-alice", "expires_in_secs":3600, "login":"alice", "account_id":"alice"}}).to_string();
            socket
                .get_mut()
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                         {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .expect("reply");
        });
        let request = tokio::spawn({
            let path = account.0.clone();
            async move {
                let mut cfg = load_config(&path).expect("config");
                valid_token(&path, &mut cfg, &url).await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("request reached server")
            .expect("signal");
        if replace {
            account.switch_account();
        }
        update_config(&account.0, |cfg| {
            cfg.set_locale("zh");
            cfg.set_relay("updated:7666");
            Ok(())
        })
        .expect("settings");
        release.send(()).expect("release");
        let result = request.await.expect("task");
        server.await.expect("server");
        let cfg = load_config(&account.0).expect("saved");
        assert_eq!(cfg.relay(), Some("updated:7666"));
        assert_eq!(cfg.locale(), Some("zh"));
        if replace {
            assert!(result.is_err());
            assert_eq!(cfg.account_login(), Some("bob"));
            assert_eq!(cfg.account_refresh_token(), Some("bob-refresh"));
        } else {
            assert_eq!(result.expect("same session"), "refreshed-alice");
            assert_eq!(cfg.account_refresh_token(), Some("rotated-alice"));
        }
    }
}

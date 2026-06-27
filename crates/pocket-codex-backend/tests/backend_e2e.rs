//! Backend integration: the real HTTP router + broker (plain TCP) over a real
//! loopback pb-mapper relay. Auth is exercised with a directly-minted JWT (the
//! GitHub device flow needs a live OAuth app), proving everything downstream of
//! login: `/v1/me`, broker register, `/v1/services`, broker subscribe + echo.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use pocket_codex_account_proto::http::{MeResponse, ServicesResponse};
use pocket_codex_auth::{Auth, Claims};
use pocket_codex_backend::{router, AppState, AuthVerifier};
use pocket_codex_broker_client::{
    run_register, run_subscribe, BrokerError, BrokerStream, Connector, RegisterConfig,
    SubscribeConfig, TokenProvider,
};
use pocket_codex_broker_server::BrokerServer;
use pocket_codex_core::service::ServiceKind;
use pocket_codex_store::Store;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef";
const JWT_SECRET: &str = "test-jwt-secret";

struct TcpConnector {
    addr: SocketAddr,
}

#[async_trait::async_trait]
impl Connector for TcpConnector {
    async fn connect(&self) -> Result<Box<dyn BrokerStream>, BrokerError> {
        Ok(Box::new(TcpStream::connect(self.addr).await?))
    }
}

struct StaticToken(String);

#[async_trait::async_trait]
impl TokenProvider for StaticToken {
    async fn token(&self) -> Result<String, BrokerError> {
        Ok(self.0.clone())
    }
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

async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("probe bind");
    let port = listener.local_addr().expect("probe addr").port();
    drop(listener);
    port
}

async fn wait_for_key(relay: SocketAddr, key: &str) {
    for _ in 0..200 {
        if let Ok(keys) = pocket_codex_pb::keys(relay).await {
            if keys.iter().any(|k| k == key) {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("relay never registered key: {key}");
}

async fn try_echo(addr: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
    let mut stream = TcpStream::connect(addr).await.ok()?;
    stream.write_all(payload).await.ok()?;
    let mut buf = vec![0u8; payload.len()];
    match tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buf)).await {
        Ok(Ok(_)) => Some(buf),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backend_http_and_broker_end_to_end() {
    pocket_codex_pb::set_msg_header_key(Some(TEST_KEY)).expect("set key");

    // Relay.
    let relay_port = free_port().await;
    let relay_addr_s = format!("127.0.0.1:{relay_port}");
    let relay_sock: SocketAddr = relay_addr_s.parse().expect("relay sock");
    {
        let addr = relay_addr_s.clone();
        tokio::spawn(async move {
            let _ = pb_mapper::pb_server::run_server(addr).await;
        });
    }
    for _ in 0..200 {
        if TcpStream::connect(&relay_addr_s).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // Auth + HTTP router.
    let store = Store::connect("sqlite::memory:").await.expect("store");
    let auth = Arc::new(
        Auth::new(store, pocket_codex_auth::Config {
            github_client_id: "test-client".to_string(),
            github_scope: "read:user".to_string(),
            jwt_secret: JWT_SECRET.to_string(),
            jwt_ttl_secs: 3600,
            refresh_ttl_secs: 1000,
        })
        .expect("auth"),
    );
    // Broker, shared into the HTTP AppState so DELETE /v1/services can drop keys.
    let broker = BrokerServer::new(
        Arc::new(AuthVerifier(auth.clone())),
        relay_addr_s.clone(),
        Duration::from_secs(60),
    );
    let app = router(AppState {
        auth: auth.clone(),
        relay_addr: relay_sock,
        broker: broker.clone(),
    });
    let http_listener = TcpListener::bind("127.0.0.1:0").await.expect("http bind");
    let http_addr = http_listener.local_addr().expect("http addr");
    tokio::spawn(async move {
        let _ = axum::serve(http_listener, app.into_make_service()).await;
    });

    let broker_listener = TcpListener::bind("127.0.0.1:0").await.expect("broker bind");
    let broker_addr = broker_listener.local_addr().expect("broker addr");
    {
        let broker = broker.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = broker_listener.accept().await {
                let broker = broker.clone();
                tokio::spawn(async move { broker.handle_connection(stream).await });
            }
        });
    }

    // Echo app-server.
    let echo = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
    let echo_addr = echo.local_addr().expect("echo addr");
    tokio::spawn(async move {
        while let Ok((mut s, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let token = mint("userA", "octocat", 42);
    let client = reqwest::Client::new();
    let base = format!("http://{http_addr}");

    // healthz
    let r = client
        .get(format!("{base}/healthz"))
        .send()
        .await
        .expect("healthz");
    assert_eq!(r.status(), 200);
    assert_eq!(r.text().await.expect("healthz body"), "ok");

    // /v1/me with + without a token
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

    let r = client
        .get(format!("{base}/v1/me"))
        .send()
        .await
        .expect("me noauth");
    assert_eq!(r.status(), 401);

    // Register an echo service under the account.
    let connector: Arc<dyn Connector> = Arc::new(TcpConnector {
        addr: broker_addr,
    });
    let tokens: Arc<dyn TokenProvider> = Arc::new(StaticToken(token.clone()));
    tokio::spawn(run_register(connector.clone(), tokens.clone(), RegisterConfig {
        device: "dev".to_string(),
        kind: ServiceKind::App,
        name: "default".to_string(),
        client_instance_id: "test".to_string(),
        local_addr: echo_addr,
        idle: Duration::from_secs(60),
    }));
    wait_for_key(relay_sock, "pcxu:usera:dev:app:default").await;

    // /v1/services lists it (prefix stripped to device/kind/name).
    let r = client
        .get(format!("{base}/v1/services"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("services");
    assert_eq!(r.status(), 200);
    let services: ServicesResponse = r.json().await.expect("services json");
    assert!(
        services
            .services
            .iter()
            .any(|s| s.device == "dev" && s.kind == ServiceKind::App && s.name == "default"),
        "expected dev/app/default in {services:?}"
    );

    // Subscribe and round-trip an echo through the whole backend chain.
    let sub_listener = TcpListener::bind("127.0.0.1:0").await.expect("sub bind");
    let sub_addr = sub_listener.local_addr().expect("sub addr");
    tokio::spawn(run_subscribe(
        connector,
        tokens,
        SubscribeConfig {
            device: "dev".to_string(),
            kind: ServiceKind::App,
            name: "default".to_string(),
            idle: Duration::from_secs(60),
        },
        sub_listener,
    ));

    let payload = b"backend round-trip";
    let mut got = None;
    for _ in 0..50 {
        if let Some(v) = try_echo(sub_addr, payload).await {
            got = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(got.as_deref(), Some(payload.as_slice()));
}

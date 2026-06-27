//! End-to-end data-path test: a real loopback pb-mapper relay, the broker
//! server, and two broker clients (register + subscribe) bridging an echo
//! "app-server". Proves the whole chain carries bytes both ways:
//!
//! ```text
//!   consumer ⇄ run_subscribe ⇄ SubscribeData ⇄ broker ⇄ pb-subscribe ⇄
//!     relay ⇄ pb-register ⇄ broker seam ⇄ RegisterData ⇄ run_register ⇄ echo
//! ```
//!
//! All in one process, sharing one `MSG_HEADER_KEY`, so no real network or the
//! real relay key is involved.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use pocket_codex_broker_client::{
    run_register, run_subscribe, BrokerError, BrokerStream, Connector, RegisterConfig,
    SubscribeConfig, TokenProvider,
};
use pocket_codex_broker_server::{BrokerServer, TokenVerifier};
use pocket_codex_core::service::ServiceKind;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

/// Exactly 32 bytes — pb-mapper requires a 32-byte header key.
const TEST_KEY: &str = "0123456789abcdef0123456789abcdef";

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

struct StaticVerifier {
    token: String,
    user: String,
}

impl TokenVerifier for StaticVerifier {
    fn verify(&self, token: &str) -> Option<String> {
        (token == self.token).then(|| self.user.clone())
    }
}

/// Verifier over several `(token, user)` pairs; an unknown token is rejected.
struct MapVerifier(Vec<(String, String)>);

impl TokenVerifier for MapVerifier {
    fn verify(&self, token: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, user)| user.clone())
    }
}

async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = listener.local_addr().expect("probe addr").port();
    drop(listener);
    port
}

async fn wait_connectable(addr: &str) {
    for _ in 0..200 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("address never became connectable: {addr}");
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

async fn try_echo(sub_addr: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
    let mut stream = TcpStream::connect(sub_addr).await.ok()?;
    stream.write_all(payload).await.ok()?;
    let mut buf = vec![0u8; payload.len()];
    match tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buf)).await {
        Ok(Ok(_)) => Some(buf),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_to_end_register_subscribe_echo() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // One process-global key shared by the relay + both pb-mapper legs.
    pocket_codex_pb::set_msg_header_key(Some(TEST_KEY)).expect("set msg header key");

    // 1. A real pb-mapper relay on a reserved loopback port.
    let relay_port = free_port().await;
    let relay_addr_s = format!("127.0.0.1:{relay_port}");
    let relay_shutdown = CancellationToken::new();
    {
        let token = relay_shutdown.clone();
        let addr = relay_addr_s.clone();
        tokio::spawn(async move {
            let _ = pb_mapper::pb_server::run_server_with_shutdown(addr, token, None).await;
        });
    }
    wait_connectable(&relay_addr_s).await;

    // 2. The broker server, plain TCP in the test (TLS is the backend's job).
    let verifier = Arc::new(StaticVerifier {
        token: "tok-A".to_string(),
        user: "userA".to_string(),
    });
    let broker = BrokerServer::new(verifier, relay_addr_s.clone(), Duration::from_secs(60));
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

    // 3. The echo "app-server" the register client exposes.
    let echo = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
    let echo_addr = echo.local_addr().expect("echo addr");
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        },
                    }
                }
            });
        }
    });

    let connector: Arc<dyn Connector> = Arc::new(TcpConnector {
        addr: broker_addr,
    });
    let tokens: Arc<dyn TokenProvider> = Arc::new(StaticToken("tok-A".to_string()));

    // 4. Register: expose the echo server under the account.
    tokio::spawn(run_register(connector.clone(), tokens.clone(), RegisterConfig {
        device: "dev".to_string(),
        kind: ServiceKind::App,
        name: "default".to_string(),
        client_instance_id: "test-instance".to_string(),
        local_addr: echo_addr,
        idle: Duration::from_secs(60),
    }));

    // The backend namespaces the key under the verified user.
    let relay_sock: SocketAddr = relay_addr_s.parse().expect("relay sockaddr");
    wait_for_key(relay_sock, "pcxu:usera:dev:app:default").await;

    // 5. Subscribe: a local listener the consumer dials.
    let sub_listener = TcpListener::bind("127.0.0.1:0").await.expect("sub bind");
    let sub_addr = sub_listener.local_addr().expect("sub addr");
    tokio::spawn(run_subscribe(
        connector.clone(),
        tokens.clone(),
        SubscribeConfig {
            device: "dev".to_string(),
            kind: ServiceKind::App,
            name: "default".to_string(),
            idle: Duration::from_secs(60),
        },
        sub_listener,
    ));

    // 6. Drive an echo through the entire chain (retry while the path warms up).
    let payload = b"hello pocket-codex broker";
    let mut got = None;
    for _ in 0..50 {
        if let Some(v) = try_echo(sub_addr, payload).await {
            got = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        got.as_deref(),
        Some(payload.as_slice()),
        "payload never round-tripped through the broker chain"
    );

    relay_shutdown.cancel();
}

/// Spawn a real loopback pb-mapper relay; returns its address + a shutdown
/// token.
async fn spawn_relay() -> (String, CancellationToken) {
    let relay_port = free_port().await;
    let relay_addr_s = format!("127.0.0.1:{relay_port}");
    let relay_shutdown = CancellationToken::new();
    {
        let token = relay_shutdown.clone();
        let addr = relay_addr_s.clone();
        tokio::spawn(async move {
            let _ = pb_mapper::pb_server::run_server_with_shutdown(addr, token, None).await;
        });
    }
    wait_connectable(&relay_addr_s).await;
    (relay_addr_s, relay_shutdown)
}

/// Spawn the broker server (plain TCP) over `verifier`; returns its address.
async fn spawn_broker(verifier: Arc<dyn TokenVerifier>, relay: String) -> SocketAddr {
    let broker = BrokerServer::new(verifier, relay, Duration::from_secs(60));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("broker bind");
    let addr = listener.local_addr().expect("broker addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let broker = broker.clone();
            tokio::spawn(async move { broker.handle_connection(stream).await });
        }
    });
    addr
}

/// Spawn an echo "app-server"; returns its address.
async fn spawn_echo() -> SocketAddr {
    let echo = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
    let addr = echo.local_addr().expect("echo addr");
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        },
                    }
                }
            });
        }
    });
    addr
}

/// Subscribe as `token` to `dev/app/default` on `connector`, then attempt an
/// echo; returns the echoed bytes (Some) or None if the path never paired.
async fn subscribe_and_try_echo(
    connector: Arc<dyn Connector>,
    token: &str,
    payload: &[u8],
    attempts: usize,
) -> Option<Vec<u8>> {
    let tokens: Arc<dyn TokenProvider> = Arc::new(StaticToken(token.to_string()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("sub bind");
    let addr = listener.local_addr().expect("sub addr");
    tokio::spawn(run_subscribe(
        connector,
        tokens,
        SubscribeConfig {
            device: "dev".to_string(),
            kind: ServiceKind::App,
            name: "default".to_string(),
            idle: Duration::from_secs(60),
        },
        listener,
    ));
    for _ in 0..attempts {
        if let Some(v) = try_echo(addr, payload).await {
            return Some(v);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

/// The crate's reason to exist: a second account cannot reach the first's
/// service, and an unknown token cannot reach any service — while the owner
/// can.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_user_isolation_and_unauthorized_are_enforced() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    pocket_codex_pb::set_msg_header_key(Some(TEST_KEY)).expect("set msg header key");

    let (relay_addr_s, relay_shutdown) = spawn_relay().await;
    // Two distinct accounts share one broker.
    let verifier = Arc::new(MapVerifier(vec![
        ("tok-A".to_string(), "userA".to_string()),
        ("tok-B".to_string(), "userB".to_string()),
    ]));
    let broker_addr = spawn_broker(verifier, relay_addr_s.clone()).await;
    let echo_addr = spawn_echo().await;
    let connector: Arc<dyn Connector> = Arc::new(TcpConnector {
        addr: broker_addr,
    });

    // userA registers the echo under dev/app/default.
    let tokens_a: Arc<dyn TokenProvider> = Arc::new(StaticToken("tok-A".to_string()));
    tokio::spawn(run_register(connector.clone(), tokens_a, RegisterConfig {
        device: "dev".to_string(),
        kind: ServiceKind::App,
        name: "default".to_string(),
        client_instance_id: "a".to_string(),
        local_addr: echo_addr,
        idle: Duration::from_secs(60),
    }));
    let relay_sock: SocketAddr = relay_addr_s.parse().expect("relay sockaddr");
    wait_for_key(relay_sock, "pcxu:usera:dev:app:default").await;

    // Positive control: userA reaches its own service (also warms the path).
    let own = subscribe_and_try_echo(connector.clone(), "tok-A", b"mine", 50).await;
    assert_eq!(own.as_deref(), Some(b"mine".as_slice()), "userA must reach its own service");

    // Isolation: userB subscribes to the SAME device/kind/name. The broker
    // derives pcxu:userb:dev:app:default — a key the relay has no registration
    // for — so the echo must never round-trip.
    let cross = subscribe_and_try_echo(connector.clone(), "tok-B", b"theirs", 20).await;
    assert_eq!(cross, None, "userB must NOT reach userA's service (cross-user isolation)");

    // Unauthorized: an unknown token is rejected at the hello, so it reaches
    // nothing either.
    let unauth = subscribe_and_try_echo(connector.clone(), "tok-bogus", b"nope", 20).await;
    assert_eq!(unauth, None, "an unauthorized token must not reach any service");

    relay_shutdown.cancel();
}

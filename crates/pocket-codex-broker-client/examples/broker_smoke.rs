//! On-server smoke test: register an echo through a RUNNING backend broker over
//! TLS, subscribe to it, and round-trip a payload — proving the deployed broker
//! and the real relay carry bytes both ways. Trusts the system root store.
//!
//! ```sh
//! POCKET_CODEX_TOKEN=<jwt> cargo run -p pocket-codex-broker-client \
//!     --example broker_smoke -- lb7666.top 7900
//! ```

use std::{net::SocketAddr, sync::Arc, time::Duration};

use pocket_codex_broker_client::{
    run_register, run_subscribe, BrokerError, BrokerStream, Connector, RegisterConfig,
    SubscribeConfig, TokenProvider,
};
use pocket_codex_core::service::ServiceKind;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct Tls {
    host: String,
    addr: String,
    tls: tokio_rustls::TlsConnector,
}

#[async_trait::async_trait]
impl Connector for Tls {
    async fn connect(&self) -> Result<Box<dyn BrokerStream>, BrokerError> {
        let tcp = TcpStream::connect(&self.addr).await?;
        let sni = rustls::pki_types::ServerName::try_from(self.host.clone())
            .map_err(|e| BrokerError::Token(e.to_string()))?;
        Ok(Box::new(self.tls.connect(sni, tcp).await.map_err(BrokerError::Io)?))
    }
}

struct Tok(String);

#[async_trait::async_trait]
impl TokenProvider for Tok {
    async fn token(&self) -> Result<String, BrokerError> {
        Ok(self.0.clone())
    }
}

async fn try_echo(addr: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
    let mut s = TcpStream::connect(addr).await.ok()?;
    s.write_all(payload).await.ok()?;
    let mut buf = vec![0u8; payload.len()];
    match tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut buf)).await {
        Ok(Ok(_)) => Some(buf),
        _ => None,
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "lb7666.top".to_string());
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(7900);
    let token = std::env::var("POCKET_CODEX_TOKEN").expect("POCKET_CODEX_TOKEN env required");

    let mut roots = rustls::RootCertStore::empty();
    for c in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(c);
    }
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    // SNI stays `host` (cert hostname); the TCP target can be overridden so the
    // server can dial 127.0.0.1:<port> while the cert still verifies as `host`.
    let connect_addr =
        std::env::var("POCKET_CODEX_CONNECT").unwrap_or_else(|_| format!("{host}:{port}"));
    let connector: Arc<dyn Connector> = Arc::new(Tls {
        host: host.clone(),
        addr: connect_addr,
        tls: tokio_rustls::TlsConnector::from(Arc::new(cfg)),
    });
    let tokens: Arc<dyn TokenProvider> = Arc::new(Tok(token));

    // Echo "app-server".
    let echo = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
    let echo_addr = echo.local_addr().expect("echo addr");
    tokio::spawn(async move {
        while let Ok((mut s, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                while let Ok(n) = s.read(&mut b).await {
                    if n == 0 || s.write_all(&b[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    tokio::spawn(run_register(connector.clone(), tokens.clone(), RegisterConfig {
        device: "smokedev".to_string(),
        kind: ServiceKind::App,
        name: "default".to_string(),
        client_instance_id: "smoke".to_string(),
        local_addr: echo_addr,
        idle: Duration::from_secs(60),
    }));
    tokio::time::sleep(Duration::from_secs(3)).await;

    let sub = TcpListener::bind("127.0.0.1:0").await.expect("sub bind");
    let sub_addr = sub.local_addr().expect("sub addr");
    tokio::spawn(run_subscribe(
        connector,
        tokens,
        SubscribeConfig {
            device: "smokedev".to_string(),
            kind: ServiceKind::App,
            name: "default".to_string(),
            idle: Duration::from_secs(60),
        },
        sub,
    ));

    let payload = b"hello deployed broker";
    for _ in 0..40 {
        if let Some(v) = try_echo(sub_addr, payload).await {
            if v == payload {
                println!("SMOKE_OK: round-tripped {} bytes through the deployed broker", v.len());
                std::process::exit(0);
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    eprintln!("SMOKE_FAIL: payload never round-tripped");
    std::process::exit(1);
}

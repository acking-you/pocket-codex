//! Register-loop behavior against a scripted broker: a key-conflict nack is
//! FATAL (the loop returns instead of reconnect-looping — the duplicate-name
//! storm guard), while ordinary rejections stay transient (the loop retries
//! and succeeds on the next attempt).

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use pocket_codex_account_proto::{
    broker::{BrokerAck, BrokerHello},
    frame::{read_frame, write_frame},
};
use pocket_codex_broker_client::{
    run_register, BrokerError, BrokerStream, Connector, RegisterConfig, TokenProvider,
};
use pocket_codex_core::service::ServiceKind;
use tokio::io::{duplex, DuplexStream};

/// Hands out one in-memory tunnel per connect; the paired server half goes to
/// a per-connection script chosen by attempt number.
struct ScriptedConnector {
    attempts: AtomicUsize,
    script: fn(usize, DuplexStream),
}

#[async_trait::async_trait]
impl Connector for ScriptedConnector {
    async fn connect(&self) -> Result<Box<dyn BrokerStream>, BrokerError> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst);
        let (client, server) = duplex(64 * 1024);
        (self.script)(n, server);
        Ok(Box::new(client))
    }
}

struct StaticToken;

#[async_trait::async_trait]
impl TokenProvider for StaticToken {
    async fn token(&self) -> Result<String, BrokerError> {
        Ok("tok".to_string())
    }
}

fn cfg() -> RegisterConfig {
    let local: SocketAddr = "127.0.0.1:9".parse().expect("addr");
    RegisterConfig {
        device: "dev".to_string(),
        kind: ServiceKind::App,
        name: "default".to_string(),
        client_instance_id: "inst".to_string(),
        local_addr: local,
        idle: Duration::from_secs(5),
    }
}

/// Read the hello then answer with `ack` and hold the stream open briefly (so
/// the client's ack read never races the close).
fn answer(server: DuplexStream, ack: BrokerAck) {
    tokio::spawn(async move {
        let mut server = server;
        let _hello: BrokerHello = match read_frame(&mut server).await {
            Ok(h) => h,
            Err(_) => return,
        };
        let _ = write_frame(&mut server, &ack).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
}

#[tokio::test]
async fn key_conflict_nack_is_fatal_and_resolves_first_outcome() {
    let connector = Arc::new(ScriptedConnector {
        attempts: AtomicUsize::new(0),
        script: |_, server| answer(server, BrokerAck::key_conflict("owned by inst-other")),
    });
    let (first_tx, first_rx) = tokio::sync::oneshot::channel();
    let task =
        tokio::spawn(run_register(connector.clone(), Arc::new(StaticToken), cfg(), Some(first_tx)));

    // The loop must RETURN promptly — no reconnect storm on a conflict.
    let fatal = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("run_register must return on a key conflict")
        .expect("join");
    assert!(fatal.reason.contains("owned by inst-other"), "reason: {}", fatal.reason);

    // The first-outcome channel reports the conflict for serve pre-flights.
    let first = first_rx.await.expect("first outcome delivered");
    assert_eq!(first, Err("owned by inst-other".to_string()));

    // Exactly one attempt: a fatal rejection is never retried.
    assert_eq!(connector.attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_rejection_stays_transient_and_retries() {
    // Attempt 0: a non-conflict rejection (e.g. relay hiccup). Attempt 1+: ok.
    let connector = Arc::new(ScriptedConnector {
        attempts: AtomicUsize::new(0),
        script: |n, server| {
            if n == 0 {
                answer(server, BrokerAck::err("relay hiccup"));
            } else {
                answer(server, BrokerAck::ok("pcxu:u:dev:app:default"));
            }
        },
    });
    let (first_tx, first_rx) = tokio::sync::oneshot::channel();
    let task =
        tokio::spawn(run_register(connector.clone(), Arc::new(StaticToken), cfg(), Some(first_tx)));

    // The transient rejection is retried and the SECOND attempt comes up, so
    // the first decisive outcome is a success.
    let first = tokio::time::timeout(Duration::from_secs(10), first_rx)
        .await
        .expect("first outcome in time")
        .expect("first outcome delivered");
    assert_eq!(first, Ok(()));
    assert!(connector.attempts.load(Ordering::SeqCst) >= 2, "the rejection must be retried");
    task.abort();
}

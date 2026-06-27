//! Register (publisher) side: the controller tunnel + per-stream data tunnels.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use pocket_codex_account_proto::{
    bridge::bridge,
    broker::{BrokerControl, BrokerHello, BrokerRole, TunnelPurpose},
    frame::{read_frame, write_frame},
    params::{self, RetryBackoff},
};
use pocket_codex_core::service::ServiceKind;
use tokio::{
    net::TcpStream,
    sync::Mutex,
    time::{interval, sleep, timeout, MissedTickBehavior},
};

use crate::{conn, BrokerError, BrokerStream, Connector, TokenProvider};

/// What a register session exposes and where the local service lives.
#[derive(Clone)]
pub struct RegisterConfig {
    /// Device id segment.
    pub device: String,
    /// Service kind.
    pub kind: ServiceKind,
    /// Instance name segment.
    pub name: String,
    /// Stable per-process id, so a reconnect deterministically takes over the
    /// prior session rather than racing it.
    pub client_instance_id: String,
    /// Local address of the service to expose (the codex app-server).
    pub local_addr: SocketAddr,
    /// Idle timeout applied to each data bridge.
    pub idle: Duration,
}

/// Run a register session forever, reconnecting with pb-mapper-matched backoff
/// (plus jitter). Intended to be `tokio::spawn`ed.
pub async fn run_register(
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    cfg: RegisterConfig,
) {
    let cfg = Arc::new(cfg);
    let mut backoff = RetryBackoff::new();
    loop {
        match session(&connector, &tokens, &cfg, &mut backoff).await {
            Ok(()) => tracing::info!("register session retired; reconnecting"),
            Err(e) => tracing::warn!(error = %e, "register session ended"),
        }
        sleep(backoff.next_delay_jittered(rand::random::<f64>())).await;
    }
}

async fn session(
    connector: &Arc<dyn Connector>,
    tokens: &Arc<dyn TokenProvider>,
    cfg: &Arc<RegisterConfig>,
    backoff: &mut RetryBackoff,
) -> Result<(), BrokerError> {
    let token = tokens.token().await?;
    let mut ctrl = connector.connect().await?;
    let hello = BrokerHello {
        token,
        role: BrokerRole::Register,
        purpose: TunnelPurpose::RegisterControl,
        device: cfg.device.clone(),
        kind: cfg.kind,
        name: cfg.name.clone(),
        client_instance_id: Some(cfg.client_instance_id.clone()),
        generation: None,
        stream_id: None,
    };
    let ack = conn::handshake(&mut ctrl, &hello).await?;
    backoff.reset();
    tracing::info!(relay_key = ?ack.relay_key, "register control tunnel up");
    control_loop(ctrl, connector.clone(), tokens.clone(), cfg.clone()).await
}

async fn control_loop(
    ctrl: Box<dyn BrokerStream>,
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    cfg: Arc<RegisterConfig>,
) -> Result<(), BrokerError> {
    let (mut reader, writer) = tokio::io::split(ctrl);
    let writer = Arc::new(Mutex::new(writer));
    let idle = params::HEARTBEAT_TOLERANCE + params::SUSPECT_GRACE;

    // Heartbeat lives in its own task: a Ping is a single write, so it can't
    // corrupt the (cancellation-unsafe) frame reads in the loop below. The read
    // loop is the liveness authority — it tears the session down on idle, which
    // aborts this task.
    let hb_writer = writer.clone();
    let heartbeat = tokio::spawn(async move {
        let mut tick = interval(params::HEARTBEAT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // skip the immediate first tick
        let mut seq: u64 = 0;
        loop {
            tick.tick().await;
            seq = seq.wrapping_add(1);
            let mut w = hb_writer.lock().await;
            if !matches!(
                timeout(
                    params::CONTROL_IO_TIMEOUT,
                    write_frame(&mut *w, &BrokerControl::Ping {
                        seq
                    }),
                )
                .await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });

    // Read frames to completion (never inside a `select!`, so a partial frame is
    // never dropped); the per-read idle timeout = "no heartbeat for tolerance".
    let result = loop {
        let frame = match timeout(idle, read_frame::<_, BrokerControl>(&mut reader)).await {
            Err(_) => break Err(BrokerError::Rejected("control tunnel idle".to_string())),
            Ok(Err(e)) => break Err(e.into()),
            Ok(Ok(frame)) => frame,
        };
        match frame {
            BrokerControl::Pong {
                ..
            } => {},
            BrokerControl::Retire {
                reason,
            } => {
                tracing::info!("register retired by backend: {reason}");
                break Ok(());
            },
            BrokerControl::NewStream {
                generation,
                stream_id,
            } => {
                // Ack on the control tunnel: renews the lease and signals intent
                // to dial the data tunnel.
                let ack = {
                    let mut w = writer.lock().await;
                    write_frame(&mut *w, &BrokerControl::StreamAck {
                        generation,
                        stream_id,
                    })
                    .await
                };
                if let Err(e) = ack {
                    // A failed control write means the session is dead — tear it
                    // down (it reconnects via backoff) rather than dialing a data
                    // tunnel the backend will never pair.
                    break Err(e.into());
                }
                let connector = connector.clone();
                let tokens = tokens.clone();
                let cfg = cfg.clone();
                // Detached: a data bridge must outlive a control reconnect.
                tokio::spawn(async move {
                    if let Err(e) = open_data(connector, tokens, cfg, generation, stream_id).await {
                        tracing::warn!(stream_id, error = %e, "register data tunnel failed");
                    }
                });
            },
            // A register client never receives these.
            BrokerControl::Ping {
                ..
            }
            | BrokerControl::StreamAck {
                ..
            } => {},
        }
    };
    heartbeat.abort();
    result
}

async fn open_data(
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    cfg: Arc<RegisterConfig>,
    generation: u64,
    stream_id: u64,
) -> Result<(), BrokerError> {
    let token = tokens.token().await?;
    let hello = BrokerHello {
        token,
        role: BrokerRole::Register,
        purpose: TunnelPurpose::RegisterData,
        device: cfg.device.clone(),
        kind: cfg.kind,
        name: cfg.name.clone(),
        client_instance_id: None,
        generation: Some(generation),
        stream_id: Some(stream_id),
    };
    let tunnel = conn::open_tunnel(connector.as_ref(), &hello).await?;
    let local = TcpStream::connect(cfg.local_addr).await?;
    bridge(tunnel, local, cfg.idle).await?;
    Ok(())
}

//! Register (publisher) handling: the control tunnel, the loopback seam that
//! pb-mapper dials per subscriber, and the per-stream rendezvous.

use std::{collections::HashMap, sync::Arc};

use pocket_codex_account_proto::{
    bridge::bridge,
    broker::{BrokerAck, BrokerControl, BrokerHello},
    frame::{read_frame, write_frame},
    params,
};
use pocket_codex_pb::{register, RegisterOptions};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use crate::{BrokerServer, BrokerServerError, RegisterSession, Result, ServerStream};

impl BrokerServer {
    /// Own a register control tunnel for its lifetime: install the session
    /// (taking over any prior one for the key), run pb-mapper register against
    /// the loopback relay, and serve `NewStream`/heartbeat on the control tunnel.
    pub(crate) async fn handle_register_control(
        &self,
        mut ctrl: Box<dyn ServerStream>,
        hello: BrokerHello,
        relay_key: String,
    ) -> Result<()> {
        let client_instance_id = hello.client_instance_id.unwrap_or_default();

        // The loopback seam pb-mapper dials once per subscriber stream. We bind
        // it ourselves (no readiness race) and own its accept loop.
        let seam = TcpListener::bind("127.0.0.1:0").await?;
        let seam_addr = seam.local_addr()?;

        let cancel = CancellationToken::new();
        // Install, retiring any prior session for this key (takeover).
        let session = {
            let mut map = self.inner.registers.lock().await;
            let generation = match map.get(&relay_key) {
                Some(old) => {
                    old.cancel.cancel();
                    old.generation.wrapping_add(1)
                }
                None => 0,
            };
            let session = Arc::new(RegisterSession {
                generation,
                client_instance_id,
                cancel: cancel.clone(),
                pending: Mutex::new(HashMap::new()),
            });
            map.insert(relay_key.clone(), session.clone());
            session
        };

        write_frame(&mut ctrl, &BrokerAck::ok(relay_key.clone())).await?;
        tracing::info!(relay_key = %relay_key, generation = session.generation, "register control up");

        // pb-mapper register against the loopback relay with the REAL key; it
        // dials `seam_addr` for each subscriber the relay routes to us.
        let pb = tokio::spawn({
            let relay_addr = self.inner.relay_addr.clone();
            let key = relay_key.clone();
            let local_addr = seam_addr.to_string();
            let cancel = cancel.clone();
            async move {
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = register(RegisterOptions { key, local_addr, relay_addr, codec: true }) => {}
                }
            }
        });

        let result = self.run_register_session(ctrl, seam, &session).await;

        // Teardown. Only evict the map slot if it is still ours (a takeover may
        // have already replaced it — then leave the newcomer in place).
        cancel.cancel();
        pb.abort();
        {
            let mut map = self.inner.registers.lock().await;
            if map
                .get(&relay_key)
                .is_some_and(|cur| Arc::ptr_eq(cur, &session))
            {
                map.remove(&relay_key);
            }
        }
        result
    }

    async fn run_register_session(
        &self,
        ctrl: Box<dyn ServerStream>,
        seam: TcpListener,
        session: &Arc<RegisterSession>,
    ) -> Result<()> {
        let (mut reader, writer) = tokio::io::split(ctrl);
        let writer = Arc::new(Mutex::new(writer));
        let data_idle = self.inner.data_idle;
        let cancel = session.cancel.clone();

        // Seam accept loop: each accept is one subscriber stream → emit a
        // NewStream and wait (briefly) for the answering data tunnel, then bridge.
        let accept = tokio::spawn({
            let writer = writer.clone();
            let session = session.clone();
            let cancel = cancel.clone();
            async move {
                let mut next_id: u64 = 0;
                loop {
                    let conn = tokio::select! {
                        _ = cancel.cancelled() => break,
                        r = seam.accept() => match r {
                            Ok((conn, _)) => conn,
                            Err(e) => {
                                tracing::warn!(error = %e, "seam accept failed");
                                break;
                            }
                        },
                    };
                    next_id = next_id.wrapping_add(1);
                    let stream_id = next_id;
                    let (tx, rx) = oneshot::channel::<Box<dyn ServerStream>>();
                    session.pending.lock().await.insert(stream_id, tx);
                    {
                        let mut w = writer.lock().await;
                        if write_frame(
                            &mut *w,
                            &BrokerControl::NewStream {
                                generation: session.generation,
                                stream_id,
                            },
                        )
                        .await
                        .is_err()
                        {
                            session.pending.lock().await.remove(&stream_id);
                            break;
                        }
                    }
                    let session = session.clone();
                    tokio::spawn(async move {
                        match timeout(params::STREAM_DIAL_TIMEOUT, rx).await {
                            Ok(Ok(tunnel)) => {
                                if let Err(e) = bridge(conn, tunnel, data_idle).await {
                                    tracing::debug!(stream_id, error = %e, "register data bridge ended");
                                }
                            }
                            _ => {
                                // Client never dialed in time; reclaim the slot.
                                session.pending.lock().await.remove(&stream_id);
                            }
                        }
                    });
                }
            }
        });

        // Control read loop: Ping→Pong, note StreamAck. No frame for the lease
        // window ⇒ the client is gone; tear the session down so it reconnects.
        // LEASE_TIMEOUT is intentionally longer than the client's own read idle
        // so the client reconnects before the backend retires the registration.
        let idle = params::LEASE_TIMEOUT;
        let result = loop {
            let frame = tokio::select! {
                _ = cancel.cancelled() => break Ok(()),
                r = timeout(idle, read_frame::<_, BrokerControl>(&mut reader)) => match r {
                    Err(_) => break Err(BrokerServerError::Timeout("control idle")),
                    Ok(Err(e)) => break Err(e.into()),
                    Ok(Ok(frame)) => frame,
                },
            };
            match frame {
                BrokerControl::Ping { seq } => {
                    let mut w = writer.lock().await;
                    let _ = write_frame(&mut *w, &BrokerControl::Pong { seq }).await;
                }
                BrokerControl::StreamAck { .. } => {}
                // A register control tunnel never receives these from a client.
                BrokerControl::NewStream { .. }
                | BrokerControl::Pong { .. }
                | BrokerControl::Retire { .. } => {}
            }
        };
        accept.abort();
        result
    }
}

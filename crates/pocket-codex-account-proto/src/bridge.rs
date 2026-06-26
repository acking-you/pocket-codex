//! Bidirectional plaintext byte bridge (feature `frame`).
//!
//! Used by the broker client and server to pump app bytes between a TLS tunnel
//! and a local/loopback socket. Two properties the naive `copy_bidirectional`
//! lacks (broker review E1/E2):
//!
//! - **Half-close propagation:** each direction copies independently and calls
//!   `shutdown()` on the peer's write half at EOF, so a request/response stream
//!   that half-closes its write side (then waits on its read side) does not
//!   hang.
//! - **Connection-level idle timeout:** if *neither* direction moves a byte for
//!   `idle`, the whole bridge is torn down — bounding leaked sockets on dead
//!   mobile links. It is connection-level (not per-direction) so a legitimately
//!   one-way-quiet stream (e.g. a long streaming response) is not killed while
//!   the other half is active.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const COPY_BUF: usize = 16 * 1024;

/// Copy `reader` → `writer` until EOF, stamping `activity` (ms since `start`)
/// on every chunk; shut the writer's write half down at EOF (half-close).
async fn pump<R, W>(
    mut reader: R,
    mut writer: W,
    start: Instant,
    activity: Arc<AtomicU64>,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        activity.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
    }
    let _ = writer.shutdown().await;
    Ok(())
}

/// Bridge two streams until both directions reach EOF, or until neither has
/// moved a byte for `idle` (then both halves are dropped/closed).
pub async fn bridge<A, B>(a: A, b: B, idle: Duration) -> std::io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    let start = Instant::now();
    let activity = Arc::new(AtomicU64::new(0));

    let copy = {
        let a2b = pump(ar, bw, start, activity.clone());
        let b2a = pump(br, aw, start, activity.clone());
        async move { tokio::try_join!(a2b, b2a).map(|_| ()) }
    };

    let watchdog = {
        let activity = activity.clone();
        async move {
            let step = (idle / 4).max(Duration::from_millis(50));
            let mut tick = tokio::time::interval(step);
            tick.tick().await; // skip the immediate first tick
            loop {
                tick.tick().await;
                let now = start.elapsed().as_millis() as u64;
                if now.saturating_sub(activity.load(Ordering::Relaxed)) >= idle.as_millis() as u64 {
                    return;
                }
            }
        }
    };

    tokio::select! {
        r = copy => r,
        _ = watchdog => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bridges_both_directions_and_half_close() {
        // a_outer <-> a_inner | bridge | b_inner <-> b_outer
        let (a_outer, a_inner) = tokio::io::duplex(1024);
        let (b_inner, b_outer) = tokio::io::duplex(1024);
        let h = tokio::spawn(bridge(a_inner, b_inner, Duration::from_secs(5)));

        let (mut ar, mut aw) = tokio::io::split(a_outer);
        let (mut br, mut bw) = tokio::io::split(b_outer);

        aw.write_all(b"ping").await.expect("write a");
        aw.shutdown().await.expect("shutdown a"); // half-close A's write
        bw.write_all(b"pong").await.expect("write b");
        bw.shutdown().await.expect("shutdown b");

        let mut got_b = Vec::new();
        br.read_to_end(&mut got_b).await.expect("read b");
        let mut got_a = Vec::new();
        ar.read_to_end(&mut got_a).await.expect("read a");

        assert_eq!(got_b, b"ping");
        assert_eq!(got_a, b"pong");
        h.await.expect("join").expect("bridge ok");
    }

    #[tokio::test]
    async fn idle_timeout_tears_down_silent_bridge() {
        let (a_outer, a_inner) = tokio::io::duplex(1024);
        let (b_inner, _b_outer) = tokio::io::duplex(1024);
        // Hold a_outer open but send nothing → idle fires and bridge returns.
        let _keep = a_outer;
        let r = tokio::time::timeout(
            Duration::from_secs(2),
            bridge(a_inner, b_inner, Duration::from_millis(150)),
        )
        .await;
        let inner = r.expect("bridge should return on idle, not hang");
        assert!(inner.is_ok());
    }
}

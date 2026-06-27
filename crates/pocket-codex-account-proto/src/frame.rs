//! Length-prefixed JSON framing for the broker handshake (feature `frame`).
//!
//! The broker tunnel exchanges exactly one [`crate::BrokerHello`] and one
//! [`crate::BrokerAck`] as `u32` (big-endian) length-prefixed JSON, then goes
//! raw. Both the client and the server use these helpers so the framing has a
//! single source of truth.

use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Upper bound on a handshake frame. The handshake is tiny; this guards against
/// a hostile length prefix forcing a large allocation.
const MAX_FRAME_LEN: u32 = 64 * 1024;

/// Errors from reading or writing a handshake frame.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// Underlying stream I/O failed.
    #[error("frame i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The frame body was not valid JSON for the target type.
    #[error("frame json: {0}")]
    Json(#[from] serde_json::Error),
    /// The declared frame length exceeded [`MAX_FRAME_LEN`].
    #[error("frame too large: {0} bytes")]
    TooLarge(u32),
}

/// Write `value` as a `u32`-length-prefixed (big-endian) JSON frame and flush.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(value)?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

/// Read one `u32`-length-prefixed JSON frame and deserialize it into `T`.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

//! Tokio-based framing helpers for Neo P2P streams with timeouts, size guards, and vectored I/O.
use crate::channels_config::ChannelsConfig;
use bytes::BytesMut;
use neo_primitives::{NetworkError, NetworkResult};
use std::io::IoSlice;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use tokio_util::codec::Decoder;

pub use crate::framed_codec::NeoMessageCodec;

/// Initial buffer capacity for reading small messages.
const INITIAL_READ_CAPACITY: usize = 256;

/// Buffer size for vectored writes.
const WRITE_BUFFER_SIZE: usize = 4096;

async fn write_all_with_timeout<W>(
    writer: &mut W,
    timeout_duration: Duration,
    data: &[u8],
) -> NetworkResult<()>
where
    W: AsyncWrite + Unpin,
{
    timeout(timeout_duration, writer.write_all(data))
        .await
        .map_err(|_| NetworkError::Timeout)?
        .map_err(|e| NetworkError::ConnectionError(format!("Failed to write frame: {e}")))
}

async fn write_all_vectored_with_timeout<W>(
    writer: &mut W,
    timeout_duration: Duration,
    buffers: &[&[u8]],
) -> NetworkResult<()>
where
    W: AsyncWrite + Unpin,
{
    timeout(timeout_duration, async {
        let mut buffer_index = 0;
        let mut offset = 0;

        loop {
            while buffer_index < buffers.len() && offset == buffers[buffer_index].len() {
                buffer_index += 1;
                offset = 0;
            }

            if buffer_index == buffers.len() {
                return Ok(());
            }

            let io_slices: Vec<IoSlice<'_>> = buffers[buffer_index..]
                .iter()
                .enumerate()
                .filter_map(|(index, buffer)| {
                    let slice = if index == 0 {
                        &buffer[offset..]
                    } else {
                        buffer
                    };
                    (!slice.is_empty()).then(|| IoSlice::new(slice))
                })
                .collect();

            let written = writer.write_vectored(&io_slices).await.map_err(|err| {
                NetworkError::ConnectionError(format!("Failed to write frame (vectored): {err}"))
            })?;

            if written == 0 {
                return Err(NetworkError::ConnectionError(
                    "Failed to write frame (vectored): wrote zero bytes".to_string(),
                ));
            }

            let mut remaining = written;
            while remaining > 0 && buffer_index < buffers.len() {
                let available = buffers[buffer_index].len().saturating_sub(offset);
                if available == 0 {
                    buffer_index += 1;
                    offset = 0;
                } else if remaining < available {
                    offset += remaining;
                    remaining = 0;
                } else {
                    remaining -= available;
                    buffer_index += 1;
                    offset = 0;
                }
            }
        }
    })
    .await
    .map_err(|_| NetworkError::Timeout)?
}

/// Runtime framing configuration to keep read behaviour consistent across the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameConfig {
    /// Read timeout applied while the connection handshake is in progress.
    pub read_timeout_handshake: Duration,
    /// Read timeout applied once the peer connection is active.
    pub read_timeout_active: Duration,
    /// Timeout applied to frame writes.
    pub write_timeout: Duration,
    /// Timeout applied when draining and shutting down a connection.
    pub shutdown_timeout: Duration,
    /// Upper bound a single underlying read may hold the connection lock
    /// during a stepwise receive. Short steps create lock windows so writers
    /// are not starved by a silent peer for the whole active read timeout.
    pub read_step_timeout: Duration,
    /// Enable vectored I/O for writes (reduces syscalls for multi-buffer writes).
    pub use_vectored_io: bool,
    /// Buffer small writes to reduce syscall overhead.
    pub buffer_small_writes: bool,
}

impl From<&ChannelsConfig> for FrameConfig {
    fn from(cfg: &ChannelsConfig) -> Self {
        Self {
            read_timeout_handshake: cfg.handshake_timeout,
            read_timeout_active: cfg.read_timeout_active,
            write_timeout: cfg.write_timeout,
            shutdown_timeout: cfg.shutdown_timeout,
            read_step_timeout: Duration::from_millis(500),
            use_vectored_io: true,
            buffer_small_writes: true,
        }
    }
}

impl From<ChannelsConfig> for FrameConfig {
    fn from(cfg: ChannelsConfig) -> Self {
        FrameConfig::from(&cfg)
    }
}

impl Default for FrameConfig {
    fn default() -> Self {
        FrameConfig::from(&ChannelsConfig::default())
    }
}

/// Write buffer for batching small writes.
#[derive(Debug)]
pub struct WriteBuffer {
    buffer: Vec<u8>,
    /// The threshold at which to flush the buffer.
    pub threshold: usize,
}

impl WriteBuffer {
    /// Creates a new write buffer with the specified threshold.
    pub fn new(threshold: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(threshold),
            threshold,
        }
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Adds data to the buffer. Returns true if all data was buffered, false if buffer is full.
    /// If the buffer is full, no data is added.
    pub fn push(&mut self, data: &[u8]) -> bool {
        let remaining = self.threshold.saturating_sub(self.buffer.len());
        if remaining < data.len() {
            return false;
        }
        self.buffer.extend_from_slice(data);
        true
    }

    /// Returns the remaining capacity in the buffer.
    pub fn remaining_capacity(&self) -> usize {
        self.threshold.saturating_sub(self.buffer.len())
    }

    /// Checks if the buffer should be flushed.
    pub fn should_flush(&self) -> bool {
        self.buffer.len() >= self.threshold
    }

    /// Takes the buffered data, leaving the buffer empty.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }
}

impl Default for WriteBuffer {
    fn default() -> Self {
        Self::new(WRITE_BUFFER_SIZE)
    }
}

/// Persistent read-side frame decoder state.
#[derive(Debug)]
pub struct FrameReader {
    buffer: BytesMut,
    codec: NeoMessageCodec,
}

impl FrameReader {
    /// Creates a persistent reader using the protocol's default payload limit.
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(INITIAL_READ_CAPACITY),
            codec: NeoMessageCodec::default(),
        }
    }

    /// Reads a full P2P message frame (flags + command + var-bytes payload) with a timeout applied
    /// to each underlying read.
    pub async fn read_from_stream(
        &mut self,
        stream: &mut TcpStream,
        cfg: &FrameConfig,
        handshake_complete: bool,
    ) -> NetworkResult<Vec<u8>> {
        let timeout_duration = if handshake_complete {
            cfg.read_timeout_active
        } else {
            cfg.read_timeout_handshake
        };

        loop {
            if let Some(frame) = self.codec.decode(&mut self.buffer)? {
                return Ok(frame);
            }

            let bytes_read = timeout(timeout_duration, stream.read_buf(&mut self.buffer))
                .await
                .map_err(|_| NetworkError::Timeout)?
                .map_err(|err| {
                    NetworkError::ConnectionError(format!("Failed to read frame: {err}"))
                })?;

            if bytes_read == 0 {
                return Err(NetworkError::ConnectionError(
                    "Connection closed while reading frame".to_string(),
                ));
            }
        }
    }

    /// Performs one bounded read step: decode a buffered frame if one is
    /// complete, otherwise issue a single short-timeout read. Partial frames
    /// stay in the internal buffer so callers can release the connection lock
    /// between steps without losing progress.
    pub async fn read_step(
        &mut self,
        stream: &mut TcpStream,
        cfg: &FrameConfig,
        handshake_complete: bool,
    ) -> NetworkResult<FrameReadStep> {
        if let Some(frame) = self.codec.decode(&mut self.buffer)? {
            return Ok(FrameReadStep::Frame(frame));
        }

        let timeout_duration = if handshake_complete {
            cfg.read_step_timeout
        } else {
            // The handshake read deadline must stay tight; use the smaller of
            // the handshake timeout and a single step so a silent peer is
            // noticed promptly.
            cfg.read_step_timeout.min(cfg.read_timeout_handshake)
        };

        let read = timeout(timeout_duration, stream.read_buf(&mut self.buffer)).await;
        let bytes_read = match read {
            // Step deadline elapsed with no data: an idle step, not fatal.
            Err(_elapsed) => return Ok(FrameReadStep::Idle),
            Ok(Err(err)) => {
                return Err(NetworkError::ConnectionError(format!(
                    "Failed to read frame: {err}"
                )));
            }
            Ok(Ok(n)) => n,
        };

        if bytes_read == 0 {
            return Ok(FrameReadStep::Closed);
        }
        Ok(FrameReadStep::Pending)
    }
}

/// Result of a single stepwise frame read.
pub enum FrameReadStep {
    /// A complete frame was decoded.
    Frame(Vec<u8>),
    /// Bytes arrived without completing a frame.
    Pending,
    /// The step deadline elapsed without any data.
    Idle,
    /// The peer closed the connection.
    Closed,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes a frame using vectored I/O to reduce syscalls.
pub async fn write_frame_vectored<W>(
    writer: &mut W,
    cfg: &FrameConfig,
    buffers: &[&[u8]],
) -> NetworkResult<()>
where
    W: AsyncWrite + Unpin,
{
    if buffers.is_empty() {
        return Ok(());
    }

    if buffers.len() == 1 || !cfg.use_vectored_io {
        for buf in buffers {
            write_all_with_timeout(writer, cfg.write_timeout, buf).await?;
        }
        return Ok(());
    }

    write_all_vectored_with_timeout(writer, cfg.write_timeout, buffers).await
}

/// Writes a complete message frame with optional buffering.
pub async fn write_frame<W>(
    writer: &mut W,
    cfg: &FrameConfig,
    data: &[u8],
    write_buffer: &mut WriteBuffer,
) -> NetworkResult<()>
where
    W: AsyncWrite + Unpin,
{
    if !cfg.buffer_small_writes || data.len() >= write_buffer.threshold {
        flush_write_buffer(writer, cfg, write_buffer).await?;
        write_all_with_timeout(writer, cfg.write_timeout, data).await?;
    } else if data.len() <= write_buffer.remaining_capacity() {
        write_buffer.push(data);

        if write_buffer.should_flush() {
            flush_write_buffer(writer, cfg, write_buffer).await?;
        }
    } else {
        flush_write_buffer(writer, cfg, write_buffer).await?;
        write_all_with_timeout(writer, cfg.write_timeout, data).await?;
    }

    Ok(())
}

/// Flushes any pending buffered writes.
pub async fn flush_write_buffer<W>(
    writer: &mut W,
    cfg: &FrameConfig,
    write_buffer: &mut WriteBuffer,
) -> NetworkResult<()>
where
    W: AsyncWrite + Unpin,
{
    if !write_buffer.is_empty() {
        let buffered = write_buffer.take();
        timeout(cfg.write_timeout, writer.write_all(&buffered))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to flush buffer: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "framed_tests.rs"]
mod tests;

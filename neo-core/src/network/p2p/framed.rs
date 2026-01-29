//! Tokio-based framing helpers for Neo P2P streams with timeouts, size guards, and vectored I/O.
use super::{channels_config::ChannelsConfig, message::PAYLOAD_MAX_SIZE};
use crate::network::{NetworkError, NetworkResult};
use std::io::IoSlice;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

/// Initial buffer capacity for reading small messages.
const INITIAL_READ_CAPACITY: usize = 256;

/// Buffer size for vectored writes.
const WRITE_BUFFER_SIZE: usize = 4096;

/// Minimal framed reader/writer that wraps Neo P2P length-prefixing with timeouts,
/// size guards, and vectored I/O support.
pub struct FramedSocket<'a> {
    stream: &'a mut TcpStream,
}

/// Runtime framing configuration to keep read behaviour consistent across the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameConfig {
    pub read_timeout_handshake: Duration,
    pub read_timeout_active: Duration,
    pub write_timeout: Duration,
    pub shutdown_timeout: Duration,
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

    /// Clears the buffer.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Returns a reference to the buffered data.
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer
    }
}

impl Default for WriteBuffer {
    fn default() -> Self {
        Self::new(WRITE_BUFFER_SIZE)
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::error::NetworkError;
    use tokio::net::{TcpListener, TcpStream};

    async fn silent_pair() -> Option<(TcpStream, TcpStream)> {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(err) => panic!("bind listener: {}", err),
        };
        let addr = listener.local_addr().expect("listener addr");

        let client = TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_stream) = tokio::join!(client, server);
        let client_stream = client_stream.expect("client connect succeeded");
        let (server_stream, _) = server_stream.expect("server accept succeeded");
        Some((client_stream, server_stream))
    }

    #[tokio::test]
    async fn read_frame_times_out_when_peer_silent() {
        let Some((mut client_stream, _server_stream)) = silent_pair().await else {
            return;
        };

        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_millis(10),
            read_timeout_active: Duration::from_millis(10),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            use_vectored_io: true,
            buffer_small_writes: true,
        };

        let result = framed.read_frame(&cfg, false).await;
        assert!(
            matches!(result, Err(NetworkError::Timeout)),
            "expected timeout error, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn read_frame_times_out_in_active_session_when_peer_silent() {
        let Some((mut client_stream, _server_stream)) = silent_pair().await else {
            return;
        };
        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_secs(5),
            read_timeout_active: Duration::from_millis(10),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            use_vectored_io: true,
            buffer_small_writes: true,
        };

        let result = framed.read_frame(&cfg, true).await;
        assert!(
            matches!(result, Err(NetworkError::Timeout)),
            "expected active timeout error, got: {:?}",
            result
        );
    }

    #[test]
    fn frame_config_default_matches_channels_defaults() {
        let expected = FrameConfig::from(&ChannelsConfig::default());
        let actual = FrameConfig::default();
        assert_eq!(expected, actual);
    }

    #[test]
    fn write_buffer_basic_operations() {
        let mut buf = WriteBuffer::new(10);
        assert!(buf.is_empty());

        // Add data that fits
        let pushed = buf.push(b"hello");
        assert!(pushed);
        assert_eq!(buf.len(), 5);
        assert!(!buf.should_flush());

        // Add more data
        let pushed = buf.push(b"world");
        assert!(pushed);
        assert_eq!(buf.len(), 10);
        assert!(buf.should_flush());

        // Add data that doesn't fit
        let pushed = buf.push(b"!");
        assert!(!pushed);
        assert_eq!(buf.len(), 10); // Buffer unchanged
        
        // Check remaining capacity
        assert_eq!(buf.remaining_capacity(), 0);
    }

    #[test]
    fn write_buffer_take_clears() {
        let mut buf = WriteBuffer::new(10);
        buf.push(b"test");
        assert_eq!(buf.len(), 4);

        let taken = buf.take();
        assert_eq!(taken, b"test");
        assert!(buf.is_empty());
    }

    #[test]
    fn write_buffer_empty_push() {
        let mut buf = WriteBuffer::new(10);
        let pushed = buf.push(b"");
        assert!(pushed);
        assert!(buf.is_empty());
    }
    
    #[test]
    fn write_buffer_remaining_capacity() {
        let mut buf = WriteBuffer::new(10);
        assert_eq!(buf.remaining_capacity(), 10);
        
        buf.push(b"hello");
        assert_eq!(buf.remaining_capacity(), 5);
        
        buf.push(b"world");
        assert_eq!(buf.remaining_capacity(), 0);
    }
}

impl<'a> FramedSocket<'a> {
    pub fn new(stream: &'a mut TcpStream) -> Self {
        Self { stream }
    }

    /// Reads a full P2P message frame (flags + command + var-bytes payload) with a timeout applied to
    /// each underlying read.
    ///
    /// Optimizations:
    /// - Uses a single buffer with pre-calculated capacity to minimize allocations
    /// - Avoids intermediate Vec appends by reading directly into the target buffer
    pub async fn read_frame(
        &mut self,
        cfg: &FrameConfig,
        handshake_complete: bool,
    ) -> NetworkResult<Vec<u8>> {
        let timeout_duration = if handshake_complete {
            cfg.read_timeout_active
        } else {
            cfg.read_timeout_handshake
        };

        // Read header (flags + command)
        let mut header = [0u8; 2];
        self.read_exact_slice(&mut header, timeout_duration, "header")
            .await?;

        // Read payload length (var_int)
        let (payload_length, varint_len) = self.read_var_int_len(timeout_duration).await?;

        // Calculate total message size and pre-allocate buffer
        let total_len = 2 + varint_len + payload_length as usize;
        let mut message_bytes = Vec::with_capacity(total_len.max(INITIAL_READ_CAPACITY));

        // Write header
        message_bytes.extend_from_slice(&header);

        // Write var_int bytes (we need to reconstruct them)
        Self::write_var_int_to_vec(payload_length, &mut message_bytes);

        // Read payload directly into buffer
        if payload_length > 0 {
            let payload_start = message_bytes.len();
            message_bytes.resize(payload_start + payload_length as usize, 0);
            self.read_exact_slice(
                &mut message_bytes[payload_start..],
                timeout_duration,
                "payload",
            )
            .await?;
        }

        Ok(message_bytes)
    }

    /// Writes a frame using vectored I/O to reduce syscalls.
    ///
    /// This is more efficient when writing multiple buffers as it avoids
    /// copying them into a single buffer first.
    pub async fn write_frame_vectored(
        &mut self,
        cfg: &FrameConfig,
        buffers: &[&[u8]],
    ) -> NetworkResult<()> {
        if buffers.is_empty() {
            return Ok(());
        }

        // Use write_all for single buffer (simpler, same efficiency)
        if buffers.len() == 1 || !cfg.use_vectored_io {
            for buf in buffers {
                timeout(cfg.write_timeout, self.stream.write_all(buf))
                    .await
                    .map_err(|_| NetworkError::Timeout)?
                    .map_err(|e| {
                        NetworkError::ConnectionError(format!("Failed to write frame: {e}"))
                    })?;
            }
            return Ok(());
        }

        // Use vectored I/O for multiple buffers
        let io_slices: Vec<IoSlice<'_>> =
            buffers.iter().map(|buf| IoSlice::new(buf)).collect();

        timeout(cfg.write_timeout, self.stream.write_vectored(&io_slices))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| {
                NetworkError::ConnectionError(format!("Failed to write frame (vectored): {e}"))
            })?;

        Ok(())
    }

    /// Writes a complete message frame with optional buffering.
    pub async fn write_frame(
        &mut self,
        cfg: &FrameConfig,
        data: &[u8],
        write_buffer: &mut WriteBuffer,
    ) -> NetworkResult<()> {
        if !cfg.buffer_small_writes || data.len() >= write_buffer.threshold {
            // Flush any pending buffered data first
            if !write_buffer.is_empty() {
                let buffered = write_buffer.take();
                timeout(cfg.write_timeout, self.stream.write_all(&buffered))
                    .await
                    .map_err(|_| NetworkError::Timeout)?
                    .map_err(|e| {
                        NetworkError::ConnectionError(format!("Failed to flush buffer: {e}"))
                    })?;
            }

            // Write large data directly
            timeout(cfg.write_timeout, self.stream.write_all(data))
                .await
                .map_err(|_| NetworkError::Timeout)?
                .map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to write frame: {e}"))
                })?;
        } else if data.len() <= write_buffer.remaining_capacity() {
            // Data fits in buffer
            write_buffer.push(data);
            
            // Flush if we've reached the threshold
            if write_buffer.should_flush() {
                let buffered = write_buffer.take();
                timeout(cfg.write_timeout, self.stream.write_all(&buffered))
                    .await
                    .map_err(|_| NetworkError::Timeout)?
                    .map_err(|e| {
                        NetworkError::ConnectionError(format!("Failed to flush buffer: {e}"))
                    })?;
            }
        } else {
            // Data doesn't fit, flush buffer first then write data directly
            if !write_buffer.is_empty() {
                let buffered = write_buffer.take();
                timeout(cfg.write_timeout, self.stream.write_all(&buffered))
                    .await
                    .map_err(|_| NetworkError::Timeout)?
                    .map_err(|e| {
                        NetworkError::ConnectionError(format!("Failed to flush buffer: {e}"))
                    })?;
            }
            
            // Write data directly (it's larger than buffer threshold anyway)
            timeout(cfg.write_timeout, self.stream.write_all(data))
                .await
                .map_err(|_| NetworkError::Timeout)?
                .map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to write frame: {e}"))
                })?;
        }

        Ok(())
    }

    /// Flushes any pending buffered writes.
    pub async fn flush(&mut self, cfg: &FrameConfig, write_buffer: &mut WriteBuffer) -> NetworkResult<()> {
        if !write_buffer.is_empty() {
            let buffered = write_buffer.take();
            timeout(cfg.write_timeout, self.stream.write_all(&buffered))
                .await
                .map_err(|_| NetworkError::Timeout)?
                .map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to flush buffer: {e}"))
                })?;
        }
        Ok(())
    }

    async fn read_exact_slice(
        &mut self,
        buf: &mut [u8],
        timeout_duration: Duration,
        context: &str,
    ) -> NetworkResult<()> {
        timeout(timeout_duration, self.stream.read_exact(buf))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to read {context}: {e}")))?;
        Ok(())
    }

    /// Reads a var_int and returns (value, byte_length) without allocating.
    async fn read_var_int_len(&mut self, timeout_duration: Duration) -> NetworkResult<(u64, usize)> {
        let mut first = [0u8; 1];
        self.read_exact_slice(&mut first, timeout_duration, "varint prefix")
            .await?;

        match first[0] {
            0xFD => {
                let mut buffer = [0u8; 2];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u16)")
                    .await?;
                let value = u16::from_le_bytes(buffer) as u64;
                if value > PAYLOAD_MAX_SIZE as u64 {
                    return Err(NetworkError::InvalidMessage(format!(
                        "Payload length {} exceeds maximum {}",
                        value, PAYLOAD_MAX_SIZE
                    )));
                }
                Ok((value, 3))
            }
            0xFE => {
                let mut buffer = [0u8; 4];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u32)")
                    .await?;
                let value = u32::from_le_bytes(buffer) as u64;
                if value > PAYLOAD_MAX_SIZE as u64 {
                    return Err(NetworkError::InvalidMessage(format!(
                        "Payload length {} exceeds maximum {}",
                        value, PAYLOAD_MAX_SIZE
                    )));
                }
                Ok((value, 5))
            }
            0xFF => {
                let mut buffer = [0u8; 8];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u64)")
                    .await?;
                let value = u64::from_le_bytes(buffer);
                if value > PAYLOAD_MAX_SIZE as u64 {
                    return Err(NetworkError::InvalidMessage(format!(
                        "Payload length {} exceeds maximum {}",
                        value, PAYLOAD_MAX_SIZE
                    )));
                }
                Ok((value, 9))
            }
            value => {
                let val = value as u64;
                if val > PAYLOAD_MAX_SIZE as u64 {
                    return Err(NetworkError::InvalidMessage(format!(
                        "Payload length {} exceeds maximum {}",
                        val, PAYLOAD_MAX_SIZE
                    )));
                }
                Ok((val, 1))
            }
        }
    }

    /// Writes a var_int to a Vec without allocating a separate buffer.
    fn write_var_int_to_vec(value: u64, vec: &mut Vec<u8>) {
        if value < 0xFD {
            vec.push(value as u8);
        } else if value <= 0xFFFF {
            vec.push(0xFD);
            vec.extend_from_slice(&(value as u16).to_le_bytes());
        } else if value <= 0xFFFF_FFFF {
            vec.push(0xFE);
            vec.extend_from_slice(&(value as u32).to_le_bytes());
        } else {
            vec.push(0xFF);
            vec.extend_from_slice(&value.to_le_bytes());
        }
    }
}

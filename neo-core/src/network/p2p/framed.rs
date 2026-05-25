//! Tokio-based framing helpers for Neo P2P streams with timeouts, size guards, and vectored I/O.
use super::{channels_config::ChannelsConfig, message::PAYLOAD_MAX_SIZE};
use crate::network::{NetworkError, NetworkResult};
use bytes::{BufMut, BytesMut};
use std::io::IoSlice;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio_util::codec::{Decoder, Encoder};

/// Initial buffer capacity for reading small messages.
const INITIAL_READ_CAPACITY: usize = 256;
const FRAME_HEADER_LEN: usize = 2;

/// Buffer size for vectored writes.
const WRITE_BUFFER_SIZE: usize = 4096;

pub(crate) fn new_read_buffer() -> BytesMut {
    BytesMut::with_capacity(INITIAL_READ_CAPACITY)
}

/// Minimal framed reader/writer that wraps Neo P2P length-prefixing with timeouts,
/// size guards, and vectored I/O support.
pub struct FramedSocket<'a> {
    stream: &'a mut TcpStream,
    read_buffer: BytesMut,
}

/// Neo P2P message codec for flags + command + var-bytes payload frames.
#[derive(Clone, Debug)]
pub struct NeoMessageCodec {
    max_payload_size: usize,
}

impl NeoMessageCodec {
    pub fn new(max_payload_size: usize) -> Self {
        Self { max_payload_size }
    }

    fn read_var_int(src: &[u8]) -> NetworkResult<Option<(u64, usize)>> {
        let Some(prefix) = src.first().copied() else {
            return Ok(None);
        };

        match prefix {
            0xFD => {
                if src.len() < 3 {
                    return Ok(None);
                }
                Ok(Some((u16::from_le_bytes([src[1], src[2]]) as u64, 3)))
            }
            0xFE => {
                if src.len() < 5 {
                    return Ok(None);
                }
                Ok(Some((
                    u32::from_le_bytes([src[1], src[2], src[3], src[4]]) as u64,
                    5,
                )))
            }
            0xFF => {
                if src.len() < 9 {
                    return Ok(None);
                }
                Ok(Some((
                    u64::from_le_bytes([
                        src[1], src[2], src[3], src[4], src[5], src[6], src[7], src[8],
                    ]),
                    9,
                )))
            }
            value => Ok(Some((value as u64, 1))),
        }
    }

    fn write_var_int(value: u64, dst: &mut Vec<u8>) {
        if value < 0xFD {
            dst.push(value as u8);
        } else if value <= 0xFFFF {
            dst.push(0xFD);
            dst.extend_from_slice(&(value as u16).to_le_bytes());
        } else if value <= 0xFFFF_FFFF {
            dst.push(0xFE);
            dst.extend_from_slice(&(value as u32).to_le_bytes());
        } else {
            dst.push(0xFF);
            dst.extend_from_slice(&value.to_le_bytes());
        }
    }
}

impl Default for NeoMessageCodec {
    fn default() -> Self {
        Self::new(PAYLOAD_MAX_SIZE)
    }
}

impl Decoder for NeoMessageCodec {
    type Item = Vec<u8>;
    type Error = NetworkError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < FRAME_HEADER_LEN + 1 {
            return Ok(None);
        }

        let Some((payload_length, varint_len)) = Self::read_var_int(&src[FRAME_HEADER_LEN..])?
        else {
            return Ok(None);
        };

        if payload_length > self.max_payload_size as u64 {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload length {} exceeds maximum {}",
                payload_length, self.max_payload_size
            )));
        }

        let payload_length = usize::try_from(payload_length).map_err(|_| {
            NetworkError::InvalidMessage(format!(
                "Payload length {} exceeds maximum {}",
                payload_length, self.max_payload_size
            ))
        })?;
        let frame_len = FRAME_HEADER_LEN
            .checked_add(varint_len)
            .and_then(|len| len.checked_add(payload_length))
            .ok_or_else(|| NetworkError::InvalidMessage("Frame length overflow".to_string()))?;

        if src.len() < frame_len {
            return Ok(None);
        }

        let frame = src.split_to(frame_len);
        let mut message = Vec::with_capacity(frame_len.max(INITIAL_READ_CAPACITY));
        message.extend_from_slice(&frame[..FRAME_HEADER_LEN]);
        Self::write_var_int(payload_length as u64, &mut message);
        message.extend_from_slice(&frame[FRAME_HEADER_LEN + varint_len..]);
        Ok(Some(message))
    }
}

impl<'a> Encoder<&'a [u8]> for NeoMessageCodec {
    type Error = NetworkError;

    fn encode(&mut self, item: &'a [u8], dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(item.len());
        dst.put_slice(item);
        Ok(())
    }
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
    use tokio::io::AsyncWriteExt;
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
    fn neo_message_codec_decodes_complete_frame() {
        let mut codec = NeoMessageCodec::default();
        let mut input = BytesMut::from(&[0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC][..]);

        let frame = codec
            .decode(&mut input)
            .expect("decode succeeds")
            .expect("frame is complete");

        assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
        assert!(input.is_empty());
    }

    #[test]
    fn neo_message_codec_waits_for_partial_frame() {
        let mut codec = NeoMessageCodec::default();
        let mut input = BytesMut::from(&[0x00, 0x01, 0xFD, 0x03][..]);

        let frame = codec.decode(&mut input).expect("partial decode succeeds");

        assert!(frame.is_none());
        assert_eq!(&input[..], &[0x00, 0x01, 0xFD, 0x03]);
    }

    #[test]
    fn neo_message_codec_rejects_oversized_payload() {
        let mut codec = NeoMessageCodec::new(1);
        let mut input = BytesMut::from(&[0x00, 0x01, 0x02][..]);

        let result = codec.decode(&mut input);

        assert!(matches!(result, Err(NetworkError::InvalidMessage(_))));
    }

    #[test]
    fn neo_message_codec_canonicalizes_var_int_prefix() {
        let mut codec = NeoMessageCodec::default();
        let mut input = BytesMut::from(&[0x00, 0x01, 0xFD, 0x01, 0x00, 0xAA][..]);

        let frame = codec
            .decode(&mut input)
            .expect("decode succeeds")
            .expect("frame is complete");

        assert_eq!(frame, vec![0x00, 0x01, 0x01, 0xAA]);
    }

    #[tokio::test]
    async fn read_frame_decodes_message_from_stream() {
        let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
            return;
        };
        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_secs(1),
            read_timeout_active: Duration::from_secs(1),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            use_vectored_io: true,
            buffer_small_writes: true,
        };

        server_stream
            .write_all(&[0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC])
            .await
            .expect("server writes frame");

        let frame = framed.read_frame(&cfg, false).await.expect("frame reads");

        assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
    }

    #[tokio::test]
    async fn read_frame_preserves_buffered_next_frame() {
        let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
            return;
        };
        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_secs(1),
            read_timeout_active: Duration::from_secs(1),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            use_vectored_io: true,
            buffer_small_writes: true,
        };

        server_stream
            .write_all(&[0x00, 0x01, 0x01, 0xAA, 0x00, 0x02, 0x01, 0xBB])
            .await
            .expect("server writes frames");

        let first = framed.read_frame(&cfg, false).await.expect("first frame");
        let second = framed.read_frame(&cfg, false).await.expect("second frame");

        assert_eq!(first, vec![0x00, 0x01, 0x01, 0xAA]);
        assert_eq!(second, vec![0x00, 0x02, 0x01, 0xBB]);
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
        Self {
            stream,
            read_buffer: new_read_buffer(),
        }
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
        Self::read_frame_from_stream(self.stream, &mut self.read_buffer, cfg, handshake_complete)
            .await
    }

    pub(crate) async fn read_frame_from_stream(
        stream: &mut TcpStream,
        read_buffer: &mut BytesMut,
        cfg: &FrameConfig,
        handshake_complete: bool,
    ) -> NetworkResult<Vec<u8>> {
        let timeout_duration = if handshake_complete {
            cfg.read_timeout_active
        } else {
            cfg.read_timeout_handshake
        };

        let mut codec = NeoMessageCodec::default();
        loop {
            if let Some(frame) = codec.decode(read_buffer)? {
                return Ok(frame);
            }

            let bytes_read = timeout(timeout_duration, stream.read_buf(read_buffer))
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
        let io_slices: Vec<IoSlice<'_>> = buffers.iter().map(|buf| IoSlice::new(buf)).collect();

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
            self.flush(cfg, write_buffer).await?;

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
                self.flush(cfg, write_buffer).await?;
            }
        } else {
            // Data doesn't fit, flush buffer first then write data directly
            self.flush(cfg, write_buffer).await?;

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
    pub async fn flush(
        &mut self,
        cfg: &FrameConfig,
        write_buffer: &mut WriteBuffer,
    ) -> NetworkResult<()> {
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
}

//! Tokio-based framing helpers for Neo P2P streams with timeouts, size guards, and vectored I/O.
use super::channels_config::ChannelsConfig;
use crate::network::{NetworkError, NetworkResult};
use bytes::BytesMut;
use std::io::IoSlice;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio_util::codec::Decoder;

pub use super::framed_codec::NeoMessageCodec;

/// Initial buffer capacity for reading small messages.
const INITIAL_READ_CAPACITY: usize = 256;

/// Buffer size for vectored writes.
const WRITE_BUFFER_SIZE: usize = 4096;

pub(crate) fn new_read_buffer() -> BytesMut {
    BytesMut::with_capacity(INITIAL_READ_CAPACITY)
}

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

/// Minimal framed reader/writer that wraps Neo P2P length-prefixing with timeouts,
/// size guards, and vectored I/O support.
pub struct FramedSocket<'a> {
    stream: &'a mut TcpStream,
    read_buffer: BytesMut,
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
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncWrite, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    struct ShortVectoredWriter {
        written: Vec<u8>,
        max_bytes_per_write: usize,
        vectored_calls: usize,
    }

    impl ShortVectoredWriter {
        fn new(max_bytes_per_write: usize) -> Self {
            Self {
                written: Vec::new(),
                max_bytes_per_write,
                vectored_calls: 0,
            }
        }
    }

    impl AsyncWrite for ShortVectoredWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let written = buf.len().min(self.max_bytes_per_write);
            self.written.extend_from_slice(&buf[..written]);
            Poll::Ready(Ok(written))
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<std::io::Result<usize>> {
            self.vectored_calls += 1;

            let mut remaining = self.max_bytes_per_write;
            let mut written = 0;
            for buf in bufs {
                if remaining == 0 {
                    break;
                }

                let bytes_to_write = buf.len().min(remaining);
                self.written.extend_from_slice(&buf[..bytes_to_write]);
                remaining -= bytes_to_write;
                written += bytes_to_write;
            }

            Poll::Ready(Ok(written))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

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

    #[tokio::test]
    async fn write_all_vectored_with_timeout_finishes_short_writes() {
        let mut writer = ShortVectoredWriter::new(2);
        let buffers: [&[u8]; 4] = [&b"ab"[..], &[][..], &b"cde"[..], &b"f"[..]];

        write_all_vectored_with_timeout(&mut writer, Duration::from_secs(1), &buffers)
            .await
            .expect("short vectored writes are retried until complete");

        assert_eq!(writer.written, b"abcdef");
        assert!(writer.vectored_calls > 1);
    }

    #[tokio::test]
    async fn write_all_vectored_with_timeout_rejects_zero_byte_write() {
        let mut writer = ShortVectoredWriter::new(0);
        let buffers: [&[u8]; 1] = [&b"a"[..]];

        let result =
            write_all_vectored_with_timeout(&mut writer, Duration::from_secs(1), &buffers).await;

        assert!(
            matches!(result, Err(NetworkError::ConnectionError(ref message)) if message.contains("wrote zero bytes")),
            "expected zero-write connection error, got: {:?}",
            result
        );
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

/// Writes a frame using vectored I/O to reduce syscalls.
pub(crate) async fn write_frame_vectored<W>(
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
pub(crate) async fn write_frame<W>(
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
pub(crate) async fn flush_write_buffer<W>(
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
}

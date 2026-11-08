use super::*;
use neo_primitives::NetworkError;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

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

fn test_frame_config(
    read_timeout_handshake: Duration,
    read_timeout_active: Duration,
) -> FrameConfig {
    FrameConfig {
        read_timeout_handshake,
        read_timeout_active,
        write_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(1),
        read_step_timeout: Duration::from_millis(100),
        use_vectored_io: true,
        buffer_small_writes: true,
    }
}

async fn read_frame(
    stream: &mut TcpStream,
    cfg: &FrameConfig,
    handshake_complete: bool,
) -> NetworkResult<Vec<u8>> {
    let mut reader = FrameReader::new();
    reader
        .read_from_stream(stream, cfg, handshake_complete)
        .await
}

#[tokio::test]
async fn read_frame_times_out_when_peer_silent() {
    let Some((mut client_stream, _server_stream)) = silent_pair().await else {
        return;
    };

    let cfg = test_frame_config(Duration::from_millis(10), Duration::from_millis(10));

    let result = read_frame(&mut client_stream, &cfg, false).await;
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
    let cfg = test_frame_config(Duration::from_secs(5), Duration::from_millis(10));

    let result = read_frame(&mut client_stream, &cfg, true).await;
    assert!(
        matches!(result, Err(NetworkError::Timeout)),
        "expected active timeout error, got: {:?}",
        result
    );
}

#[tokio::test]
async fn read_frame_times_out_when_partial_frame_stalls() {
    let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
        return;
    };
    let cfg = test_frame_config(Duration::from_millis(10), Duration::from_millis(10));

    server_stream
        .write_all(&[0x00, 0x01, 0x03, 0xAA])
        .await
        .expect("server writes partial frame");

    let result = read_frame(&mut client_stream, &cfg, false).await;
    assert!(
        matches!(result, Err(NetworkError::Timeout)),
        "expected partial-frame timeout, got: {:?}",
        result
    );
}

#[tokio::test]
async fn read_frame_rejects_partial_frame_eof() {
    let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
        return;
    };
    let cfg = test_frame_config(Duration::from_secs(1), Duration::from_secs(1));

    server_stream
        .write_all(&[0x00, 0x01, 0x03, 0xAA])
        .await
        .expect("server writes partial frame");
    drop(server_stream);

    let result = read_frame(&mut client_stream, &cfg, false).await;
    assert!(
        matches!(
            result,
            Err(NetworkError::ConnectionError(_)) | Err(NetworkError::InvalidMessage(_))
        ),
        "expected partial-frame EOF to fail closed, got: {:?}",
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
    let cfg = test_frame_config(Duration::from_secs(1), Duration::from_secs(1));

    server_stream
        .write_all(&[0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC])
        .await
        .expect("server writes frame");

    let frame = read_frame(&mut client_stream, &cfg, false)
        .await
        .expect("frame reads");

    assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
}

#[tokio::test]
async fn read_frame_preserves_buffered_next_frame() {
    let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
        return;
    };
    let mut reader = FrameReader::new();
    let cfg = test_frame_config(Duration::from_secs(1), Duration::from_secs(1));

    server_stream
        .write_all(&[0x00, 0x01, 0x01, 0xAA, 0x00, 0x02, 0x01, 0xBB])
        .await
        .expect("server writes frames");

    let first = reader
        .read_from_stream(&mut client_stream, &cfg, false)
        .await
        .expect("first frame");
    let second = reader
        .read_from_stream(&mut client_stream, &cfg, false)
        .await
        .expect("second frame");

    assert_eq!(first, vec![0x00, 0x01, 0x01, 0xAA]);
    assert_eq!(second, vec![0x00, 0x02, 0x01, 0xBB]);
}

#[tokio::test]
async fn read_frame_allows_progressing_frame_across_total_timeout_budget() {
    let Some((mut client_stream, mut server_stream)) = silent_pair().await else {
        return;
    };
    let cfg = test_frame_config(Duration::from_millis(200), Duration::from_millis(200));

    let writer = tokio::spawn(async move {
        for chunk in [
            &[0x00, 0x01, 0x04, 0xAA][..],
            &[0xBB][..],
            &[0xCC][..],
            &[0xDD][..],
        ] {
            server_stream.write_all(chunk).await.expect("write chunk");
            sleep(Duration::from_millis(75)).await;
        }
    });

    let frame = read_frame(&mut client_stream, &cfg, false)
        .await
        .expect("progressing frame reads");
    writer.await.expect("writer task completes");

    assert_eq!(frame, vec![0x00, 0x01, 0x04, 0xAA, 0xBB, 0xCC, 0xDD]);
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

#[test]
fn frame_config_preserves_channels_overrides_and_transport_defaults() {
    let channels = ChannelsConfig {
        handshake_timeout: Duration::from_millis(11),
        read_timeout_active: Duration::from_millis(22),
        write_timeout: Duration::from_millis(33),
        shutdown_timeout: Duration::from_millis(44),
        ..ChannelsConfig::default()
    };
    let borrowed = FrameConfig::from(&channels);
    let owned = FrameConfig::from(channels.clone());

    assert_eq!(borrowed, owned);
    assert_eq!(borrowed.read_timeout_handshake, channels.handshake_timeout);
    assert_eq!(borrowed.read_timeout_active, channels.read_timeout_active);
    assert_eq!(borrowed.write_timeout, channels.write_timeout);
    assert_eq!(borrowed.shutdown_timeout, channels.shutdown_timeout);
    assert_eq!(borrowed.read_step_timeout, Duration::from_millis(500));
    assert!(borrowed.use_vectored_io);
    assert!(borrowed.buffer_small_writes);
    assert_eq!(WriteBuffer::default().threshold, WRITE_BUFFER_SIZE);
}

#[tokio::test]
async fn read_frame_preserves_partial_buffer_after_timeout_and_resumes() {
    let (mut client_stream, mut server_stream) = silent_pair()
        .await
        .expect("loopback permission is required for migration parity");
    let cfg = test_frame_config(Duration::from_millis(20), Duration::from_secs(1));
    let mut reader = FrameReader::default();
    let partial = [0x00, 0x01, 0xFD, 0x03];
    server_stream
        .write_all(&partial)
        .await
        .expect("write partial");

    assert!(matches!(
        reader
            .read_from_stream(&mut client_stream, &cfg, false)
            .await,
        Err(NetworkError::Timeout)
    ));
    assert_eq!(&reader.buffer[..], &partial);

    server_stream
        .write_all(&[0x00, 0xAA, 0xBB, 0xCC])
        .await
        .expect("write remaining varint and payload");
    let frame = reader
        .read_from_stream(&mut client_stream, &cfg, true)
        .await
        .expect("resume the same persistent reader");
    assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
    assert!(reader.buffer.is_empty());
}

#[tokio::test]
async fn read_step_preserves_partial_buffer_across_idle_and_pending() {
    let (mut client_stream, mut server_stream) = silent_pair()
        .await
        .expect("loopback permission is required for migration parity");
    let mut cfg = test_frame_config(Duration::from_secs(1), Duration::from_secs(1));
    cfg.read_step_timeout = Duration::from_millis(10);
    let mut reader = FrameReader::new();
    let partial = [0x00, 0x01, 0xFD, 0x03];
    reader.buffer.extend_from_slice(&partial);

    assert!(matches!(
        reader.read_step(&mut client_stream, &cfg, false).await,
        Ok(FrameReadStep::Idle)
    ));
    assert_eq!(&reader.buffer[..], &partial);

    server_stream
        .write_all(&[0x00, 0xAA, 0xBB, 0xCC])
        .await
        .expect("write remaining varint and payload");
    let frame = timeout(Duration::from_secs(1), async {
        loop {
            match reader
                .read_step(&mut client_stream, &cfg, true)
                .await
                .expect("step reads")
            {
                FrameReadStep::Frame(frame) => break frame,
                FrameReadStep::Pending => assert!(reader.buffer.starts_with(&partial)),
                FrameReadStep::Idle => {}
                FrameReadStep::Closed => panic!("peer remains open"),
            }
        }
    })
    .await
    .expect("fragmented frame completes within test deadline");
    assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
    assert!(reader.buffer.is_empty());
}

#[tokio::test]
async fn read_step_drains_buffered_frames_before_reporting_closed() {
    let (mut client_stream, server_stream) = silent_pair()
        .await
        .expect("loopback permission is required for migration parity");
    drop(server_stream);
    let cfg = FrameConfig::default();
    let mut reader = FrameReader::new();
    reader
        .buffer
        .extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x10, 0x00]);

    for expected in [vec![0x00, 0x01, 0x00], vec![0x00, 0x10, 0x00]] {
        let result = reader
            .read_step(&mut client_stream, &cfg, true)
            .await
            .expect("buffered frame decodes");
        let FrameReadStep::Frame(frame) = result else {
            panic!("buffered frame must precede EOF");
        };
        assert_eq!(frame, expected);
    }
    assert!(matches!(
        reader.read_step(&mut client_stream, &cfg, true).await,
        Ok(FrameReadStep::Closed)
    ));
}

#[tokio::test]
async fn read_step_reports_partial_eof_without_discarding_bytes() {
    let (mut client_stream, server_stream) = silent_pair()
        .await
        .expect("loopback permission is required for migration parity");
    drop(server_stream);
    let cfg = FrameConfig::default();
    let mut reader = FrameReader::new();
    let partial = [0x00, 0x01, 0x03, 0xAA];
    reader.buffer.extend_from_slice(&partial);

    assert!(matches!(
        reader.read_step(&mut client_stream, &cfg, true).await,
        Ok(FrameReadStep::Closed)
    ));
    assert_eq!(&reader.buffer[..], &partial);
}

#[tokio::test]
async fn reader_paths_share_the_message_payload_limit_and_primitive_error() {
    let (mut client_stream, _server_stream) = silent_pair()
        .await
        .expect("loopback permission is required for migration parity");
    let cfg = FrameConfig::default();
    let mut header = vec![0x00, 0x2C];
    neo_io_crate::var_int::write_var_int(crate::message::PAYLOAD_MAX_SIZE as u64 + 1, &mut header);

    let mut reader = FrameReader::new();
    reader.buffer.extend_from_slice(&header);
    let full: neo_primitives::NetworkResult<Vec<u8>> = reader
        .read_from_stream(&mut client_stream, &cfg, true)
        .await;
    assert!(matches!(
        full,
        Err(neo_primitives::NetworkError::InvalidMessage(_))
    ));
    assert_eq!(&reader.buffer[..], header.as_slice());

    let step: neo_primitives::NetworkResult<FrameReadStep> =
        reader.read_step(&mut client_stream, &cfg, true).await;
    assert!(matches!(
        step,
        Err(neo_primitives::NetworkError::InvalidMessage(_))
    ));
    assert_eq!(&reader.buffer[..], header.as_slice());
}

#[tokio::test]
async fn public_write_helpers_preserve_buffer_order_and_short_write_bytes() {
    let cfg = FrameConfig::default();
    let mut writer = ShortVectoredWriter::new(2);
    let mut buffer = WriteBuffer::new(5);

    write_frame(&mut writer, &cfg, b"ab", &mut buffer)
        .await
        .expect("buffer small frame");
    assert!(writer.written.is_empty());
    assert_eq!(buffer.len(), 2);

    write_frame(&mut writer, &cfg, b"cdef", &mut buffer)
        .await
        .expect("flush before frame that does not fit");
    assert_eq!(writer.written, b"abcdef");
    assert!(buffer.is_empty());

    write_frame(&mut writer, &cfg, b"gh", &mut buffer)
        .await
        .expect("buffer next frame");
    flush_write_buffer(&mut writer, &cfg, &mut buffer)
        .await
        .expect("flush remaining frame");
    assert_eq!(writer.written, b"abcdefgh");
    assert!(buffer.is_empty());

    write_frame_vectored(&mut writer, &cfg, &[&b"ij"[..], &[], &b"klm"[..]])
        .await
        .expect("public vectored helper handles short writes");
    assert_eq!(writer.written, b"abcdefghijklm");
    assert!(writer.vectored_calls > 1);
}

#[tokio::test]
async fn public_write_helpers_preserve_disabled_buffering_and_vectored_fallback() {
    let cfg = FrameConfig {
        buffer_small_writes: false,
        use_vectored_io: false,
        ..FrameConfig::default()
    };
    let mut writer = ShortVectoredWriter::new(1);
    let mut buffer = WriteBuffer::new(16);
    assert!(buffer.push(b"old"));

    write_frame(&mut writer, &cfg, b"new", &mut buffer)
        .await
        .expect("disabled buffering flushes before direct write");
    write_frame_vectored(&mut writer, &cfg, &[&b"ab"[..], &[], &b"cd"[..]])
        .await
        .expect("disabled vectored I/O writes buffers in order");
    assert_eq!(writer.written, b"oldnewabcd");
    assert_eq!(writer.vectored_calls, 0);
    assert!(buffer.is_empty());
}

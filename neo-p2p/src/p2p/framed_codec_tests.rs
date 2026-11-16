use super::*;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{FramedRead, FramedWrite};

async fn read_one_framed_frame(chunks: &[&[u8]]) -> Result<Vec<u8>, NetworkError> {
    let (reader, mut writer) = tokio::io::duplex(64);
    let chunks = chunks
        .iter()
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    let writer_task = tokio::spawn(async move {
        for chunk in chunks {
            writer.write_all(&chunk).await.expect("write chunk");
        }
    });

    let mut framed = FramedRead::new(reader, NeoMessageCodec::default());
    let frame = framed.next().await.expect("framed reader yields a frame")?;

    writer_task.await.expect("writer task completes");
    Ok(frame)
}

#[test]
fn decodes_complete_frame() {
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
fn waits_for_partial_frame() {
    let mut codec = NeoMessageCodec::default();
    let mut input = BytesMut::from(&[0x00, 0x01, 0xFD, 0x03][..]);

    let frame = codec.decode(&mut input).expect("partial decode succeeds");

    assert!(frame.is_none());
    assert_eq!(&input[..], &[0x00, 0x01, 0xFD, 0x03]);
}

#[test]
fn waits_for_partial_payload_after_var_int() {
    let mut codec = NeoMessageCodec::default();
    let mut input = BytesMut::from(&[0x00, 0x01, 0x03, 0xAA][..]);

    let frame = codec.decode(&mut input).expect("partial decode succeeds");

    assert!(frame.is_none());
    assert_eq!(&input[..], &[0x00, 0x01, 0x03, 0xAA]);
}

#[test]
fn rejects_oversized_payload() {
    let mut codec = NeoMessageCodec::new(1);
    let mut input = BytesMut::from(&[0x00, 0x01, 0x02][..]);

    let result = codec.decode(&mut input);

    assert!(matches!(result, Err(NetworkError::InvalidMessage(_))));
}

#[test]
fn canonicalizes_var_int_prefix() {
    let mut codec = NeoMessageCodec::default();
    let mut input = BytesMut::from(&[0x00, 0x01, 0xFD, 0x01, 0x00, 0xAA][..]);

    let frame = codec
        .decode(&mut input)
        .expect("decode succeeds")
        .expect("frame is complete");

    assert_eq!(frame, vec![0x00, 0x01, 0x01, 0xAA]);
}

#[test]
fn canonicalizes_large_non_canonical_var_int_prefix() {
    let mut codec = NeoMessageCodec::default();
    let mut input = BytesMut::from(&[0x00, 0x01, 0xFE, 0x01, 0x00, 0x00, 0x00, 0xAA][..]);

    let frame = codec
        .decode(&mut input)
        .expect("decode succeeds")
        .expect("frame is complete");

    assert_eq!(frame, vec![0x00, 0x01, 0x01, 0xAA]);
}

#[tokio::test]
async fn framed_read_decodes_complete_frame() {
    let frame = read_one_framed_frame(&[&[0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]])
        .await
        .expect("complete frame decodes");

    assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
}

#[tokio::test]
async fn framed_read_decodes_split_header_var_int_and_payload() {
    let frame = read_one_framed_frame(&[
        &[0x00],
        &[0x01, 0xFD],
        &[0x03],
        &[0x00, 0xAA],
        &[0xBB, 0xCC],
    ])
    .await
    .expect("split frame decodes");

    assert_eq!(frame, vec![0x00, 0x01, 0x03, 0xAA, 0xBB, 0xCC]);
}

#[tokio::test]
async fn framed_read_preserves_two_frames_from_one_read() {
    let (reader, mut writer) = tokio::io::duplex(64);
    writer
        .write_all(&[0x00, 0x01, 0x01, 0xAA, 0x00, 0x02, 0x01, 0xBB])
        .await
        .expect("write two frames");
    drop(writer);

    let mut framed = FramedRead::new(reader, NeoMessageCodec::default());
    let first = framed
        .next()
        .await
        .expect("first frame")
        .expect("first frame decodes");
    let second = framed
        .next()
        .await
        .expect("second frame")
        .expect("second frame decodes");

    assert_eq!(first, vec![0x00, 0x01, 0x01, 0xAA]);
    assert_eq!(second, vec![0x00, 0x02, 0x01, 0xBB]);
}

#[tokio::test]
async fn framed_read_rejects_oversized_payload() {
    let (reader, mut writer) = tokio::io::duplex(64);
    writer
        .write_all(&[0x00, 0x01, 0x02, 0xAA, 0xBB])
        .await
        .expect("write oversized frame");
    drop(writer);

    let mut framed = FramedRead::new(reader, NeoMessageCodec::new(1));
    let result = framed.next().await.expect("framed reader yields error");

    assert!(matches!(result, Err(NetworkError::InvalidMessage(_))));
}

#[tokio::test]
async fn framed_read_decodes_empty_payload_commands() {
    for expected in [&[0x00, 0x01, 0x00][..], &[0x00, 0x10, 0x00][..]] {
        let frame = read_one_framed_frame(&[expected])
            .await
            .expect("empty payload frame decodes");
        assert_eq!(frame, expected);
    }
}

#[tokio::test]
async fn framed_read_canonicalizes_non_canonical_var_int_prefix() {
    let frame = read_one_framed_frame(&[&[0x00, 0x01, 0xFD, 0x01, 0x00, 0xAA]])
        .await
        .expect("non-canonical frame decodes");

    assert_eq!(frame, vec![0x00, 0x01, 0x01, 0xAA]);
}

#[tokio::test]
async fn framed_write_preserves_raw_wire_bytes() {
    for expected in [
        &[0x00, 0x18, 0x03, 0xAA, 0xBB, 0xCC][..],
        &[0x00, 0x00, 0x01, 0xAA][..],
        &[0x00, 0x01, 0x00][..],
        &[0x01, 0x2C, 0xFD, 0x01, 0x00, 0xAA][..],
    ] {
        let (writer, mut reader) = tokio::io::duplex(expected.len() + 8);
        let mut framed = FramedWrite::new(writer, NeoMessageCodec::default());
        framed.send(expected).await.expect("framed write succeeds");

        let mut actual = vec![0; expected.len()];
        reader
            .read_exact(&mut actual)
            .await
            .expect("read written frame");

        assert_eq!(actual, expected);
    }
}

fn frame_header(payload_length: u64) -> Vec<u8> {
    let mut header = vec![0x00, 0x2C];
    write_var_int(payload_length, &mut header);
    header
}

#[test]
fn default_codec_uses_message_payload_limit() {
    let mut codec = NeoMessageCodec::default();
    assert_eq!(codec.max_payload_size, PAYLOAD_MAX_SIZE);

    let mut at_limit = BytesMut::from(frame_header(PAYLOAD_MAX_SIZE as u64).as_slice());
    let original = at_limit.clone();
    let result: NetworkResult<Option<Vec<u8>>> = codec.decode(&mut at_limit);
    assert!(
        result
            .expect("maximum payload header is accepted")
            .is_none()
    );
    assert_eq!(at_limit, original);

    let mut oversized = BytesMut::from(frame_header(PAYLOAD_MAX_SIZE as u64 + 1).as_slice());
    let original = oversized.clone();
    assert!(matches!(
        codec.decode(&mut oversized),
        Err(NetworkError::InvalidMessage(_))
    ));
    assert_eq!(oversized, original);
}

#[test]
fn default_codec_accepts_maximum_payload_without_counting_header() {
    let header = frame_header(PAYLOAD_MAX_SIZE as u64);
    let mut input = BytesMut::from(header.as_slice());
    input.resize(header.len() + PAYLOAD_MAX_SIZE, 0xAA);

    let frame = NeoMessageCodec::default()
        .decode(&mut input)
        .expect("maximum payload decodes")
        .expect("maximum payload is complete");

    assert_eq!(frame.len(), header.len() + PAYLOAD_MAX_SIZE);
    assert_eq!(&frame[..header.len()], header.as_slice());
    assert!(frame[header.len()..].iter().all(|&byte| byte == 0xAA));
    assert!(input.is_empty());
}

#[test]
fn preserves_every_partial_header_var_int_and_payload_prefix() {
    for prefix in [
        &[0x03][..],
        &[0xFD, 0x03, 0x00][..],
        &[0xFE, 0x03, 0x00, 0x00, 0x00][..],
        &[0xFF, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00][..],
    ] {
        let mut wire = vec![0x01, 0x2C];
        wire.extend_from_slice(prefix);
        wire.extend_from_slice(&[0xAA, 0xBB, 0xCC]);

        for split in 0..wire.len() {
            let mut codec = NeoMessageCodec::default();
            let mut input = BytesMut::from(&wire[..split]);
            let partial = input.clone();
            assert!(codec.decode(&mut input).expect("partial decode").is_none());
            assert_eq!(input, partial);

            input.extend_from_slice(&wire[split..]);
            input.extend_from_slice(&[0x00, 0x01, 0x00]);
            assert_eq!(
                codec.decode(&mut input).expect("complete decode"),
                Some(vec![0x01, 0x2C, 0x03, 0xAA, 0xBB, 0xCC])
            );
            assert_eq!(&input[..], &[0x00, 0x01, 0x00]);
            assert_eq!(
                codec.decode(&mut input).expect("next decode"),
                Some(vec![0x00, 0x01, 0x00])
            );
            assert!(input.is_empty());
        }
    }
}

#[test]
fn rejects_u64_maximum_payload_without_consuming_input() {
    let mut input = BytesMut::from(frame_header(u64::MAX).as_slice());
    let original = input.clone();
    assert!(matches!(
        NeoMessageCodec::default().decode(&mut input),
        Err(NetworkError::InvalidMessage(_))
    ));
    assert_eq!(input, original);
}

#[test]
fn rejects_frame_length_overflow_with_custom_limit() {
    let mut input = BytesMut::from(frame_header(usize::MAX as u64).as_slice());
    let original = input.clone();
    assert!(
        matches!(NeoMessageCodec::new(usize::MAX).decode(&mut input), Err(NetworkError::InvalidMessage(ref message)) if message == "Frame length overflow")
    );
    assert_eq!(input, original);
}

#[tokio::test]
async fn framed_read_rejects_truncated_header_var_int_and_payload_at_eof() {
    for partial in [
        &[0x00][..],
        &[0x00, 0x01, 0xFD, 0x03][..],
        &[0x00, 0x01, 0xFE, 0x03, 0x00, 0x00][..],
        &[0x00, 0x01, 0xFF, 0x03, 0x00, 0x00, 0x00][..],
        &[0x00, 0x01, 0x03, 0xAA][..],
    ] {
        let (reader, mut writer) = tokio::io::duplex(64);
        writer
            .write_all(partial)
            .await
            .expect("write partial frame");
        drop(writer);

        let mut framed = FramedRead::new(reader, NeoMessageCodec::default());
        let result = framed.next().await.expect("truncated frame yields error");
        assert!(matches!(result, Err(NetworkError::ConnectionError(_))));
    }
}

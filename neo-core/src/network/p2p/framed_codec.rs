use super::message::PAYLOAD_MAX_SIZE;
use crate::network::NetworkError;
use bytes::{BufMut, BytesMut};
use neo_io_crate::var_int::{read_var_int_prefix, write_var_int};
use tokio_util::codec::{Decoder, Encoder};

const FRAME_HEADER_LEN: usize = 2;
const INITIAL_FRAME_CAPACITY: usize = 256;

/// Neo P2P message codec for flags + command + var-bytes payload frames.
#[derive(Clone, Debug)]
pub struct NeoMessageCodec {
    max_payload_size: usize,
}

impl NeoMessageCodec {
    pub fn new(max_payload_size: usize) -> Self {
        Self { max_payload_size }
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

        let Some((payload_length, varint_len)) = read_var_int_prefix(&src[FRAME_HEADER_LEN..])
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
        let mut message = Vec::with_capacity(frame_len.max(INITIAL_FRAME_CAPACITY));
        message.extend_from_slice(&frame[..FRAME_HEADER_LEN]);
        write_var_int(payload_length as u64, &mut message);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::{
        payloads::{PingPayload, VersionPayload},
        NetworkMessage, ProtocolMessage,
    };
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
        let verack = NetworkMessage::new(ProtocolMessage::Verack)
            .to_bytes(true)
            .expect("verack serializes");
        let getaddr = NetworkMessage::new(ProtocolMessage::GetAddr)
            .to_bytes(true)
            .expect("getaddr serializes");

        let first = read_one_framed_frame(&[&verack])
            .await
            .expect("verack frame decodes");
        let second = read_one_framed_frame(&[&getaddr])
            .await
            .expect("getaddr frame decodes");

        assert_eq!(first, verack);
        assert_eq!(second, getaddr);
    }

    #[tokio::test]
    async fn framed_read_canonicalizes_non_canonical_var_int_prefix() {
        let frame = read_one_framed_frame(&[&[0x00, 0x01, 0xFD, 0x01, 0x00, 0xAA]])
            .await
            .expect("non-canonical frame decodes");

        assert_eq!(frame, vec![0x00, 0x01, 0x01, 0xAA]);
    }

    #[tokio::test]
    async fn framed_write_matches_network_message_bytes() {
        let ping = NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(
            77, 1234,
        )))
        .to_bytes(true)
        .expect("ping serializes");
        let version = NetworkMessage::new(ProtocolMessage::Version(VersionPayload::default()))
            .to_bytes(true)
            .expect("version serializes");
        let verack = NetworkMessage::new(ProtocolMessage::Verack)
            .to_bytes(true)
            .expect("verack serializes");

        for expected in [ping, version, verack] {
            let (writer, mut reader) = tokio::io::duplex(expected.len() + 8);
            let mut framed = FramedWrite::new(writer, NeoMessageCodec::default());
            framed
                .send(expected.as_slice())
                .await
                .expect("framed write succeeds");

            let mut actual = vec![0; expected.len()];
            reader
                .read_exact(&mut actual)
                .await
                .expect("read written frame");

            assert_eq!(actual, expected);
        }
    }
}

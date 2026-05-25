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
}

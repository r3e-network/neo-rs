use crate::message::PAYLOAD_MAX_SIZE;
use bytes::{BufMut, BytesMut};
use neo_io::var_int::{read_var_int_prefix, write_var_int};
use neo_primitives::{NetworkError, NetworkResult};
use tokio_util::codec::{Decoder, Encoder};

const FRAME_HEADER_LEN: usize = 2;
const INITIAL_FRAME_CAPACITY: usize = 256;

/// Neo P2P message codec for flags + command + var-bytes payload frames.
#[derive(Clone, Debug)]
pub struct NeoMessageCodec {
    max_payload_size: usize,
}

impl NeoMessageCodec {
    /// Creates a codec that rejects frames whose payload exceeds `max_payload_size` bytes.
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

    fn decode(&mut self, src: &mut BytesMut) -> NetworkResult<Option<Self::Item>> {
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

    fn encode(&mut self, item: &'a [u8], dst: &mut BytesMut) -> NetworkResult<()> {
        dst.reserve(item.len());
        dst.put_slice(item);
        Ok(())
    }
}

#[cfg(test)]
#[path = "framed_codec_tests.rs"]
mod tests;

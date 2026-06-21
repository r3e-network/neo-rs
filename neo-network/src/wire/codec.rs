//! Tokio framed codec for the Neo P2P wire protocol.
//!
//! The codec is gated behind the `codec` Cargo feature so the basic
//! `Message` / `NetworkMessage` types can be used without pulling in
//! the Tokio dependency tree.

use super::error::{WireError, WireResult};
use super::message::{Message, PAYLOAD_MAX_SIZE};
use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Minimum number of bytes a complete wire message can occupy
/// (flags byte + command byte + 1-byte var-int).
const MIN_MESSAGE_LEN: usize = 3;

/// Tokio framed codec that splits an inbound byte stream into
/// `Message` frames and encodes outbound `Message` frames back into
/// their wire bytes.
#[derive(Debug, Default, Clone)]
pub struct MessageCodec;

impl MessageCodec {
    /// Creates a new codec instance.
    pub fn new() -> Self {
        Self
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = WireError;

    fn decode(&mut self, src: &mut BytesMut) -> WireResult<Option<Self::Item>> {
        if src.len() < MIN_MESSAGE_LEN {
            return Ok(None);
        }

        // Peek at the length prefix without consuming the buffer, reusing the
        // canonical var-int prefix reader from neo-io.
        let Some((payload_len, payload_len_size)) =
            neo_io::var_int::VarInt::read_var_int_prefix(&src[2..])
        else {
            return Ok(None);
        };

        if payload_len > PAYLOAD_MAX_SIZE as u64 {
            return Err(WireError::PayloadTooLarge(
                payload_len as usize,
                PAYLOAD_MAX_SIZE,
            ));
        }

        let total_len = 2 + payload_len_size + payload_len as usize;
        if src.len() < total_len {
            // Reserve room for the rest of the frame to avoid repeat
            // allocations.
            src.reserve(total_len - src.len());
            return Ok(None);
        }

        let frame = src.split_to(total_len);
        let message = Message::from_bytes(&frame)?;
        Ok(Some(message))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = WireError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> WireResult<()> {
        let bytes = item.to_bytes()?;
        dst.reserve(bytes.len());
        dst.put_slice(&bytes);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/wire/codec.rs"]
mod tests;

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

        // Peek at the length prefix without consuming the buffer.
        let Some((payload_len, payload_len_size)) = peek_var_int(&src[2..])? else {
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

/// Decodes a Neo `var_int` from a byte slice without consuming it.
/// Returns `Ok(None)` if more bytes are needed to fully decode the
/// value.
fn peek_var_int(bytes: &[u8]) -> WireResult<Option<(u64, usize)>> {
    let Some(&first) = bytes.first() else {
        return Ok(None);
    };

    match first {
        0xFD => {
            if bytes.len() < 3 {
                return Ok(None);
            }
            let mut v = [0u8; 2];
            v.copy_from_slice(&bytes[1..3]);
            Ok(Some((u16::from_le_bytes(v) as u64, 3)))
        }
        0xFE => {
            if bytes.len() < 5 {
                return Ok(None);
            }
            let mut v = [0u8; 4];
            v.copy_from_slice(&bytes[1..5]);
            Ok(Some((u32::from_le_bytes(v) as u64, 5)))
        }
        0xFF => {
            if bytes.len() < 9 {
                return Ok(None);
            }
            let mut v = [0u8; 8];
            v.copy_from_slice(&bytes[1..9]);
            Ok(Some((u64::from_le_bytes(v), 9)))
        }
        _ => Ok(Some((first as u64, 1))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_p2p::MessageCommand;
    use neo_p2p::payloads::PingPayload;

    #[test]
    fn codec_encodes_and_decodes_ping_message() {
        let mut codec = MessageCodec::new();
        let ping = PingPayload::create(99);
        let msg = Message::create(MessageCommand::Ping, Some(&ping), false).expect("create");

        let mut buf = BytesMut::new();
        codec.encode(msg.clone(), &mut buf).expect("encode");

        let decoded = codec.decode(&mut buf).expect("decode").expect("frame");
        assert_eq!(decoded.command, MessageCommand::Ping);
        assert_eq!(decoded.payload_raw, msg.payload_raw);
        assert!(buf.is_empty());
    }

    #[test]
    fn codec_returns_none_for_partial_frame() {
        let mut codec = MessageCodec::new();
        let mut buf = BytesMut::from(&[0x00u8][..]);
        assert!(codec.decode(&mut buf).expect("decode").is_none());
    }

    #[test]
    fn codec_decodes_two_frames_from_one_buffer() {
        let mut codec = MessageCodec::new();
        let msg1 = Message::create(MessageCommand::Ping, Some(&PingPayload::create(1)), false)
            .expect("create");
        let msg2 = Message::create(MessageCommand::Ping, Some(&PingPayload::create(2)), false)
            .expect("create");

        let mut buf = BytesMut::new();
        codec.encode(msg1.clone(), &mut buf).expect("encode 1");
        codec.encode(msg2.clone(), &mut buf).expect("encode 2");

        let d1 = codec.decode(&mut buf).expect("decode 1").expect("frame 1");
        let d2 = codec.decode(&mut buf).expect("decode 2").expect("frame 2");
        assert!(buf.is_empty());
        assert_eq!(d1.payload_raw, msg1.payload_raw);
        assert_eq!(d2.payload_raw, msg2.payload_raw);
    }
}

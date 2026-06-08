//! Wire-level [`Message`] - the on-the-wire representation of a Neo
//! P2P network frame, mirroring `Neo.Network.P2P.Message`.
//!
//! ```text
//! ┌──────────┬──────────┬──────────────┬──────────┐
//! │  Flags   │ Command  │    Length    │ Payload  │
//! │ (1 byte) │ (1 byte) │ (var_int LE) │  (var)   │
//! └──────────┴──────────┴──────────────┴──────────┘
//! ```
//!
//! The payload bytes are either the raw serialised payload or, when
//! the compression bit is set in `Flags`, the LZ4-compressed payload.

use crate::error::{WireError, WireResult};
use neo_extensions::compression::{
    compress_lz4, decompress_lz4, COMPRESSION_MIN_SIZE,
};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_p2p::{MessageCommand, MessageFlags};
use serde::{Deserialize, Serialize};

/// Maximum payload size (matches `Neo.Network.P2P.Message.PayloadMaxSize` = 32 MiB).
pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000;

/// Default buffer capacity for small messages.
const DEFAULT_MESSAGE_CAPACITY: usize = 256;

/// Threshold above which LZ4 compression is attempted.
pub const COMPRESSION_THRESHOLD: usize = COMPRESSION_MIN_SIZE;

/// Neo wire-level network message (parity with `Neo.Network.P2P.Message`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Message flags (e.g. compression bit).
    pub flags: MessageFlags,
    /// Message command discriminator.
    pub command: MessageCommand,
    /// Uncompressed payload bytes (matches C# `_payloadRaw`).
    pub payload_raw: Vec<u8>,
    /// Wire-format payload bytes (matches C# `_payloadCompressed`).
    pub payload_compressed: Vec<u8>,
}

impl Message {
    /// Creates a new message from a typed payload. When `enable_compression`
    /// is true and the serialised payload exceeds the compression threshold,
    /// the wire bytes are LZ4-compressed and the [`MessageFlags::COMPRESSED`]
    /// bit is set on the resulting message.
    pub fn create<T>(
        command: MessageCommand,
        payload: Option<&T>,
        enable_compression: bool,
    ) -> WireResult<Self>
    where
        T: Serializable,
    {
        let estimated_capacity = payload
            .map(|p| p.size().max(DEFAULT_MESSAGE_CAPACITY))
            .unwrap_or(0);
        let mut writer = BinaryWriter::with_capacity(estimated_capacity);
        if let Some(p) = payload {
            Serializable::serialize(p, &mut writer)?;
        }
        let raw = writer.into_bytes();
        Self::from_payload_bytes(command, raw, enable_compression)
    }

    /// Creates a new message from already-serialised payload bytes.
    pub fn from_payload_bytes(
        command: MessageCommand,
        payload_raw: Vec<u8>,
        enable_compression: bool,
    ) -> WireResult<Self> {
        if payload_raw.len() > PAYLOAD_MAX_SIZE {
            return Err(WireError::PayloadTooLarge(
                payload_raw.len(),
                PAYLOAD_MAX_SIZE,
            ));
        }

        let (flags, payload_compressed) = if enable_compression
            && command.allows_compression()
            && payload_raw.len() >= COMPRESSION_THRESHOLD
        {
            let compressed = compress_lz4(&payload_raw)
                .map_err(|e| WireError::Compression(e.to_string()))?;
            if compressed.len() < payload_raw.len() {
                (MessageFlags::COMPRESSED, compressed)
            } else {
                (MessageFlags::NONE, payload_raw.clone())
            }
        } else {
            (MessageFlags::NONE, payload_raw.clone())
        };

        Ok(Self {
            flags,
            command,
            payload_raw,
            payload_compressed,
        })
    }

    /// Returns the on-the-wire size (header + var-int length prefix + payload).
    pub fn wire_size(&self) -> usize {
        let payload_len = self.payload_compressed.len();
        2 + neo_io::var_int::encoded_len(payload_len as u64) + payload_len
    }

    /// Encodes the message into its on-the-wire byte sequence.
    pub fn to_bytes(&self) -> WireResult<Vec<u8>> {
        let mut buf = Vec::with_capacity(self.wire_size());
        buf.push(self.flags.bits());
        buf.push(self.command.to_byte());
        neo_io::var_int::write_var_bytes(&self.payload_compressed, &mut buf);
        Ok(buf)
    }

    /// Decodes a `Message` from a complete wire byte sequence.
    pub fn from_bytes(bytes: &[u8]) -> WireResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        let flags = MessageFlags::from_bits(reader.read_u8()?)
            .ok_or_else(|| WireError::InvalidMessage(format!("invalid flags byte")))?;
        let command = MessageCommand::from_byte(reader.read_u8()?)
            .map_err(|e| WireError::InvalidMessage(format!("invalid command byte: {e}")))?;
        let payload_compressed = reader
            .read_var_bytes(PAYLOAD_MAX_SIZE)
            .map_err(|e| WireError::InvalidMessage(format!("invalid payload length: {e}")))?;

        let payload_raw = if flags.contains(MessageFlags::COMPRESSED) {
            decompress_lz4(&payload_compressed, PAYLOAD_MAX_SIZE)
                .map_err(|e| WireError::Compression(e.to_string()))?
        } else {
            payload_compressed.clone()
        };

        Ok(Self {
            flags,
            command,
            payload_raw,
            payload_compressed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_p2p::payloads::PingPayload;

    #[test]
    fn message_round_trip_uncompressed_ping() {
        let ping = PingPayload::create(42);
        let msg = Message::create(MessageCommand::Ping, Some(&ping), false).expect("create");
        let bytes = msg.to_bytes().expect("encode");
        let decoded = Message::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.command, MessageCommand::Ping);
        assert_eq!(decoded.payload_raw, msg.payload_raw);
        assert_eq!(decoded.flags, MessageFlags::NONE);
    }

    #[test]
    fn message_compresses_large_payload_when_allowed() {
        let payload = vec![0xABu8; COMPRESSION_THRESHOLD + 100];
        let msg = Message::from_payload_bytes(MessageCommand::FilterAdd, payload.clone(), true)
            .expect("create");
        assert_eq!(msg.flags, MessageFlags::COMPRESSED);
        assert!(msg.payload_compressed.len() < payload.len());

        let bytes = msg.to_bytes().expect("encode");
        let decoded = Message::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.payload_raw, payload);
    }

    #[test]
    fn message_rejects_oversized_payload() {
        let payload = vec![0u8; PAYLOAD_MAX_SIZE + 1];
        let err = Message::from_payload_bytes(MessageCommand::Block, payload, false)
            .expect_err("must reject");
        assert!(matches!(err, WireError::PayloadTooLarge(_, _)));
    }
}

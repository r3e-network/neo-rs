//! Raw P2P message framing (mirrors `Neo.Network.P2P.Message`).
//!
//! This module provides wire-format message handling without requiring
//! knowledge of concrete payload types. Higher layers (neo-core) handle
//! typed payload deserialization.

use crate::{MessageCommand, MessageFlags, P2PError, P2PResult};
use neo_io::compression::{compress_lz4, decompress_lz4, COMPRESSION_MIN_SIZE};
use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};

/// Maximum payload size (matches `Neo.Network.P2P.Message.PayloadMaxSize`).
pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32 MiB

/// Default buffer capacity for small messages.
const DEFAULT_MESSAGE_CAPACITY: usize = 256;

/// Raw P2P message with uncompressed payload bytes.
///
/// This is the wire-format representation. Concrete payload types
/// are deserialized by the caller (in neo-core).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessage {
    /// Message flags (e.g. compression bit).
    pub flags: MessageFlags,
    /// Message command discriminator.
    pub command: MessageCommand,
    /// Uncompressed payload bytes.
    pub payload: Vec<u8>,
}

impl RawMessage {
    /// Creates a new raw message from a command and serialized payload.
    pub fn new(command: MessageCommand, payload: Vec<u8>) -> Self {
        Self {
            flags: MessageFlags::NONE,
            command,
            payload,
        }
    }

    /// Creates a raw message from a serializable payload.
    pub fn from_serializable<T: Serializable>(
        command: MessageCommand,
        payload: &T,
    ) -> IoResult<Self> {
        let mut writer = BinaryWriter::with_capacity(payload.size().max(DEFAULT_MESSAGE_CAPACITY));
        payload.serialize(&mut writer)?;
        Ok(Self::new(command, writer.into_bytes()))
    }

    /// Serializes this message to wire format, optionally compressing the payload.
    pub fn to_bytes(&self, enable_compression: bool) -> IoResult<Vec<u8>> {
        let (flags, wire_payload) = if enable_compression
            && self.payload.len() >= COMPRESSION_MIN_SIZE
        {
            let compressed = compress_lz4(&self.payload)
                .map_err(|e| neo_io::IoError::invalid_data(e.to_string()))?;
            if compressed.len() < self.payload.len() {
                (MessageFlags::COMPRESSED, compressed)
            } else {
                (MessageFlags::NONE, self.payload.clone())
            }
        } else {
            (MessageFlags::NONE, self.payload.clone())
        };

        let mut writer = BinaryWriter::with_capacity(2 + wire_payload.len() + 8);
        writer.write_u8(flags.bits())?;
        writer.write_u8(self.command.to_byte())?;
        writer.write_var_bytes(&wire_payload)?;
        Ok(writer.into_bytes())
    }

    /// Deserializes a message from wire format bytes.
    pub fn from_bytes(data: &[u8]) -> P2PResult<Self> {
        let mut reader = MemoryReader::new(data);

        let flags_byte = reader
            .read_u8()
            .map_err(|e| P2PError::protocol_error(format!("Failed to read flags: {e}")))?;
        let flags = MessageFlags::from_bits_truncate(flags_byte);

        let command_byte = reader
            .read_u8()
            .map_err(|e| P2PError::protocol_error(format!("Failed to read command: {e}")))?;
        let command = MessageCommand::from_byte(command_byte)
            .map_err(|e| P2PError::protocol_error(format!("Unknown command: {e}")))?;

        let wire_payload = reader
            .read_var_bytes(PAYLOAD_MAX_SIZE)
            .map_err(|e| P2PError::protocol_error(format!("Failed to read payload: {e}")))?;

        let payload = if flags.contains(MessageFlags::COMPRESSED) {
            decompress_lz4(&wire_payload, PAYLOAD_MAX_SIZE)
                .map_err(|e| P2PError::protocol_error(format!("Decompression failed: {e}")))?
        } else {
            wire_payload
        };

        Ok(Self {
            flags,
            command,
            payload,
        })
    }
}

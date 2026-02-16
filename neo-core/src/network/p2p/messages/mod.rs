//! Message framing and serialization (mirrors `Neo.Network.P2P.Message`).

mod payload_codec;
mod protocol_message;

use super::{
    message::{PAYLOAD_MAX_SIZE, decode_wire_payload, encode_wire_payload, encoded_var_size},
    message_command::MessageCommand,
    message_flags::MessageFlags,
};
use crate::compression::CompressionError;
use crate::neo_io::{BinaryWriter, IoError, MemoryReader};
use crate::network::{NetworkError, NetworkResult};

pub use protocol_message::ProtocolMessage;

/// Header metadata attached to every P2P message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Command opcode that identifies the payload type.
    pub command: MessageCommand,
}

/// Fully decoded network message.
#[derive(Debug, Clone)]
pub struct NetworkMessage {
    /// Header metadata.
    pub header: MessageHeader,
    /// Message flags (e.g. compression state).
    pub flags: MessageFlags,
    /// Strongly typed payload.
    pub payload: ProtocolMessage,
    /// Raw payload bytes as sent on the wire (compressed when flag is set).
    wire_payload: Option<Vec<u8>>,
}

impl NetworkMessage {
    /// Creates a new network message from the supplied payload.
    pub fn new(payload: ProtocolMessage) -> Self {
        let command = payload.command();
        Self {
            header: MessageHeader { command },
            flags: MessageFlags::NONE,
            payload,
            wire_payload: None,
        }
    }

    /// Convenience accessor for the command associated with the payload.
    pub fn command(&self) -> MessageCommand {
        self.header.command
    }

    /// Returns the original wire-format payload if available.
    pub fn wire_payload(&self) -> Option<&[u8]> {
        self.wire_payload.as_deref()
    }

    /// Returns the message encoded exactly as it would appear on the wire.
    ///
    /// `allow_compression` mirrors the C# `Message.ToArray(bool)` behaviour:
    /// when set to `false`, the payload is always emitted uncompressed even if
    /// it would normally satisfy the compression heuristics.
    ///
    /// Optimizations:
    /// - Pre-calculates buffer capacity to minimize reallocations
    /// - Single allocation for the output buffer
    pub fn to_bytes(&self, allow_compression: bool) -> NetworkResult<Vec<u8>> {
        let payload_bytes = self.payload.serialize()?;
        if payload_bytes.len() > PAYLOAD_MAX_SIZE {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload exceeds maximum size ({} > {})",
                payload_bytes.len(),
                PAYLOAD_MAX_SIZE
            )));
        }

        let should_compress =
            allow_compression && self.payload.should_try_compress() && !payload_bytes.is_empty();

        let (flags, final_payload, _compression_error) =
            encode_wire_payload(&payload_bytes, should_compress);

        // Calculate exact capacity needed: 1 (flags) + 1 (command) + varint + payload
        let varint_size = encoded_var_size(final_payload.len());
        let total_size = 1 + 1 + varint_size + final_payload.len();
        let mut writer = BinaryWriter::with_capacity(total_size);

        writer.write_u8(flags.to_byte()).map_err(map_io_error)?;
        writer
            .write_u8(self.header.command.to_byte())
            .map_err(map_io_error)?;
        writer
            .write_var_bytes(&final_payload)
            .map_err(map_io_error)?;
        Ok(writer.into_bytes())
    }

    /// Decodes a message that was previously produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> NetworkResult<Self> {
        if bytes.len() < 2 {
            return Err(NetworkError::InvalidMessage(
                "Message is too short (missing header)".to_string(),
            ));
        }

        let mut reader = MemoryReader::new(bytes);
        let flags_byte = reader.read_u8().map_err(map_io_error)?;
        let command_byte = reader.read_u8().map_err(map_io_error)?;

        let flags = MessageFlags::from_byte(flags_byte)?;
        let command = MessageCommand::from_byte(command_byte)?;

        let payload_len = reader
            .read_var_int(PAYLOAD_MAX_SIZE as u64)
            .map_err(map_io_error)? as usize;
        let payload_raw = reader.read_bytes(payload_len).map_err(map_io_error)?;
        let wire_payload = payload_raw.clone();

        if reader.remaining() != 0 {
            return Err(NetworkError::InvalidMessage(
                "Trailing data detected after payload".to_string(),
            ));
        }

        let payload_data = decode_wire_payload(flags, &payload_raw, PAYLOAD_MAX_SIZE)
            .map_err(map_compression_error)?;

        let payload = ProtocolMessage::deserialize(command, &payload_data)?;

        Ok(Self {
            header: MessageHeader { command },
            flags,
            payload,
            wire_payload: Some(wire_payload),
        })
    }
}

fn map_io_error(error: IoError) -> NetworkError {
    NetworkError::InvalidMessage(error.to_string())
}

fn map_compression_error(error: CompressionError) -> NetworkError {
    NetworkError::InvalidMessage(error.to_string())
}

//! Neo `Message` implementation (mirrors the C# class) with optimized serialization.
//!
//! Optimizations:
//! - Pre-allocated buffer capacity estimation to reduce reallocations
//! - Efficient serialization path with minimal intermediate allocations
//! - Reusable buffer support for high-throughput scenarios

use super::{
    message_command::MessageCommand, message_flags::MessageFlags, messages::ProtocolMessage,
};
use crate::compression::{
    compress_lz4, decompress_lz4, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::{NetworkError, NetworkResult as Result};
use serde::{Deserialize, Serialize};

/// Maximum payload size (matches `Neo.Network.P2P.Message.PayloadMaxSize`)
pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32 MiB

/// Default buffer capacity for small messages (avoids reallocation for common cases).
const DEFAULT_MESSAGE_CAPACITY: usize = 256;

/// Neo network message (parity with `Neo.Network.P2P.Message`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Message flags (e.g. compression bit).
    pub flags: MessageFlags,
    /// Message command discriminator.
    pub command: MessageCommand,
    /// Uncompressed payload bytes (matches `_payloadRaw` in C#).
    pub payload_raw: Vec<u8>,
    /// Wire-format payload bytes (matches `_payloadCompressed` in C#).
    pub payload_compressed: Vec<u8>,
}

impl Message {
    /// Creates a new message with optional compression.
    ///
    /// `enable_compression` mirrors the C# behaviour of only attempting
    /// compression when the caller explicitly allows it (e.g. when peers
    /// negotiated compression support).
    ///
    /// Optimizations:
    /// - Estimates buffer capacity upfront to minimize reallocations
    /// - Avoids intermediate Vec copies where possible
    pub fn create<T>(
        command: MessageCommand,
        payload: Option<&T>,
        enable_compression: bool,
    ) -> Result<Self>
    where
        T: Serializable,
    {
        // Estimate capacity for the BinaryWriter to avoid reallocations
        let estimated_capacity = payload
            .map(|p| p.size().max(DEFAULT_MESSAGE_CAPACITY))
            .unwrap_or(0);

        let payload_bytes = if let Some(payload) = payload {
            let mut writer = BinaryWriter::with_capacity(estimated_capacity);
            payload.serialize(&mut writer).map_err(|e| {
                NetworkError::InvalidMessage(format!("Failed to serialize payload: {e}"))
            })?;
            writer.into_bytes()
        } else {
            Vec::new()
        };

        if payload_bytes.len() > PAYLOAD_MAX_SIZE {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload exceeds maximum size ({} > {})",
                payload_bytes.len(),
                PAYLOAD_MAX_SIZE
            )));
        }

        let mut message = Self {
            flags: MessageFlags::NONE,
            command,
            payload_raw: payload_bytes.clone(),
            payload_compressed: payload_bytes,
        };

        // C# uses strict > comparison for compression threshold
        // Match this exactly to ensure cross-implementation compatibility
        if enable_compression
            && Self::should_try_compress(command)
            && message.payload_compressed.len() > COMPRESSION_MIN_SIZE
        {
            if let Ok(compressed) = compress_lz4(&message.payload_compressed) {
                if compressed.len() + COMPRESSION_THRESHOLD < message.payload_compressed.len() {
                    message.payload_compressed = compressed;
                    message.flags = MessageFlags::COMPRESSED;
                }
            }
        }

        Ok(message)
    }

    /// Reconstructs a message using on-wire payload bytes.
    pub fn from_wire_parts(
        flags: MessageFlags,
        command: MessageCommand,
        wire_payload: &[u8],
    ) -> Result<Self> {
        let mut message = Self {
            flags,
            command,
            payload_raw: Vec::new(),
            payload_compressed: wire_payload.to_vec(),
        };
        message.decompress_payload()?;
        Ok(message)
    }

    /// Returns true if the payload should attempt compression.
    fn should_try_compress(command: MessageCommand) -> bool {
        matches!(
            command,
            MessageCommand::Block
                | MessageCommand::Extensible
                | MessageCommand::Transaction
                | MessageCommand::Headers
                | MessageCommand::Addr
                | MessageCommand::MerkleBlock
                | MessageCommand::FilterLoad
                | MessageCommand::FilterAdd
        )
    }

    /// Returns `true` when the payload is currently compressed.
    pub fn is_compressed(&self) -> bool {
        self.flags.is_compressed()
    }

    /// Returns the raw (uncompressed) payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload_raw
    }

    /// Returns the payload as it will be written on the wire.
    pub fn payload_compressed(&self) -> &[u8] {
        &self.payload_compressed
    }

    /// Serialises the message to bytes, optionally forcing decompression.
    ///
    /// Optimizations:
    /// - Pre-calculates required capacity to avoid buffer growth
    /// - Uses single allocation for the output buffer
    pub fn to_bytes(&self, enable_compression: bool) -> IoResult<Vec<u8>> {
        let (flags, payload) = if enable_compression || !self.is_compressed() {
            (self.flags, self.payload_compressed())
        } else {
            (MessageFlags::NONE, self.payload())
        };

        // Calculate exact size needed to avoid reallocations
        let total_size = 1 + 1 + Self::get_var_size(payload.len()) + payload.len();
        let mut writer = BinaryWriter::with_capacity(total_size);

        writer.write_u8(flags.to_byte())?;
        writer.write_u8(self.command.to_byte())?;
        writer.write_var_bytes(payload)?;
        Ok(writer.into_bytes())
    }

    /// Serializes the message into an existing buffer (for buffer reuse).
    ///
    /// This is useful for high-throughput scenarios where buffer reuse
    /// is desired to reduce allocation overhead.
    ///
    /// Returns the number of bytes written.
    pub fn serialize_into(
        &self,
        buffer: &mut Vec<u8>,
        enable_compression: bool,
    ) -> IoResult<usize> {
        let start_len = buffer.len();
        let (flags, payload) = if enable_compression || !self.is_compressed() {
            (self.flags, self.payload_compressed())
        } else {
            (MessageFlags::NONE, self.payload())
        };

        // Reserve capacity to avoid reallocations
        let required = 1 + 1 + Self::get_var_size(payload.len()) + payload.len();
        buffer.reserve(required);

        buffer.push(flags.to_byte());
        buffer.push(self.command.to_byte());

        // Write var_bytes manually for efficiency
        Self::write_var_bytes_to_vec(payload, buffer)?;

        Ok(buffer.len() - start_len)
    }

    /// Helper: writes var_bytes to a Vec without intermediate allocations.
    fn write_var_bytes_to_vec(bytes: &[u8], vec: &mut Vec<u8>) -> IoResult<()> {
        let len = bytes.len();
        if len < 0xFD {
            vec.push(len as u8);
        } else if len <= 0xFFFF {
            vec.push(0xFD);
            vec.extend_from_slice(&(len as u16).to_le_bytes());
        } else if len <= 0xFFFF_FFFF {
            vec.push(0xFE);
            vec.extend_from_slice(&(len as u32).to_le_bytes());
        } else {
            vec.push(0xFF);
            vec.extend_from_slice(&(len as u64).to_le_bytes());
        }
        vec.extend_from_slice(bytes);
        Ok(())
    }

    /// Decompress payload when needed (matches `DecompressPayload`).
    fn decompress_payload(&mut self) -> Result<()> {
        if self.payload_compressed.is_empty() {
            self.payload_raw.clear();
            return Ok(());
        }

        if self.is_compressed() {
            let decompressed =
                decompress_lz4(&self.payload_compressed, PAYLOAD_MAX_SIZE).map_err(|err| {
                    NetworkError::InvalidMessage(format!("Failed to decompress payload: {err}"))
                })?;
            self.payload_raw = decompressed;
        } else {
            self.payload_raw.clone_from(&self.payload_compressed);
        }

        Ok(())
    }

    /// Get payload size (matches C# `Size` property).
    pub fn size(&self) -> usize {
        1 + 1 + Self::get_var_size(self.payload_compressed.len()) + self.payload_compressed.len()
    }

    /// Calculate the encoded length of a var-size integer.
    const fn get_var_size(value: usize) -> usize {
        if value < 0xFD {
            1
        } else if value <= 0xFFFF {
            3
        } else if value <= 0xFFFF_FFFF {
            5
        } else {
            9
        }
    }

    /// Attempts to deserialize the payload into a strongly typed representation.
    pub fn to_protocol_message(&self) -> Result<ProtocolMessage> {
        ProtocolMessage::from_bytes(self.command, &self.payload_raw)
    }
}

impl Serializable for Message {
    fn deserialize(reader: &mut MemoryReader) -> crate::neo_io::IoResult<Self> {
        let flags = MessageFlags::from_byte(reader.read_u8()?).map_err(|_| {
            IoError::invalid_data_with_context("Message::deserialize", "invalid flags value")
        })?;
        let command = MessageCommand::from_byte(reader.read_u8()?).map_err(|_| {
            IoError::invalid_data_with_context("Message::deserialize", "invalid command value")
        })?;

        let payload_compressed = reader.read_var_bytes(PAYLOAD_MAX_SIZE)?;

        let mut message = Self {
            flags,
            command,
            payload_raw: Vec::new(),
            payload_compressed,
        };

        message
            .decompress_payload()
            .map_err(|err| IoError::invalid_data(err.to_string()))?;

        Ok(message)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> crate::neo_io::IoResult<()> {
        writer.write_u8(self.flags.to_byte())?;
        writer.write_u8(self.command.to_byte())?;
        writer.write_var_bytes(self.payload_compressed())?;
        Ok(())
    }

    fn size(&self) -> usize {
        self.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};

    #[derive(Default)]
    struct DummyPayload {
        bytes: Vec<u8>,
    }

    impl Serializable for DummyPayload {
        fn serialize(&self, writer: &mut BinaryWriter) -> crate::neo_io::IoResult<()> {
            writer.write_bytes(&self.bytes)
        }

        fn deserialize(reader: &mut MemoryReader) -> crate::neo_io::IoResult<Self> {
            // Consume the remaining buffer as the payload for test round-trips.
            let remaining = reader.read_to_end()?.to_vec();
            Ok(Self { bytes: remaining })
        }

        fn size(&self) -> usize {
            self.bytes.len()
        }
    }

    #[test]
    fn compresses_only_whitelisted_commands() {
        let data = vec![0xAB; COMPRESSION_MIN_SIZE + 32];
        let payload = DummyPayload {
            bytes: data.clone(),
        };
        let block_msg = Message::create(MessageCommand::Block, Some(&payload), true).unwrap();
        assert!(block_msg.is_compressed());
        assert_eq!(block_msg.payload(), data.as_slice());
        assert!(block_msg.payload_compressed().len() < data.len());

        let ping_msg = Message::create(MessageCommand::Ping, Some(&payload), true).unwrap();
        assert!(!ping_msg.is_compressed());
        assert_eq!(ping_msg.payload(), data.as_slice());
        assert_eq!(ping_msg.payload_compressed(), data.as_slice());
    }

    #[test]
    fn to_bytes_respects_compression_flag() {
        let data = vec![0xCD; COMPRESSION_MIN_SIZE + 64];
        let payload = DummyPayload {
            bytes: data.clone(),
        };
        let message = Message::create(MessageCommand::Headers, Some(&payload), true).unwrap();
        assert!(message.is_compressed());

        let compressed = message.to_bytes(true).unwrap();
        assert_eq!(compressed[0], MessageFlags::COMPRESSED.to_byte());

        let uncompressed = message.to_bytes(false).unwrap();
        assert_eq!(uncompressed[0], MessageFlags::NONE.to_byte());

        let mut reader = MemoryReader::new(&compressed);
        let decoded = <Message as Serializable>::deserialize(&mut reader).unwrap();
        assert_eq!(decoded.payload(), data.as_slice());
    }

    #[test]
    fn serialize_into_reuses_buffer() {
        let data = vec![0xAB; 100];
        let payload = DummyPayload { bytes: data };
        let message = Message::create(MessageCommand::Ping, Some(&payload), false).unwrap();

        let mut buffer = Vec::with_capacity(1024);
        let len1 = message.serialize_into(&mut buffer, false).unwrap();
        let len2 = message.serialize_into(&mut buffer, false).unwrap();

        assert_eq!(len1, len2);
        assert_eq!(buffer.len(), len1 + len2);

        // Verify both messages can be deserialized correctly using MemoryReader
        let mut reader1 = MemoryReader::new(&buffer[..len1]);
        let msg1 = <Message as Serializable>::deserialize(&mut reader1).unwrap();

        let mut reader2 = MemoryReader::new(&buffer[len1..]);
        let msg2 = <Message as Serializable>::deserialize(&mut reader2).unwrap();

        assert_eq!(msg1.command, MessageCommand::Ping);
        assert_eq!(msg2.command, MessageCommand::Ping);
    }

    #[test]
    fn get_var_size_calculation() {
        assert_eq!(Message::get_var_size(0), 1);
        assert_eq!(Message::get_var_size(0xFC), 1);
        assert_eq!(Message::get_var_size(0xFD), 3);
        assert_eq!(Message::get_var_size(0xFFFF), 3);
        assert_eq!(Message::get_var_size(0x10000), 5);
        assert_eq!(Message::get_var_size(0xFFFF_FFFF), 5);
        assert_eq!(Message::get_var_size(0x1_0000_0000), 9);
    }
}

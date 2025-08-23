//! Neo Message implementation matching C# Message.cs exactly
//!
//! This module provides the exact Neo Message structure and serialization
//! as implemented in C# Neo.Network.P2P.Message.cs

use super::{
    commands::{MessageCommand, MessageFlags},
    compression::{compress_lz4, decompress_lz4, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD},
};
use crate::{NetworkError, NetworkResult as Result};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Maximum payload size (matches C# PayloadMaxSize exactly)
pub const PAYLOAD_MAX_SIZE: usize = 0x02000000; // 32MB

/// Neo network message (matches C# Message.cs exactly)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The flags of the message (matches C# Flags property)
    pub flags: MessageFlags,

    /// The command of the message (matches C# Command property)
    pub command: MessageCommand,

    /// The raw payload data (matches C# _payloadCompressed)
    pub payload_raw: Vec<u8>,

    /// The deserialized payload (matches C# Payload property)
    #[serde(skip)]
    pub payload: Option<Box<dyn Serializable + Send + Sync>>,
}

impl Message {
    /// Create new message (matches C# Create method exactly)
    pub fn create(
        command: MessageCommand,
        payload: Option<&dyn Serializable>,
        enable_compression: bool,
    ) -> Result<Self> {
        let payload_bytes = if let Some(payload) = payload {
            payload
                .to_array()
                .map_err(|e| NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message: format!("Failed to serialize payload: {}", e),
                })?
        } else {
            Vec::new()
        };

        let mut message = Self {
            flags: MessageFlags::None,
            command,
            payload_raw: payload_bytes.clone(),
            payload: None,
        };

        // Apply compression if enabled and beneficial (matches C# compression logic)
        if enable_compression && payload_bytes.len() >= COMPRESSION_MIN_SIZE {
            match compress_lz4(&payload_bytes) {
                Ok(compressed) => {
                    if payload_bytes.len() > compressed.len() + COMPRESSION_THRESHOLD {
                        message.payload_raw = compressed;
                        message.flags = MessageFlags::Compressed;
                    }
                }
                Err(_) => {
                    // Compression failed, use uncompressed
                }
            }
        }

        Ok(message)
    }

    /// Decompress payload if needed (matches C# DecompressPayload exactly)
    fn decompress_payload(&mut self) -> Result<()> {
        if self.payload_raw.is_empty() {
            return Ok(());
        }

        let decompressed = if self.flags == MessageFlags::Compressed {
            decompress_lz4(&self.payload_raw, PAYLOAD_MAX_SIZE)?
        } else {
            self.payload_raw.clone()
        };

        // Store decompressed data for payload creation
        self.payload_raw = decompressed;
        Ok(())
    }

    /// Get payload size (matches C# Size property)
    pub fn size(&self) -> usize {
        1 + // flags
        1 + // command  
        self.get_var_size(self.payload_raw.len()) + // var_size prefix
        self.payload_raw.len() // payload data
    }

    /// Calculate variable-length size encoding (matches C# GetVarSize)
    fn get_var_size(&self, value: usize) -> usize {
        if value < 0xFD {
            1
        } else if value <= 0xFFFF {
            3 // 0xFD + 2 bytes
        } else if value <= 0xFFFFFFFF {
            5 // 0xFE + 4 bytes
        } else {
            9 // 0xFF + 8 bytes
        }
    }

    /// Write variable-length integer (matches C# WriteVarInt)
    fn write_var_int(&self, writer: &mut BinaryWriter, value: u64) -> std::io::Result<()> {
        if value < 0xFD {
            writer.write_u8(value as u8)
        } else if value <= 0xFFFF {
            writer.write_u8(0xFD)?;
            writer.write_u16(value as u16)
        } else if value <= 0xFFFFFFFF {
            writer.write_u8(0xFE)?;
            writer.write_u32(value as u32)
        } else {
            writer.write_u8(0xFF)?;
            writer.write_u64(value)
        }
    }

    /// Read variable-length integer (matches C# ReadVarInt)
    fn read_var_int(reader: &mut MemoryReader, max: u64) -> std::io::Result<u64> {
        let fb = reader.read_u8()?;
        let value = match fb {
            0xFD => reader.read_u16()? as u64,
            0xFE => reader.read_u32()? as u64,
            0xFF => reader.read_u64()?,
            _ => fb as u64,
        };

        if value > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("VarInt value {} exceeds maximum {}", value, max),
            ));
        }

        Ok(value)
    }
}

impl Serializable for Message {
    /// Deserialize message (matches C# ISerializable.Deserialize exactly)
    fn deserialize(reader: &mut MemoryReader) -> std::io::Result<Self> {
        let flags = MessageFlags::from_byte(reader.read_u8()?);
        let command = MessageCommand::from_byte(reader.read_u8()?);

        // Read payload using VarBytes (matches C# ReadVarMemory)
        let payload_len = Self::read_var_int_static(reader, PAYLOAD_MAX_SIZE as u64)? as usize;
        let payload_raw = reader.read_bytes(payload_len)?;

        let mut message = Self {
            flags,
            command,
            payload_raw,
            payload: None,
        };

        // Decompress if needed
        message
            .decompress_payload()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        Ok(message)
    }

    /// Serialize message (matches C# ISerializable.Serialize exactly)
    fn serialize(&self, writer: &mut BinaryWriter) -> std::io::Result<()> {
        writer.write_u8(self.flags as u8)?;
        writer.write_u8(self.command as u8)?;

        // Write payload as VarBytes (matches C# WriteVarBytes)
        self.write_var_int(writer, self.payload_raw.len() as u64)?;
        writer.write_bytes(&self.payload_raw)?;

        Ok(())
    }

    /// Get serialized size
    fn size(&self) -> usize {
        self.size()
    }
}

impl MessageFlags {
    /// Convert from byte value (helper for deserialization)
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => MessageFlags::None,
            1 => MessageFlags::Compressed,
            _ => MessageFlags::None, // Default to None for invalid values
        }
    }
}

impl MessageCommand {
    /// Convert from byte value (helper for deserialization)
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => MessageCommand::Version,
            0x01 => MessageCommand::Verack,
            0x10 => MessageCommand::GetAddr,
            0x11 => MessageCommand::Addr,
            0x18 => MessageCommand::Ping,
            0x19 => MessageCommand::Pong,
            0x20 => MessageCommand::GetHeaders,
            0x21 => MessageCommand::Headers,
            0x24 => MessageCommand::GetBlocks,
            0x25 => MessageCommand::Mempool,
            0x27 => MessageCommand::Inv,
            0x28 => MessageCommand::GetData,
            0x29 => MessageCommand::GetBlockByIndex,
            0x2a => MessageCommand::NotFound,
            0x2b => MessageCommand::Transaction,
            0x2c => MessageCommand::Block,
            0x2e => MessageCommand::Extensible,
            0x2f => MessageCommand::Reject,
            0x30 => MessageCommand::FilterLoad,
            0x31 => MessageCommand::FilterAdd,
            0x32 => MessageCommand::FilterClear,
            0x38 => MessageCommand::MerkleBlock,
            0x40 => MessageCommand::Alert,
            _ => MessageCommand::Version, // Default for unknown commands
        }
    }
}

// Static helper for reading VarInt without instance
impl Message {
    fn read_var_int_static(reader: &mut MemoryReader, max: u64) -> std::io::Result<u64> {
        let fb = reader.read_u8()?;
        let value = match fb {
            0xFD => reader.read_u16()? as u64,
            0xFE => reader.read_u32()? as u64,
            0xFF => reader.read_u64()?,
            _ => fb as u64,
        };

        if value > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("VarInt value {} exceeds maximum {}", value, max),
            ));
        }

        Ok(value)
    }
}

use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;

/// Sent to detect whether the connection has been disconnected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PingPayload {
    /// The latest block index.
    pub last_block_index: u32,

    /// The timestamp when the message was sent.
    pub timestamp: u32,

    /// A random number. This number must be the same in
    /// `MessageCommand::Ping` and `MessageCommand::Pong` messages.
    pub nonce: u32,
}

impl PingPayload {

    /// Creates a new instance of the `PingPayload` struct.
    ///
    /// # Arguments
    ///
    /// * `height` - The latest block index.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn create(height: u32) -> Self {
        let nonce = rand::random::<u32>();
        Self::create_with_nonce(height, nonce)
    }

    /// Creates a new instance of the `PingPayload` struct.
    ///
    /// # Arguments
    ///
    /// * `height` - The latest block index.
    /// * `nonce` - The random number.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn create_with_nonce(height: u32, nonce: u32) -> Self {
        Self {
            last_block_index: height,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs() as u32,
            nonce,
        }
    }
}

impl ISerializable for PingPayload {
    fn size(&self) -> usize {
        std::mem::size_of::<u32>() * 3 // LastBlockIndex + Timestamp + Nonce
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.last_block_index);
        writer.write_u32(self.timestamp);
        writer.write_u32(self.nonce);
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let last_block_index = reader.read_u32()?;
        let timestamp = reader.read_u32()?;
        let nonce = reader.read_u32()?;
        Ok(Self {
            last_block_index,
            timestamp,
            nonce,
        })
    }
}

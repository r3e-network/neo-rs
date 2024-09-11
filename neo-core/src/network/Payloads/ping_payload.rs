use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use byteorder::LittleEndian;
use crate::io::iserializable::ISerializable;

/// Sent to detect whether the connection has been disconnected.
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
    pub fn size(&self) -> usize {
        std::mem::size_of::<u32>() * 3 // LastBlockIndex + Timestamp + Nonce
    }

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
        todo!()
    }

    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_u32::<LittleEndian>(self.last_block_index)?;
        writer.write_u32::<LittleEndian>(self.timestamp)?;
        writer.write_u32::<LittleEndian>(self.nonce)?;
        Ok(())
    }

    fn deserialize<R: Read>(&mut self, reader: &mut R) -> std::io::Result<()> {
        self.last_block_index = reader.read_u32::<LittleEndian>()?;
        self.timestamp = reader.read_u32::<LittleEndian>()?;
        self.nonce = reader.read_u32::<LittleEndian>()?;
        Ok(())
    }
}

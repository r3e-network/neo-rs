use std::io::{Error, ErrorKind};
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::network::Payloads::NetworkAddressWithTime;

/// This message is sent to respond to `MessageCommand::GetAddr` messages.
pub struct AddrPayload {
    /// The list of nodes.
    pub address_list: Vec<NetworkAddressWithTime>,
}

impl AddrPayload {
    /// Indicates the maximum number of nodes sent each time.
    pub const MAX_COUNT_TO_SEND: usize = 200;

    /// Creates a new instance of the `AddrPayload` struct.
    ///
    /// # Arguments
    ///
    /// * `addresses` - The list of nodes.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn new(addresses: Vec<NetworkAddressWithTime>) -> Self {
        AddrPayload {
            address_list: addresses,
        }
    }

    /// Returns the size of the payload.
    pub fn size(&self) -> usize {
        // Assuming NetworkAddressWithTime implements Serializable
        self.address_list.iter().map(|addr| addr.size()).sum()
    }
}

impl ISerializable for AddrPayload {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), Error> {
        writer.write_serializable_list(&self.address_list)
    }

    fn deserialize(&mut self, reader: &mut BinaryReader) -> Result<(), Error> {
        self.address_list = reader.read_serializable_list(Self::MAX_COUNT_TO_SEND)?;
        if self.address_list.is_empty() {
            return Err(Error::new(ErrorKind::InvalidData, "Empty address list"));
        }
        Ok(())
    }
}

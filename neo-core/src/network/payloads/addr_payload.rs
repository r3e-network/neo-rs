use std::io::{Error, ErrorKind};
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::network::payloads::NetworkAddressWithTime;

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

}

impl ISerializable for AddrPayload {
    fn size(&self) -> usize {
        self.address_list.iter().map(|addr| addr.size()).sum()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_serializable_list(&self.address_list)
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let address_list = reader.read_serializable_list(Self::MAX_COUNT_TO_SEND)?;
        if address_list.is_empty() {
            return Err(Error::new(ErrorKind::InvalidData, "Empty address list"));
        }
        Ok(AddrPayload { address_list })
    }

}

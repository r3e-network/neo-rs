use super::network_address_with_time::NetworkAddressWithTime;
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates the maximum number of nodes sent each time.
pub const MAX_COUNT_TO_SEND: usize = 200;

/// This message is sent to respond to GetAddr messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddrPayload {
    /// The list of nodes.
    pub address_list: Vec<NetworkAddressWithTime>,
}

impl AddrPayload {
    /// Creates a new instance of the AddrPayload class.
    pub fn create(addresses: Vec<NetworkAddressWithTime>) -> Self {
        Self {
            address_list: addresses,
        }
    }
}

impl Serializable for AddrPayload {
    fn size(&self) -> usize {
        SerializeHelper::get_var_size_serializable_slice(&self.address_list)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.address_list.len() > MAX_COUNT_TO_SEND {
            return Err(IoError::invalid_data("Too many addresses"));
        }

        SerializeHelper::serialize_array(&self.address_list, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let address_list = SerializeHelper::deserialize_array(reader, MAX_COUNT_TO_SEND)?;
        if address_list.is_empty() {
            return Err(IoError::invalid_data("Empty address list"));
        }

        Ok(Self { address_list })
    }
}

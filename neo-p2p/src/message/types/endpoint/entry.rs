use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::NetworkAddress;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressEntry {
    pub timestamp: u32,
    pub address: NetworkAddress,
}

impl AddressEntry {
    pub fn new(timestamp: u32, address: NetworkAddress) -> Self {
        Self { timestamp, address }
    }
}

impl NeoEncode for AddressEntry {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.timestamp.neo_encode(writer);
        self.address.neo_encode(writer);
    }
}

impl NeoDecode for AddressEntry {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            timestamp: u32::neo_decode(reader)?,
            address: NetworkAddress::neo_decode(reader)?,
        })
    }
}

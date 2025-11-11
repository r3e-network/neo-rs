use std::vec::Vec;

use neo_base::encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite,
};

use super::{AddressEntry, MAX_ADDRESSES};

#[cfg(test)]
use super::{Endpoint, NetworkAddress};
#[cfg(test)]
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressPayload {
    pub entries: Vec<AddressEntry>,
}

impl AddressPayload {
    pub fn new(entries: Vec<AddressEntry>) -> Self {
        Self { entries }
    }
}

impl NeoEncode for AddressPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let count = self.entries.len() as u64;
        debug_assert!(
            (1..=MAX_ADDRESSES).contains(&count),
            "address payload entry count out of range"
        );
        write_varint(writer, count);
        for entry in &self.entries {
            entry.neo_encode(writer);
        }
    }
}

impl NeoDecode for AddressPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let count = read_varint(reader)?;
        if count == 0 || count > MAX_ADDRESSES {
            return Err(DecodeError::LengthOutOfRange {
                len: count,
                max: MAX_ADDRESSES,
            });
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(AddressEntry::neo_decode(reader)?);
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::encoding::{write_varint, SliceReader};

    #[test]
    fn address_payload_rejects_over_limit() {
        let mut bytes = Vec::new();
        let count = (MAX_ADDRESSES + 1) as u64;
        write_varint(&mut bytes, count);
        for idx in 0..count as usize {
            let endpoint =
                Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2000 + (idx as u16 % 100));
            let entry = AddressEntry::new(idx as u32, NetworkAddress::new(1, endpoint));
            entry.neo_encode(&mut bytes);
        }

        let mut reader = SliceReader::new(bytes.as_slice());
        let err = AddressPayload::neo_decode(&mut reader).unwrap_err();
        assert!(matches!(err, DecodeError::LengthOutOfRange { .. }));
    }
}

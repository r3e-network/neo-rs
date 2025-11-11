use neo_base::encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite,
};

use super::{InventoryItem, MAX_ITEMS};

#[cfg(test)]
use super::InventoryKind;
#[cfg(test)]
use neo_base::hash::Hash256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryPayload {
    pub items: Vec<InventoryItem>,
}

impl InventoryPayload {
    pub fn new(items: Vec<InventoryItem>) -> Self {
        Self { items }
    }
}

impl NeoEncode for InventoryPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let count = self.items.len() as u64;
        debug_assert!(
            count <= MAX_ITEMS,
            "inventory payload entry count out of range"
        );
        write_varint(writer, count);
        for item in &self.items {
            item.neo_encode(writer);
        }
    }
}

impl NeoDecode for InventoryPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let count = read_varint(reader)?;
        if count > MAX_ITEMS {
            return Err(DecodeError::LengthOutOfRange {
                len: count,
                max: MAX_ITEMS,
            });
        }
        let mut items = Vec::with_capacity(count as usize);
        for _ in 0..count {
            items.push(InventoryItem::neo_decode(reader)?);
        }
        Ok(Self { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::encoding::{write_varint, SliceReader};

    #[test]
    fn inventory_payload_rejects_over_limit() {
        let mut bytes = Vec::new();
        let count = (MAX_ITEMS + 1) as u64;
        write_varint(&mut bytes, count);
        for i in 0..count as usize {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0] = (i & 0xFF) as u8;
            let item = InventoryItem {
                kind: InventoryKind::Block,
                hash: Hash256::new(hash_bytes),
            };
            item.neo_encode(&mut bytes);
        }

        let mut reader = SliceReader::new(bytes.as_slice());
        let err = InventoryPayload::neo_decode(&mut reader).unwrap_err();
        assert!(matches!(err, DecodeError::LengthOutOfRange { .. }));
    }
}

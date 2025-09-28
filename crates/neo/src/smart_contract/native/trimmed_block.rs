//! TrimmedBlock - matches C# Neo.SmartContract.Native.TrimmedBlock exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::{UInt160, UInt256};
use neo_vm::StackItem;
use num_traits::ToPrimitive;

/// A trimmed block containing only header and transaction hashes (matches C# TrimmedBlock)
#[derive(Clone, Debug)]
pub struct TrimmedBlock {
    /// The block hash
    pub hash: UInt256,
    
    /// The block index
    pub index: u32,
    
    /// The block timestamp
    pub timestamp: u64,
    
    /// The previous block hash
    pub prev_hash: UInt256,
    
    /// The next consensus data
    pub next_consensus: UInt160,
    
    /// The witness index
    pub witness: u16,
    
    /// The transaction count
    pub transaction_count: u32,
    
    /// The transaction hashes
    pub hashes: Vec<UInt256>,
}

impl TrimmedBlock {
    /// Creates a new trimmed block
    pub fn new(
        hash: UInt256,
        index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        next_consensus: UInt160,
        witness: u16,
        hashes: Vec<UInt256>,
    ) -> Self {
        Self {
            hash,
            index,
            timestamp,
            prev_hash,
            next_consensus,
            witness,
            transaction_count: hashes.len() as u32,
            hashes,
        }
    }
}

impl IInteroperable for TrimmedBlock {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 7 {
                return;
            }

            if let Ok(bytes) = items[0].as_bytes() {
                if bytes.len() == 32 {
                    self.hash = UInt256::from_bytes(&bytes);
                }
            }

            if let Ok(integer) = items[1].as_int() {
                if let Some(idx) = integer.to_u32() {
                    self.index = idx;
                }
            }

            if let Ok(integer) = items[2].as_int() {
                if let Some(ts) = integer.to_u64() {
                    self.timestamp = ts;
                }
            }

            if let Ok(bytes) = items[3].as_bytes() {
                if bytes.len() == 32 {
                    self.prev_hash = UInt256::from_bytes(&bytes);
                }
            }

            if let Ok(bytes) = items[4].as_bytes() {
                if bytes.len() == 20 {
                    self.next_consensus = UInt160::from_bytes(&bytes);
                }
            }

            if let Ok(integer) = items[5].as_int() {
                if let Some(witness) = integer.to_u16() {
                    self.witness = witness;
                }
            }

            if let Ok(hash_items) = items[6].as_array() {
                self.hashes = hash_items
                    .iter()
                    .filter_map(|item| {
                        item.as_bytes().ok().and_then(|bytes| {
                            if bytes.len() == 32 {
                                Some(UInt256::from_bytes(&bytes))
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                self.transaction_count = self.hashes.len() as u32;
            }
        }
    }
    
    fn to_stack_item(&self) -> StackItem {
        let hashes = self
            .hashes
            .iter()
            .map(|hash| StackItem::from_byte_string(hash.to_bytes()))
            .collect::<Vec<_>>();

        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.hash.to_bytes()),
            StackItem::from_int(self.index),
            StackItem::from_int(self.timestamp),
            StackItem::from_byte_string(self.prev_hash.to_bytes()),
            StackItem::from_byte_string(self.next_consensus.to_bytes()),
            StackItem::from_int(self.witness),
            StackItem::from_array(hashes),
        ])
    }
    
    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

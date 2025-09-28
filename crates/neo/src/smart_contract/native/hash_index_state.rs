//! HashIndexState - matches C# Neo.SmartContract.Native.HashIndexState exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::UInt256;
use neo_vm::StackItem;
use num_traits::ToPrimitive;

/// State for hash index tracking (matches C# HashIndexState)
#[derive(Clone, Debug)]
pub struct HashIndexState {
    /// The hash value
    pub hash: UInt256,
    
    /// The index value
    pub index: u32,
}

impl HashIndexState {
    /// Creates a new hash index state
    pub fn new(hash: UInt256, index: u32) -> Self {
        Self { hash, index }
    }
    
    /// Creates default state
    pub fn default() -> Self {
        Self {
            hash: UInt256::default(),
            index: 0,
        }
    }
}

impl IInteroperable for HashIndexState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 2 {
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
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.hash.to_bytes()),
            StackItem::from_int(self.index),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}


use neo_sdk::prelude::*;
use neo_sdk::types::UInt256;
use neo_sdk::vm::types::{StackItem, Struct};

/// Represents a state that combines a hash and an index.
#[derive(Default)]
pub struct HashIndexState {
    pub hash: UInt256,
    pub index: u32,
}

impl IInteroperable for HashIndexState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), String> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err("Invalid struct length for HashIndexState".into());
            }
            self.hash = UInt256::from_slice(&s[0].as_bytes()?)?;
            self.index = s[1].as_u32()?;
            Ok(())
        } else {
            Err("Expected Struct for HashIndexState".into())
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::Struct(vec![
            StackItem::ByteString(self.hash.to_vec()),
            StackItem::Integer(self.index.into()),
        ])
    }
}

//! HashIndexState - matches C# Neo.SmartContract.Native.HashIndexState exactly

use crate::error::CoreError;
use crate::UInt256;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// State for hash index tracking (matches C# HashIndexState)
#[derive(Clone, Debug, Default)]
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

    fn stack_value_to_u32(value: &StackValue) -> Option<u32> {
        match value {
            StackValue::Integer(value) => u32::try_from(*value).ok(),
            StackValue::Boolean(value) => Some(u32::from(*value)),
            StackValue::BigInteger(bytes) => BigInt::from_signed_bytes_le(bytes).to_u32(),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
                BigInt::from_signed_bytes_le(bytes).to_u32()
            }
            _ => None,
        }
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::ByteString(self.hash.to_bytes()),
            StackValue::Integer(i64::from(self.index)),
        ])
    }

    /// Updates this hash/index state from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if items.len() < 2 {
                return Ok(());
            }

            if let Some(bytes) = items[0].to_byte_string_bytes() {
                if bytes.len() == 32 {
                    if let Ok(hash) = UInt256::from_bytes(&bytes) {
                        self.hash = hash;
                    }
                }
            }

            if let Some(index) = Self::stack_value_to_u32(&items[1]) {
                self.index = index;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    #[test]
    fn hash_index_state_projects_to_neo_vm_rs_stack_value() {
        let state = HashIndexState::new(UInt256::from_bytes(&[7u8; 32]).unwrap(), 42);

        assert_eq!(
            state.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::ByteString(vec![7u8; 32]),
                StackValue::Integer(42),
            ])
        );
    }

    #[test]
    fn hash_index_state_reads_from_neo_vm_rs_stack_value() {
        let mut state = HashIndexState::default();

        state
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(vec![9u8; 32]),
                StackValue::Integer(99),
            ]))
            .unwrap();

        assert_eq!(state.hash, UInt256::from_bytes(&[9u8; 32]).unwrap());
        assert_eq!(state.index, 99);
    }
}

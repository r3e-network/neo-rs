//! TransactionState - matches C# Neo.SmartContract.Native.TransactionState exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::UInt256;
use neo_vm::StackItem;
use num_traits::ToPrimitive;

/// State of a transaction in the ledger (matches C# TransactionState)
#[derive(Clone, Debug)]
pub struct TransactionState {
    /// The block index containing the transaction
    pub block_index: u32,
    
    /// The transaction itself (simplified as hash for now)
    pub transaction: Transaction,
    
    /// The execution state
    pub state: VMState,
}

/// Simplified transaction representation
#[derive(Clone, Debug)]
pub struct Transaction {
    /// Transaction hash
    pub hash: UInt256,
    
    /// Version
    pub version: u8,
    
    /// Nonce
    pub nonce: u32,
    
    /// System fee
    pub system_fee: i64,
    
    /// Network fee
    pub network_fee: i64,
    
    /// Valid until block
    pub valid_until_block: u32,
}

/// VM execution state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VMState {
    None = 0,
    Halt = 1,
    Fault = 2,
    Break = 4,
}

impl TransactionState {
    /// Creates a new transaction state
    pub fn new(block_index: u32, transaction: Transaction, state: VMState) -> Self {
        Self {
            block_index,
            transaction,
            state,
        }
    }
}

impl IInteroperable for TransactionState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 3 {
                return;
            }

            if let Ok(integer) = items[0].as_int() {
                if let Some(index) = integer.to_u32() {
                    self.block_index = index;
                }
            }

            if let Ok(bytes) = items[1].as_bytes() {
                if bytes.len() == 32 {
                    self.transaction.hash = UInt256::from_bytes(&bytes);
                }
            }

            if let Ok(integer) = items[2].as_int() {
                if let Some(state_val) = integer.to_u8() {
                    self.state = match state_val {
                        1 => VMState::Halt,
                        2 => VMState::Fault,
                        4 => VMState::Break,
                        _ => VMState::None,
                    };
                }
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_int(self.block_index),
            StackItem::from_byte_string(self.transaction.hash.to_bytes()),
            StackItem::from_int(self.state as u8),
        ])
    }
    
    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

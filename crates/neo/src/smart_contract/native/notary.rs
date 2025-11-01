//! Notary - matches C# Neo.SmartContract.Native.Notary exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Notary deposit state (matches C# DepositState in Notary)
#[derive(Clone, Debug)]
pub struct DepositState {
    /// The amount deposited
    pub amount: BigInt,
    
    /// The block height until which the deposit is valid
    pub till: u32,
}

impl DepositState {
    /// Creates a new deposit state
    pub fn new(amount: BigInt, till: u32) -> Self {
        Self { amount, till }
    }
}

impl IInteroperable for DepositState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 2 {
                return;
            }

            if let Ok(integer) = items[0].as_int() {
                self.amount = integer;
            }

            if let Ok(integer) = items[1].as_int() {
                if let Some(till) = integer.to_u32() {
                    self.till = till;
                }
            }
        }
    }
    
    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_int(self.amount.clone()),
            StackItem::from_int(self.till),
        ])
    }
    
    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

/// Notary service configuration
pub struct NotaryConfig {
    /// Maximum number of keys per request
    pub max_notary_keys: u32,
    
    /// Deposit required per key
    pub deposit_per_key: BigInt,
    
    /// Maximum valid until delta
    pub max_valid_until_delta: u32,
}

impl Default for NotaryConfig {
    fn default() -> Self {
        Self {
            max_notary_keys: 16,
            deposit_per_key: BigInt::from(1000000000i64), // 10 GAS
            max_valid_until_delta: 5760, // ~24 hours
        }
    }
}

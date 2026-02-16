//! AccountState - matches C# Neo.SmartContract.Native.AccountState exactly

use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::stack_item_extract::extract_int;
use neo_vm::StackItem;
use num_bigint::BigInt;

/// Base account state for native tokens (matches C# AccountState)
#[derive(Clone, Debug, Default)]
pub struct AccountState {
    /// The balance of the account
    pub balance: BigInt,
}

impl AccountState {
    /// Creates a new account state
    pub fn new() -> Self {
        Self {
            balance: BigInt::from(0),
        }
    }

    /// Creates with initial balance
    pub fn with_balance(balance: BigInt) -> Self {
        Self { balance }
    }

    /// Adds to balance
    pub fn add_balance(&mut self, amount: &BigInt) -> Result<(), String> {
        if amount.sign() == num_bigint::Sign::Minus {
            return Err("Cannot add negative amount".to_string());
        }
        self.balance += amount;
        Ok(())
    }

    /// Subtracts from balance
    pub fn subtract_balance(&mut self, amount: &BigInt) -> Result<(), String> {
        if amount.sign() == num_bigint::Sign::Minus {
            return Err("Cannot subtract negative amount".to_string());
        }
        if &self.balance < amount {
            return Err("Insufficient balance".to_string());
        }
        self.balance -= amount;
        Ok(())
    }
}

impl IInteroperable for AccountState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let StackItem::Struct(struct_item) = stack_item else {
            return Ok(());
        };
        let items = struct_item.items();
        if let Some(balance) = items.first().and_then(extract_int) {
            self.balance = balance;
        }
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        Ok(StackItem::from_struct(vec![StackItem::from_int(
            self.balance.clone(),
        )]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

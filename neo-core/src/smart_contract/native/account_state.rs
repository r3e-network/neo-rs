//! AccountState - matches C# Neo.SmartContract.Native.AccountState exactly

use crate::error::CoreError;
use neo_vm_rs::StackValue;
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

    fn stack_value_to_bigint(value: &StackValue) -> Option<BigInt> {
        match value {
            StackValue::Integer(value) => Some(BigInt::from(*value)),
            StackValue::Boolean(value) => Some(BigInt::from(i32::from(*value))),
            StackValue::BigInteger(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
                Some(BigInt::from_signed_bytes_le(bytes))
            }
            _ => None,
        }
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![StackValue::BigInteger(
            self.balance.to_signed_bytes_le(),
        )])
    }

    /// Updates this account state from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if let Some(first) = items.first() {
                if let Some(balance) = Self::stack_value_to_bigint(first) {
                    self.balance = balance;
                }
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
    fn account_state_projects_to_neo_vm_rs_stack_value() {
        let state = AccountState::with_balance(BigInt::from(1_000_000_000i64));

        assert_eq!(
            state.to_stack_value(),
            StackValue::Struct(vec![StackValue::BigInteger(
                BigInt::from(1_000_000_000i64).to_signed_bytes_le()
            )])
        );
    }

    #[test]
    fn account_state_reads_from_neo_vm_rs_stack_value() {
        let mut state = AccountState::new();
        let balance = BigInt::from(987_654_321i64);

        state
            .from_stack_value(StackValue::Struct(vec![StackValue::BigInteger(
                balance.to_signed_bytes_le(),
            )]))
            .unwrap();

        assert_eq!(state.balance, balance);
    }
}

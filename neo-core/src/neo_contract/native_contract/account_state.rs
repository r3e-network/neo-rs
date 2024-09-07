
use num_bigint::{BigInt, BigUint};
use num_traits::One;

/// The base struct of account state for all native tokens.
#[derive(Default)]
pub struct AccountState {
    /// The balance of the account.
    pub balance: BigInt,
}

impl Interoperable for AccountState {
    fn from_stack_item(&mut self, stack_item: &StackItem) -> Result<(), Box<dyn std::error::Error>> {
        if let StackItem::Struct(s) = stack_item {
            self.balance = s[0].try_into()?;
            Ok(())
        } else {
            Err("Expected Struct StackItem".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &ReferenceCounter) -> Result<StackItem, Box<dyn std::error::Error>> {
        Ok(StackItem::Struct(Struct::new(vec![self.balance.clone().into()], reference_counter)))
    }
}

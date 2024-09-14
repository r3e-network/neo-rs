use alloc::rc::Rc;
use num_bigint::{BigInt, BigUint};
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;

pub trait AccountStateTrait: Default {
    fn balance(&self) -> BigInt;
    fn set_balance(&mut self, balance: BigInt);
}

/// The base struct of account state for all native tokens.
#[derive(Default)]
pub struct AccountState {
    /// The balance of the account.
    pub balance: BigInt,
}

impl AccountStateTrait for AccountState{
    fn balance(&self) -> BigInt {
        self.balance.clone()
    }

    fn set_balance(&mut self, balance: BigInt) {
        self.balance = balance;
    }
}

impl IInteroperable for AccountState {
    type Error = std::io::Error;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<(), Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            self.balance = s[0].try_into()?;
            Ok(())
        } else {
            Err("Expected Struct StackItem".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &ReferenceCounter) -> Result<StackItem, Self::Error> {
        Ok(StackItem::new_struct(reference_counter, vec![self.balance.clone().into()]))
    }
}

//! Complete port of `Neo.SmartContract.IInteroperableVerifiable`.

use crate::smart_contract::i_interoperable::{IInteroperable, SmartContractStackItem};

/// Extends [`IInteroperable`] with the ability to request verified conversions.
#[allow(clippy::wrong_self_convention)]
pub trait IInteroperableVerifiable: IInteroperable {
    /// Convert a [`StackItem`](SmartContractStackItem) to the current object with optional
    /// verification of the payload contents.
    fn from_stack_item_verifiable(&mut self, stack_item: SmartContractStackItem, verify: bool);

    /// Convenience helper aligning with the default managed behaviour (`verify = true`).
    fn from_stack_item_verified_default(&mut self, stack_item: SmartContractStackItem) {
        self.from_stack_item_verifiable(stack_item, true);
    }
}

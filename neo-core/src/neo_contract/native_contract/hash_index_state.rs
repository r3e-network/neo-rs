use alloc::rc::Rc;
use neo_vm::References;
use neo_vm::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::native_contract::native_contract_error::NativeContractError;
use neo_type::H256;

/// Represents a state that combines a hash and an index.
#[derive(Default)]
pub struct HashIndexState {
    pub hash: H256,
    pub index: u32,
}

impl IInteroperable for HashIndexState {
    type Error = NativeContractError;

    fn from_stack_item( stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err("Invalid struct length for HashIndexState".into());
            }
            self.hash = H256::from_slice(&s[0].as_bytes()?)?;
            self.index = s[1].as_u32()?;
            Ok(())
        } else {
            Err("Expected Struct for HashIndexState".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &mut References) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Struct(vec![
            StackItem::ByteString(self.hash.to_vec()),
            StackItem::Integer(self.index.into()),
        ]))
    }
}

use alloc::rc::Rc;
use NeoRust::neo_types::VMState;
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::native_contract::native_contract_error::NativeContractError;
use crate::network::payloads::Transaction;

/// Represents a transaction that has been included in a block.
#[derive(Clone)]
pub struct TransactionState {
    /// The block containing this transaction.
    pub block_index: u32,

    /// The transaction, if the transaction is trimmed this value will be None
    pub transaction: Option<Transaction>,

    /// The execution state
    pub state: VMState,

    raw_transaction: Vec<u8>,
}

impl Default for TransactionState {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for TransactionState {
    type Error = NativeContractError;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(struct_item) = stack_item {
            // Conflict record.
            if struct_item.len() == 1 {
                return Ok(Self {
                    block_index: struct_item[0].as_u32()?,
                    transaction: None,
                    state: VMState::from_u8(struct_item[2].as_u8()?)?,
                    raw_transaction: vec![],
                });
            }

            // Fully-qualified transaction.
            let raw_transaction = struct_item[1].as_bytes()?.to_vec();
                let transaction = Some(Transaction::deserialize(&raw_transaction, &mut reference_counter)?);
            let state = VMState::from_u8(struct_item[2].as_u8()?)?;
            Ok(Self {
                block_index: struct_item[0].as_u32()?,
                transaction,
                state,
                raw_transaction,
            })
        } else {
            Err(Error::InvalidStackItemType)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        if self.transaction.is_none() {
            return StackItem::Struct(Struct::new(vec![StackItem::Integer(self.block_index.into())], reference_counter));
        }
        if self.raw_transaction.is_empty() {
            self.raw_transaction = self.transaction.as_ref().unwrap().serialize();
        }
        StackItem::Struct(Struct::new(
            vec![
                StackItem::Integer(self.block_index.into()),
                StackItem::ByteString(ByteString::from(self.raw_transaction.clone())),
                StackItem::Integer((self.state as u8).into()),
            ],
            reference_counter,
        ))
    }

    fn clone(&self) -> Box<dyn IInteroperable> {
        Box::new(Self {
            block_index: self.block_index,
            transaction: self.transaction.clone(),
            state: self.state,
            raw_transaction: self.raw_transaction.clone(),
        })
    }

    fn from_replica(&mut self, replica: &dyn IInteroperable) {
        let from = replica.downcast_ref::<TransactionState>().unwrap();
        self.block_index = from.block_index;
        self.transaction = from.transaction.clone();
        self.state = from.state;
        if self.raw_transaction.is_empty() {
            self.raw_transaction = from.raw_transaction.clone();
        }
    }
}

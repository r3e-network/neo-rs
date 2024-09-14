use alloc::rc::Rc;
use serde::{Deserialize, Serialize};
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::native_contract::native_contract_error::NativeContractError;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents an Oracle request in smart contracts.
#[derive(Serialize, Deserialize)]
pub struct OracleRequest {
    /// The original transaction that sent the related request.
    pub original_txid: UInt256,

    /// The maximum amount of GAS that can be used when executing response callback.
    pub gas_for_response: i64,

    /// The url of the request.
    pub url: String,

    /// The filter for the response.
    pub filter: Option<String>,

    /// The hash of the callback contract.
    pub callback_contract: UInt160,

    /// The name of the callback method.
    pub callback_method: String,

    /// The user-defined object that will be passed to the callback.
    pub user_data: Vec<u8>,
}

impl Default for OracleRequest {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for OracleRequest {
    type Error = NativeContractError;

    fn from_stack_item(item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Array(array) = item {
            let request = OracleRequest {
                original_txid: UInt256::from_slice(&array[0].as_bytes()?)?,
                gas_for_response: array[1].as_integer()? as i64,
                url: array[2].as_string()?,
                filter: if array[3].is_null() {
                    None
                } else {
                    Some(array[3].as_string()?)
                },
                callback_contract: UInt160::from_slice(&array[4].as_bytes()?)?,
                callback_method: array[5].as_string()?,
                user_data: array[6].as_bytes()?.to_vec(),
            };
            Ok(request)
        } else {
            Err("Expected Array".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Array(Array::new_with_items(
            vec![
                StackItem::ByteArray(self.original_txid.to_vec()),
                StackItem::Integer(self.gas_for_response.into()),
                StackItem::String(self.url.clone()),
                match &self.filter {
                    Some(f) => StackItem::String(f.clone()),
                    None => StackItem::Null,
                },
                StackItem::ByteArray(self.callback_contract.to_vec()),
                StackItem::String(self.callback_method.clone()),
                StackItem::ByteArray(self.user_data.clone()),
            ],
            reference_counter,
        )))
    }
}

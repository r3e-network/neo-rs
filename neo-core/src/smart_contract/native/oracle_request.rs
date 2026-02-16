//! OracleRequest - matches C# Neo.SmartContract.Native.OracleRequest exactly

use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::stack_item_extract::{extract_bytes, extract_i64, extract_string};
use crate::{UInt160, UInt256};
use neo_vm::StackItem;

/// Oracle request state (matches C# OracleRequest)
#[derive(Clone, Debug)]
pub struct OracleRequest {
    /// The original transaction hash
    pub original_tx_id: UInt256,

    /// The gas for callback
    pub gas_for_response: i64,

    /// The URL to fetch
    pub url: String,

    /// The filter expression
    pub filter: Option<String>,

    /// The callback contract hash
    pub callback_contract: UInt160,

    /// The callback method name
    pub callback_method: String,

    /// User data to pass to callback
    pub user_data: Vec<u8>,
}

impl OracleRequest {
    /// Creates a new oracle request
    pub fn new(
        original_tx_id: UInt256,
        gas_for_response: i64,
        url: String,
        filter: Option<String>,
        callback_contract: UInt160,
        callback_method: String,
        user_data: Vec<u8>,
    ) -> Self {
        Self {
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        }
    }
}

impl IInteroperable for OracleRequest {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 7 {
                return Ok(());
            }

            if let Some(bytes) = extract_bytes(&items[0])
                && bytes.len() == 32
                && let Ok(hash) = UInt256::from_bytes(&bytes)
            {
                self.original_tx_id = hash;
            }

            if let Some(value) = extract_i64(&items[1]) {
                self.gas_for_response = value;
            }

            if let Some(url) = extract_string(&items[2]) {
                self.url = url;
            }

            if matches!(items[3], StackItem::Null) {
                self.filter = None;
            } else if let Some(filter) = extract_string(&items[3]) {
                self.filter = Some(filter);
            }

            if let Some(bytes) = extract_bytes(&items[4])
                && bytes.len() == 20
                && let Ok(hash) = UInt160::from_bytes(&bytes)
            {
                self.callback_contract = hash;
            }

            if let Some(method) = extract_string(&items[5]) {
                self.callback_method = method;
            }

            if let Some(bytes) = extract_bytes(&items[6]) {
                self.user_data = bytes;
            }
        }
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        Ok(StackItem::from_struct(vec![
            StackItem::from_byte_string(self.original_tx_id.to_bytes()),
            StackItem::from_int(self.gas_for_response),
            StackItem::from_byte_string(self.url.as_bytes()),
            match &self.filter {
                Some(filter) => StackItem::from_byte_string(filter.as_bytes()),
                None => StackItem::null(),
            },
            StackItem::from_byte_string(self.callback_contract.to_bytes()),
            StackItem::from_byte_string(self.callback_method.as_bytes()),
            StackItem::from_byte_string(self.user_data.clone()),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

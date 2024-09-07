// Copyright (C) 2015-2024 The Neo Project.
//
// oracle_request.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_contract::prelude::*;
use neo_types::primitives::{u160::UInt160, u256::UInt256};
use neo_vm::types::{Array, StackItem};
use neo_vm::ReferenceCounter;

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

impl Interoperable for OracleRequest {
    fn from_stack_item(&mut self, item: StackItem) -> Result<(), String> {
        if let StackItem::Array(array) = item {
            self.original_txid = UInt256::from_slice(&array[0].as_bytes()?)?;
            self.gas_for_response = array[1].as_integer()? as i64;
            self.url = array[2].as_string()?;
            self.filter = if array[3].is_null() {
                None
            } else {
                Some(array[3].as_string()?)
            };
            self.callback_contract = UInt160::from_slice(&array[4].as_bytes()?)?;
            self.callback_method = array[5].as_string()?;
            self.user_data = array[6].as_bytes()?.to_vec();
            Ok(())
        } else {
            Err("Expected Array".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        StackItem::Array(Array::new_with_items(
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
        ))
    }
}

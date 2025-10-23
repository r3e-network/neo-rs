// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmArrayJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{array::Array, StackItem};
use serde_json::Value;

pub struct VmArrayJsonConverter;

impl VmArrayJsonConverter {
    /// Serialises a VM array into the standard JSON stack-item representation.
    pub fn to_json(value: &Array) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::from_array(value.items().to_vec());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    /// Deserialises a VM array from the JSON stack-item representation.
    pub fn from_json(token: &Value) -> Result<Array, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Array(array) => Ok(array),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Array stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

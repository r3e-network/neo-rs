// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmIntegerJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{integer::Integer, StackItem};
use serde_json::Value;

pub struct VmIntegerJsonConverter;

impl VmIntegerJsonConverter {
    pub fn to_json(value: &Integer) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::Integer(value.value().clone());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Integer, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Integer(value) => Ok(Integer::new(value)),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Integer stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

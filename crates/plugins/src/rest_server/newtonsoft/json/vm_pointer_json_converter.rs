// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmPointerJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{pointer::Pointer, StackItem};
use serde_json::Value;

pub struct VmPointerJsonConverter;

impl VmPointerJsonConverter {
    pub fn to_json(value: &Pointer) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::Pointer(value.clone());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Pointer, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Pointer(pointer) => Ok(pointer),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Pointer stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

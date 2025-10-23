// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmBufferJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{buffer::Buffer, StackItem};
use serde_json::Value;

pub struct VmBufferJsonConverter;

impl VmBufferJsonConverter {
    pub fn to_json(value: &Buffer) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::Buffer(value.clone());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Buffer, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Buffer(buffer) => Ok(buffer),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Buffer stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

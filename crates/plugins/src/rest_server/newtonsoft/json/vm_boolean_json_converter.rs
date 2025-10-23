// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmBooleanJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{boolean::Boolean, StackItem};
use serde_json::Value;

pub struct VmBooleanJsonConverter;

impl VmBooleanJsonConverter {
    pub fn to_json(value: &Boolean) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::Boolean(value.value());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Boolean, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Boolean(value) => Ok(Boolean::new(value)),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Boolean stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

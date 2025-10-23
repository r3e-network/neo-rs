// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmStructJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{struct_item::Struct, StackItem};
use serde_json::Value;

pub struct VmStructJsonConverter;

impl VmStructJsonConverter {
    pub fn to_json(value: &Struct) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::from_struct(value.items().to_vec());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Struct, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Struct(structure) => Ok(structure),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Struct stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

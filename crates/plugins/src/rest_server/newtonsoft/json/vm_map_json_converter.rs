// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmMapJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{map::Map, StackItem};
use serde_json::Value;

pub struct VmMapJsonConverter;

impl VmMapJsonConverter {
    pub fn to_json(value: &Map) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::from_map(value.items().clone());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<Map, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Map(map) => Ok(map),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Map stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

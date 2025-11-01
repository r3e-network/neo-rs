// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmNullJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::StackItem;
use serde_json::Value;

pub struct VmNullJsonConverter;

impl VmNullJsonConverter {
    pub fn to_json() -> Result<Value, RestServerUtilityError> {
        RestServerUtility::stack_item_to_j_token(&StackItem::null())
    }

    pub fn from_json(token: &Value) -> Result<(), RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::Null => Ok(()),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected Null stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

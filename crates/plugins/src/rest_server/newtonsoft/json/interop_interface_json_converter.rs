// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.InteropInterfaceJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::StackItem;
use serde_json::Value;

/// Serialises and deserialises VM `InteropInterface` stack items using the same shape as the C# converter.
pub struct InteropInterfaceJsonConverter;

impl InteropInterfaceJsonConverter {
    pub fn to_json(item: &StackItem) -> Result<Value, RestServerUtilityError> {
        RestServerUtility::stack_item_to_j_token(item)
    }

    pub fn from_json(token: &Value) -> Result<StackItem, RestServerUtilityError> {
        RestServerUtility::stack_item_from_j_token(token)
    }
}

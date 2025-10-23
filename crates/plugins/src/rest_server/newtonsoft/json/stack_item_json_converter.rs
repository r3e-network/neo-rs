// Copyright (C) 2015-2025 The Neo Project.
//
// JSON converter for Neo VM stack items mirroring the behaviour of
// `Neo.Plugins.RestServer.Newtonsoft.Json.StackItemJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::StackItem;
use serde_json::Value;

/// Helper wrapper that exposes JSON conversion utilities for `StackItem`.
pub struct StackItemJsonConverter;

impl StackItemJsonConverter {
    /// Serialises the provided stack item into a JSON value compatible with the C# implementation.
    pub fn to_json(item: &StackItem) -> Result<Value, RestServerUtilityError> {
        RestServerUtility::stack_item_to_j_token(item)
    }

    /// Deserialises the JSON representation produced by [`to_json`] back into a stack item.
    pub fn from_json(token: &Value) -> Result<StackItem, RestServerUtilityError> {
        RestServerUtility::stack_item_from_j_token(token)
    }
}

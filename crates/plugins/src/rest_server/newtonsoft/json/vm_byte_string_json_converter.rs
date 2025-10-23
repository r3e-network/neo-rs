// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.VmByteStringJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_vm::stack_item::{byte_string::ByteString, StackItem};
use serde_json::Value;

pub struct VmByteStringJsonConverter;

impl VmByteStringJsonConverter {
    pub fn to_json(value: &ByteString) -> Result<Value, RestServerUtilityError> {
        let stack_item = StackItem::ByteString(value.data().to_vec());
        RestServerUtility::stack_item_to_j_token(&stack_item)
    }

    pub fn from_json(token: &Value) -> Result<ByteString, RestServerUtilityError> {
        match RestServerUtility::stack_item_from_j_token(token)? {
            StackItem::ByteString(bytes) => Ok(ByteString::new(bytes)),
            other => Err(RestServerUtilityError::StackItem(format!(
                "Expected ByteString stack item, found {:?}",
                other.stack_item_type()
            ))),
        }
    }
}

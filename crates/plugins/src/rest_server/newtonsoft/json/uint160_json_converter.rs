// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.UInt160JsonConverter`.

use crate::rest_server::exceptions::script_hash_format_exception::ScriptHashFormatException;
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_core::UInt160;
use serde_json::Value;
use std::str::FromStr;

pub struct UInt160JsonConverter;

impl UInt160JsonConverter {
    pub fn to_json(value: &UInt160) -> Value {
        Value::String(value.to_string())
    }

    pub fn from_json(token: &Value) -> Result<UInt160, ScriptHashFormatException> {
        let value = token
            .as_str()
            .ok_or_else(|| ScriptHashFormatException::with_message("value must be a string"))?;

        if let Some(system) = RestServerGlobals::neo_system() {
            RestServerUtility::convert_to_script_hash(value, system.settings())
                .map_err(|err| match err {
                    RestServerUtilityError::InvalidAddress(msg) => {
                        ScriptHashFormatException::with_message(msg)
                    }
                    RestServerUtilityError::StackItem(msg) => {
                        ScriptHashFormatException::with_message(msg)
                    }
                })
        } else {
            UInt160::from_str(value).map_err(|_| {
                ScriptHashFormatException::with_message(format!("'{value}' is invalid."))
            })
        }
    }
}

// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_invoke_result.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::vm_state_utils::vm_state_from_str;
use neo_json::{JObject, JToken};
use neo_vm::{StackItem, VMState};

/// RPC invoke result matching C# RpcInvokeResult
#[derive(Debug, Clone)]
pub struct RpcInvokeResult {
    /// The script that was invoked
    pub script: String,

    /// VM execution state
    pub state: VMState,

    /// Gas consumed during execution
    pub gas_consumed: i64,

    /// Stack items after execution
    pub stack: Vec<StackItem>,

    /// Transaction if available
    pub tx: Option<String>,

    /// Exception message if any
    pub exception: Option<String>,

    /// Session ID if available
    pub session: Option<String>,
}

impl RpcInvokeResult {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let script = json
            .get("script")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'script' field")?
            .to_string();

        let state_str = json
            .get("state")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'state' field")?;
        let state = vm_state_from_str(&state_str)
            .ok_or_else(|| format!("Invalid VM state: {}", state_str))?;

        let gas_consumed_str = json
            .get("gasconsumed")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'gasconsumed' field")?;
        let gas_consumed = gas_consumed_str
            .parse::<i64>()
            .map_err(|_| format!("Invalid gas consumed value: {}", gas_consumed_str))?;

        let exception = json
            .get("exception")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let session = json
            .get("session")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let tx = json
            .get("tx")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        // Try to parse stack items
        let stack = json
            .get("stack")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| crate::utility::stack_item_from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            script,
            state,
            gas_consumed,
            stack,
            tx,
            exception,
            session,
        })
    }
}

/// RPC stack item representation matching C# RpcStack
#[derive(Debug, Clone)]
pub struct RpcStack {
    /// Stack item type
    pub item_type: String,

    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let item_type = json
            .get("type")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'type' field")?
            .to_string();

        let value = json.get("value").ok_or("Missing 'value' field")?.clone();

        Ok(Self { item_type, value })
    }
}

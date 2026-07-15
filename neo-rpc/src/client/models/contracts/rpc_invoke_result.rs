use super::super::utility::{
    optional_string, required_string, stack_items_from_json_field, stack_items_to_json,
};
use super::vm_state_utils::{
    insert_gas_consumed_field, insert_vm_state_field, parse_gas_consumed_field,
    parse_vm_state_field,
};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use neo_vm::VmState;

use super::RpcStackItem;

/// RPC invoke result matching C# `RpcInvokeResult`
#[derive(Debug, Clone)]
pub struct RpcInvokeResult {
    /// The script that was invoked
    pub script: String,

    /// VM execution state
    pub state: VmState,

    /// Gas consumed during execution
    pub gas_consumed: i64,

    /// Stack items after execution
    pub stack: Vec<RpcStackItem>,

    /// Transaction if available
    pub tx: Option<String>,

    /// Exception message if any
    pub exception: Option<String>,

    /// Session ID if available
    pub session: Option<String>,
}

impl RpcInvokeResult {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let script =
            required_string(json, "script").map_err(|e| CoreError::other(e.to_string()))?;

        let state = parse_vm_state_field(json, "state")?;
        let gas_consumed = parse_gas_consumed_field(json)?;

        let exception = optional_string(json, "exception");

        let session = optional_string(json, "session");

        let tx = optional_string(json, "tx");

        let stack = stack_items_from_json_field(json, "stack");

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

    /// Converts to JSON
    /// Matches C# `ToJson`
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String(self.script.clone()));
        insert_vm_state_field(&mut json, "state", self.state);
        insert_gas_consumed_field(&mut json, self.gas_consumed);

        if let Some(exception) = &self.exception {
            if !exception.is_empty() {
                json.insert("exception".to_string(), JToken::String(exception.clone()));
            }
        }

        let stack_json = stack_items_to_json(&self.stack)
            .unwrap_or_else(|_| JToken::String("error: recursive reference".to_string()));
        json.insert("stack".to_string(), stack_json);

        if let Some(tx) = &self.tx {
            if !tx.is_empty() {
                json.insert("tx".to_string(), JToken::String(tx.clone()));
            }
        }

        json
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/contracts/rpc_invoke_result.rs"]
mod tests;

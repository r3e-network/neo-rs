use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Captures the payload of a method invocation submitted via RPC.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMethodInvocation {
    /// The contract script being executed.
    pub script: String,
    /// Optional parameters supplied to the invocation.
    #[serde(default)]
    pub parameters: Vec<Value>,
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_method_invocation.rs"]
mod tests;

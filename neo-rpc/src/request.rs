use serde::Deserialize;
use serde_json::Value;

use crate::response::JSONRPC_VERSION;

#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    #[serde(default)]
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default = "default_id")]
    pub id: Value,
}

impl RpcRequest {
    pub fn is_version_valid(&self) -> bool {
        self.jsonrpc == JSONRPC_VERSION
    }
}

fn default_id() -> Value {
    Value::Null
}

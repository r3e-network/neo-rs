use serde::Serialize;
use serde_json::Value;

pub(crate) const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcResponse {
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            result: None,
            error: Some(error),
            id,
        }
    }
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(-32700, message, None)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(-32600, message, None)
    }

    pub fn method_not_found(name: &str) -> Self {
        Self::new(-32601, format!("method '{name}' not found"), None)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message, None)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(-32603, message, None)
    }
}

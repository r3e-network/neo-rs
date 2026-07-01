use serde::{Deserialize, Serialize};

/// Lightweight representation of an oracle response entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcOracleResponse {
    /// Oracle request hash or identifier.
    pub id: u64,
    /// Oracle response code.
    pub code: i32,
    /// Result payload encoded as base64.
    pub result: String,
}

#[cfg(test)]
#[path = "../../../tests/client/models/state/rpc_oracle_response.rs"]
mod tests;

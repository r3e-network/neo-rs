use serde::{Deserialize, Serialize};

/// Model representing the hashes accepted into the mempool when invoking
/// the `getrawmempool` RPC with `true` flag. Mirrors the shape exposed by
/// the C# client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMempoolAccepted {
    /// Transaction hashes currently accepted in the mempool.
    pub hashes: Vec<String>,
}

#[cfg(test)]
#[path = "../../../tests/client/models/ledger/rpc_mempool_accepted.rs"]
mod tests;

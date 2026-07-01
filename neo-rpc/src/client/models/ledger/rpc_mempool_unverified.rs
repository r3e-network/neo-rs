use serde::{Deserialize, Serialize};

/// Model describing transactions pending re-verification in the mempool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMempoolUnverified {
    /// Transaction hashes awaiting re-verification.
    pub hashes: Vec<String>,
}

#[cfg(test)]
#[path = "../../../tests/client/models/ledger/rpc_mempool_unverified.rs"]
mod tests;

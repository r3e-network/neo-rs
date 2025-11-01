// Copyright (C) 2015-2025 The Neo Project.
//
// memory_pool_count_model.rs mirrors Neo.Plugins.RestServer.Models.Ledger.MemoryPoolCountModel.

use serde::{Deserialize, Serialize};

/// Snapshot of the mempool counts (total/verified/unverified) exposed by the REST server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct MemoryPoolCountModel {
    /// Total transactions tracked by the mempool.
    pub count: usize,
    /// Count of unverified transactions.
    pub un_verified_count: usize,
    /// Count of verified transactions.
    pub verified_count: usize,
}

impl MemoryPoolCountModel {
    pub fn new(count: usize, un_verified: usize, verified: usize) -> Self {
        Self {
            count,
            un_verified_count: un_verified,
            verified_count: verified,
        }
    }
}

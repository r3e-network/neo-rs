// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Models.Blockchain.AccountDetails.

use neo_core::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountDetails {
    /// Account script hash (hex string in JSON via `UInt160` serializer).
    pub script_hash: UInt160,
    /// Associated Neo address.
    pub address: String,
    /// Token balance held by the account.
    pub balance: BigInt,
    /// Token decimals for the reported balance.
    pub decimals: i32,
}

impl AccountDetails {
    pub fn new(
        script_hash: UInt160,
        address: impl Into<String>,
        balance: BigInt,
        decimals: i32,
    ) -> Self {
        Self {
            script_hash,
            address: address.into(),
            balance,
            decimals,
        }
    }
}

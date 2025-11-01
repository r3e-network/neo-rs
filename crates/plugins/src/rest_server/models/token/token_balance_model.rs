//! Rust port of `Neo.Plugins.RestServer.Models.Token.TokenBalanceModel`.

use neo_core::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Represents an account balance for a fungible or non-fungible token.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct TokenBalanceModel {
    pub name: String,
    pub script_hash: UInt160,
    pub symbol: String,
    pub decimals: u8,
    pub balance: BigInt,
    pub total_supply: BigInt,
}

impl TokenBalanceModel {
    pub fn new(
        name: impl Into<String>,
        script_hash: UInt160,
        symbol: impl Into<String>,
        decimals: u8,
        balance: BigInt,
        total_supply: BigInt,
    ) -> Self {
        Self {
            name: name.into(),
            script_hash,
            symbol: symbol.into(),
            decimals,
            balance,
            total_supply,
        }
    }
}

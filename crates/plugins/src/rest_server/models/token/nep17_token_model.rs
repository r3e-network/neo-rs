//! Rust port of `Neo.Plugins.RestServer.Models.Token.NEP17TokenModel`.

use neo_core::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Summary information for a NEP-17 token.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Nep17TokenModel {
    pub name: String,
    pub script_hash: UInt160,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: BigInt,
}

impl Nep17TokenModel {
    pub fn new(
        name: impl Into<String>,
        script_hash: UInt160,
        symbol: impl Into<String>,
        decimals: u8,
        total_supply: BigInt,
    ) -> Self {
        Self {
            name: name.into(),
            script_hash,
            symbol: symbol.into(),
            decimals,
            total_supply,
        }
    }
}

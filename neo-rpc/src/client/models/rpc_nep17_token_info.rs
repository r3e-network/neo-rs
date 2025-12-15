// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep17_token_info.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP17 token information matching C# RpcNep17TokenInfo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17TokenInfo {
    /// Token name
    pub name: String,

    /// Token symbol
    pub symbol: String,

    /// Number of decimals
    pub decimals: u8,

    /// Total supply
    pub total_supply: BigInt,

    /// Optional balance for a specific address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<BigInt>,

    /// Optional last updated block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_block: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_info_roundtrip() {
        let info = RpcNep17TokenInfo {
            name: "TestToken".to_string(),
            symbol: "TT".to_string(),
            decimals: 8,
            total_supply: BigInt::from(1_000_000),
            balance: Some(BigInt::from(42)),
            last_updated_block: Some(123),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: RpcNep17TokenInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, info.name);
        assert_eq!(parsed.symbol, info.symbol);
        assert_eq!(parsed.decimals, info.decimals);
        assert_eq!(parsed.total_supply, info.total_supply);
        assert_eq!(parsed.balance, info.balance);
        assert_eq!(parsed.last_updated_block, info.last_updated_block);
    }
}

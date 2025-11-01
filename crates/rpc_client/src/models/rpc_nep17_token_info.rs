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

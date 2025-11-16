// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep17_balances.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, UInt160};
use neo_json::{JArray, JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// NEP17 balances for an address matching C# RpcNep17Balances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balances {
    /// User script hash
    pub user_script_hash: UInt160,

    /// List of token balances
    pub balances: Vec<RpcNep17Balance>,
}

impl RpcNep17Balances {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();

        let balances_array: Vec<JToken> = self
            .balances
            .iter()
            .map(|b| JToken::Object(b.to_json()))
            .collect();
        json.insert(
            "balance".to_string(),
            JToken::Array(JArray::from(balances_array)),
        );

        json.insert(
            "address".to_string(),
            JToken::String(self.user_script_hash.to_address()),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let balances = json
            .get("balance")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcNep17Balance::from_json(obj, protocol_settings).ok())
                    .collect()
            })
            .unwrap_or_default();

        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?;

        let user_script_hash =
            UInt160::from_address(&address).map_err(|_| format!("Invalid address: {}", address))?;

        Ok(Self {
            user_script_hash,
            balances,
        })
    }
}

/// Individual NEP17 balance entry matching C# RpcNep17Balance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balance {
    /// Asset hash
    pub asset_hash: UInt160,

    /// Balance amount
    pub amount: BigInt,

    /// Last updated block height
    pub last_updated_block: u32,
}

impl RpcNep17Balance {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
        );
        json.insert(
            "amount".to_string(),
            JToken::String(self.amount.to_string()),
        );
        json.insert(
            "lastupdatedblock".to_string(),
            JToken::Number(self.last_updated_block as f64),
        );
        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let asset_hash_str = json
            .get("assethash")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'assethash' field")?;

        let asset_hash = if asset_hash_str.starts_with("0x") {
            UInt160::parse(&asset_hash_str)
        } else {
            UInt160::from_address(&asset_hash_str)
        }
        .map_err(|_| format!("Invalid asset hash: {}", asset_hash_str))?;

        let amount_str = json
            .get("amount")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'amount' field")?;
        let amount =
            BigInt::from_str(&amount_str).map_err(|_| format!("Invalid amount: {}", amount_str))?;

        let last_updated_block =
            json.get("lastupdatedblock")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'lastupdatedblock' field")? as u32;

        Ok(Self {
            asset_hash,
            amount,
            last_updated_block,
        })
    }
}

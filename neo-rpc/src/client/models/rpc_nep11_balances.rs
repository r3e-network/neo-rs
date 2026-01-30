// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep11_balances.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JArray, JObject, JToken};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// NEP11 balances for an address matching C# `RpcNep11Balances`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11Balances {
    /// User script hash.
    pub user_script_hash: UInt160,
    /// List of NEP11 asset balances.
    pub balances: Vec<RpcNep11Balance>,
}

impl RpcNep11Balances {
    /// Converts to JSON.
    #[must_use]
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
            JToken::String(WalletHelper::to_address(
                &self.user_script_hash,
                protocol_settings.address_version,
            )),
        );

        json
    }

    /// Creates from JSON.
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let balances = json
            .get("balance")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcNep11Balance::from_json(obj).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let address = json
            .get("address")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'address' field")?;

        let user_script_hash = if address.starts_with("0x") {
            UInt160::parse(&address).map_err(|_| format!("Invalid address: {address}"))?
        } else {
            WalletHelper::to_script_hash(&address, protocol_settings.address_version)
                .map_err(|err| format!("Invalid address: {err}"))?
        };

        Ok(Self {
            user_script_hash,
            balances,
        })
    }
}

/// Individual NEP11 balance per asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11Balance {
    /// Asset hash.
    pub asset_hash: UInt160,
    /// Asset name.
    pub name: String,
    /// Symbol.
    pub symbol: String,
    /// Decimals.
    pub decimals: u8,
    /// Tokens held for this asset.
    pub tokens: Vec<RpcNep11TokenBalance>,
}

impl RpcNep11Balance {
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
        );
        json.insert("name".to_string(), JToken::String(self.name.clone()));
        json.insert("symbol".to_string(), JToken::String(self.symbol.clone()));
        json.insert(
            "decimals".to_string(),
            JToken::String(self.decimals.to_string()),
        );
        let tokens_array: Vec<JToken> = self
            .tokens
            .iter()
            .map(|t| JToken::Object(t.to_json()))
            .collect();
        json.insert(
            "tokens".to_string(),
            JToken::Array(JArray::from(tokens_array)),
        );
        json
    }

    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let asset_hash_str = json
            .get("assethash")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'assethash' field")?;
        let asset_hash = UInt160::parse(&asset_hash_str)
            .map_err(|_| format!("Invalid asset hash: {asset_hash_str}"))?;

        let name = json
            .get("name")
            .and_then(neo_json::JToken::as_string)
            .unwrap_or_default();
        let symbol = json
            .get("symbol")
            .and_then(neo_json::JToken::as_string)
            .unwrap_or_default();

        let decimals_token = json.get("decimals");
        let decimals = match decimals_token.and_then(neo_json::JToken::as_string) {
            Some(text) => text.parse::<u8>().unwrap_or(0),
            None => decimals_token
                .and_then(neo_json::JToken::as_number)
                .map_or(0, |n| n as u8),
        };

        let tokens = json
            .get("tokens")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcNep11TokenBalance::from_json(obj).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(Self {
            asset_hash,
            name,
            symbol,
            decimals,
            tokens,
        })
    }
}

/// Balance of a specific NEP11 token id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11TokenBalance {
    /// Token id bytes.
    pub token_id: Vec<u8>,
    /// Amount.
    pub amount: BigInt,
    /// Last updated block.
    pub last_updated_block: u32,
}

impl RpcNep11TokenBalance {
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "tokenid".to_string(),
            JToken::String(hex::encode(&self.token_id)),
        );
        json.insert(
            "amount".to_string(),
            JToken::String(self.amount.to_string()),
        );
        json.insert(
            "lastupdatedblock".to_string(),
            JToken::Number(f64::from(self.last_updated_block)),
        );
        json
    }

    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let token_id_str = json
            .get("tokenid")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'tokenid' field")?;
        let token_id = hex::decode(token_id_str.trim_start_matches("0x"))
            .map_err(|_| format!("Invalid tokenid: {token_id_str}"))?;

        let amount_str = json
            .get("amount")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'amount' field")?;
        let amount =
            BigInt::from_str(&amount_str).map_err(|_| format!("Invalid amount: {amount_str}"))?;

        let last_updated_block =
            json.get("lastupdatedblock")
                .and_then(neo_json::JToken::as_number)
                .ok_or("Missing or invalid 'lastupdatedblock' field")? as u32;

        Ok(Self {
            token_id,
            amount,
            last_updated_block,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_balance_roundtrip() {
        let entry = RpcNep11TokenBalance {
            token_id: vec![0x01],
            amount: BigInt::from(42),
            last_updated_block: 7,
        };
        let json = entry.to_json();
        let parsed = RpcNep11TokenBalance::from_json(&json).unwrap();
        assert_eq!(parsed.token_id, entry.token_id);
        assert_eq!(parsed.amount, entry.amount);
        assert_eq!(parsed.last_updated_block, entry.last_updated_block);
    }

    #[test]
    fn balances_roundtrip() {
        let settings = ProtocolSettings::default_settings();
        let entry = RpcNep11TokenBalance {
            token_id: vec![0x01],
            amount: BigInt::from(5),
            last_updated_block: 3,
        };
        let balance = RpcNep11Balance {
            asset_hash: UInt160::zero(),
            name: "Test".to_string(),
            symbol: "T".to_string(),
            decimals: 0,
            tokens: vec![entry.clone()],
        };
        let balances = RpcNep11Balances {
            user_script_hash: UInt160::zero(),
            balances: vec![balance.clone()],
        };
        let json = balances.to_json(&settings);
        let parsed = RpcNep11Balances::from_json(&json, &settings).unwrap();
        assert_eq!(parsed.user_script_hash, balances.user_script_hash);
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].asset_hash, balance.asset_hash);
        assert_eq!(parsed.balances[0].tokens[0].amount, entry.amount);
    }
}

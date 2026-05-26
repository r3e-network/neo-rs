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

use super::super::utility::{
    object_array, parse_object_array_lossy, required_address_script_hash, required_bigint_string,
    required_string, required_u32_number,
};
use neo_config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JObject, JToken};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

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

        json.insert(
            "balance".to_string(),
            object_array(&self.balances, RpcNep11Balance::to_json),
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
        let balances = parse_object_array_lossy(json, "balance", RpcNep11Balance::from_json);
        let user_script_hash = required_address_script_hash(json, "address", protocol_settings)?;

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
        json.insert(
            "tokens".to_string(),
            object_array(&self.tokens, RpcNep11TokenBalance::to_json),
        );
        json
    }

    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let asset_hash_str = required_string(json, "assethash")?;
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

        let tokens = parse_object_array_lossy(json, "tokens", RpcNep11TokenBalance::from_json);

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
        let token_id_str = required_string(json, "tokenid")?;
        let token_id = hex::decode(token_id_str.trim_start_matches("0x"))
            .map_err(|_| format!("Invalid tokenid: {token_id_str}"))?;

        let amount = required_bigint_string(json, "amount", "amount")?;
        let last_updated_block = required_u32_number(json, "lastupdatedblock")?;

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
    use neo_json::JArray;

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

    #[test]
    fn balances_and_tokens_arrays_keep_lossy_parse_behavior() {
        let settings = ProtocolSettings::default_settings();

        let valid_token = RpcNep11TokenBalance {
            token_id: vec![0x01],
            amount: BigInt::from(5),
            last_updated_block: 3,
        }
        .to_json();

        let mut malformed_token = JObject::new();
        malformed_token.insert("tokenid".to_string(), JToken::String("01".to_string()));

        let mut tokens = JArray::new();
        tokens.add(Some(JToken::Object(valid_token)));
        tokens.add(None);
        tokens.add(Some(JToken::String("not an object".to_string())));
        tokens.add(Some(JToken::Object(malformed_token)));

        let mut valid_balance = RpcNep11Balance {
            asset_hash: UInt160::zero(),
            name: "Test".to_string(),
            symbol: "T".to_string(),
            decimals: 0,
            tokens: Vec::new(),
        }
        .to_json();
        valid_balance.insert("tokens".to_string(), JToken::Array(tokens));

        let mut malformed_balance = JObject::new();
        malformed_balance.insert(
            "name".to_string(),
            JToken::String("missing hash".to_string()),
        );

        let mut balances = JArray::new();
        balances.add(Some(JToken::Object(valid_balance)));
        balances.add(None);
        balances.add(Some(JToken::String("not an object".to_string())));
        balances.add(Some(JToken::Object(malformed_balance)));

        let mut root = JObject::new();
        root.insert("balance".to_string(), JToken::Array(balances));
        root.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &UInt160::zero(),
                settings.address_version,
            )),
        );

        let parsed = RpcNep11Balances::from_json(&root, &settings).unwrap();
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].tokens.len(), 1);
        assert_eq!(parsed.balances[0].tokens[0].amount, BigInt::from(5));
    }
}

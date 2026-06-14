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
    NepBalanceFieldRefs, balance_list_to_json, insert_nep_balance_fields, object_array,
    parse_balance_list, parse_nep_balance_fields, parse_object_array_lossy, required_string,
};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_serialization::json::{JObject, JToken};
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
        balance_list_to_json(
            &self.balances,
            &self.user_script_hash,
            protocol_settings,
            RpcNep11Balance::to_json,
        )
    }

    /// Creates from JSON.
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let (balances, user_script_hash) =
            parse_balance_list(json, protocol_settings, RpcNep11Balance::from_json)?;

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

    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let asset_hash_str = required_string(json, "assethash")
            .map_err(|e| CoreError::other(e.to_string()))?;
        let asset_hash = UInt160::parse(&asset_hash_str)
            .map_err(|_| CoreError::other(format!("Invalid asset hash: {asset_hash_str}")))?;

        let name = json
            .get("name")
            .and_then(neo_serialization::json::JToken::as_string)
            .unwrap_or_default();
        let symbol = json
            .get("symbol")
            .and_then(neo_serialization::json::JToken::as_string)
            .unwrap_or_default();

        let decimals_token = json.get("decimals");
        let decimals = match decimals_token.and_then(neo_serialization::json::JToken::as_string) {
            Some(text) => text.parse::<u8>().unwrap_or(0),
            None => decimals_token
                .and_then(neo_serialization::json::JToken::as_number)
                .map_or(0, |n| n as u8),
        };

        let tokens = parse_object_array_lossy(json, "tokens", |obj| {
            RpcNep11TokenBalance::from_json(obj).map_err(|e| e.to_string())
        });

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
        insert_nep_balance_fields(
            &mut json,
            NepBalanceFieldRefs {
                amount: &self.amount,
                last_updated_block: self.last_updated_block,
            },
        );
        json
    }

    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let token_id_str = required_string(json, "tokenid")
            .map_err(|e| CoreError::other(e.to_string()))?;
        let token_id = hex::decode(token_id_str.trim_start_matches("0x"))
            .map_err(|_| CoreError::other(format!("Invalid tokenid: {token_id_str}")))?;

        let fields = parse_nep_balance_fields(json)?;

        Ok(Self {
            token_id,
            amount: fields.amount,
            last_updated_block: fields.last_updated_block,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_serialization::json::JArray;
    use neo_wallets::wallet_helper as WalletHelper;

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
    fn token_balance_to_json_keeps_tokenid_before_shared_fields() {
        let entry = RpcNep11TokenBalance {
            token_id: vec![0x01, 0x02],
            amount: BigInt::from(42),
            last_updated_block: 7,
        };

        assert_eq!(
            entry.to_json().to_string(),
            r#"{"tokenid":"0102","amount":"42","lastupdatedblock":7}"#
        );
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

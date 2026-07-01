use super::super::utility::{
    NepBalanceFieldRefs, balance_list_to_json, insert_nep_balance_fields, object_array,
    parse_balance_list, parse_nep_balance_fields, parse_object_array_lossy, required_string,
};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, strip_hex_prefix};
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
    /// Convert this asset balance to its Neo JSON-RPC representation.
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

    /// Parse an asset balance from its Neo JSON-RPC representation.
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let asset_hash_str =
            required_string(json, "assethash").map_err(|e| CoreError::other(e.to_string()))?;
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
    /// Convert this token balance to its Neo JSON-RPC representation.
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

    /// Parse a token balance from its Neo JSON-RPC representation.
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let token_id_str =
            required_string(json, "tokenid").map_err(|e| CoreError::other(e.to_string()))?;
        let token_id = hex::decode(strip_hex_prefix(&token_id_str))
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
#[path = "../../../tests/client/models/tokens/rpc_nep11_balances.rs"]
mod tests;

use super::super::utility::{
    NepBalanceFieldRefs, balance_list_to_json, insert_nep_balance_fields, parse_balance_list,
    parse_nep_balance_fields, required_script_hash_or_address,
};
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP17 balances for an address matching C# `RpcNep17Balances`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balances {
    /// User script hash
    pub user_script_hash: UInt160,

    /// List of token balances
    pub balances: Vec<RpcNep17Balance>,
}

impl RpcNep17Balances {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        balance_list_to_json(
            &self.balances,
            &self.user_script_hash,
            protocol_settings,
            RpcNep17Balance::to_json,
        )
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let (balances, user_script_hash) = parse_balance_list(json, protocol_settings, |obj| {
            RpcNep17Balance::from_json(obj, protocol_settings)
        })?;

        Ok(Self {
            user_script_hash,
            balances,
        })
    }
}

/// Individual NEP17 balance entry matching C# `RpcNep17Balance`
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
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
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

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, _protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let asset_hash =
            required_script_hash_or_address(json, "assethash", _protocol_settings, "asset hash")?;
        let fields = parse_nep_balance_fields(json)?;

        Ok(Self {
            asset_hash,
            amount: fields.amount,
            last_updated_block: fields.last_updated_block,
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/tokens/rpc_nep17_balances.rs"]
mod tests;
